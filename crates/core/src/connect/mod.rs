//! SSH connection pipeline: [JumpHost] -> Tcp -> HostKey -> Auth -> Channel -> Shell.

pub mod auth;
pub mod classify;
pub mod handler;
pub mod prompt;
pub mod reconnect;
mod sftp_session;
pub mod ssh_session;

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use russh::client;
use russh::keys::Algorithm;
use russh_sftp::client::RawSftpSession;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub use classify::{classify_io, classify_resolve_error, map_russh_error, network_error, sanitize_log_line};
pub use handler::{ClientHandler, HandlerError, HostKeyAction, decide_host_key};
pub use prompt::{EmitFn, HostKeyDecision, KbdPrompt, Prompt, PromptAnswer, PromptBroker};
pub use reconnect::{BACKOFF_SECS, ConnectFuture, Connector, Enabled, MAX_ATTEMPTS, ReconnectingSession, is_fatal};
pub use sftp_session::{LostFn, SftpSession};
pub use ssh_session::SshSession;

use crate::error::{CoreError, NetworkKind, Result};
use crate::known_hosts::KnownHosts;
use crate::model::{AuthMethod, Host, LoginProfile};
use crate::secrets::SecretStore;
use crate::session::{OutputSink, SessionId};
use crate::sftp::{Listing, join};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    JumpHost,
    Tcp,
    HostKey,
    Auth,
    Channel,
    Shell,
    Subsystem,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ConnectEvent {
    Steps {
        steps: Vec<Step>,
    },
    StepStarted {
        step: Step,
    },
    StepDone {
        step: Step,
    },
    StepFailed {
        step: Step,
        error: CoreError,
    },
    /// Sanitized `ssh -v` style log line.
    Log {
        line: String,
    },
    Prompt {
        prompt_id: u64,
        prompt: Prompt,
    },
    Connected {
        session_id: SessionId,
    },
    Failed {
        error: CoreError,
    },
}

/// What was negotiated for an established connection; shown in the terminal sidebar.
/// Never add secrets (passwords, passphrases, key material, prompt answers) to this struct.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub host_key_algorithm: String,
    pub host_key_sha256: String,
    pub kex: String,
    pub cipher: String,
    /// Client→server MAC exactly as russh names it (a placeholder for AEAD ciphers).
    pub mac: String,
    /// Methods of the successful login, in order (two for 2FA).
    pub auth_methods: Vec<AuthMethod>,
}

/// Filled by the handler during the handshake (host key, negotiated algorithms).
pub(crate) type InfoSlot = Arc<Mutex<SessionInfo>>;

/// russh's default host-key algorithm list with the `known` types (from known_hosts) moved to the
/// front; both parts keep the default order. A stored `ssh-rsa` key matches all RSA signature
/// algorithms (rsa-sha2-512, rsa-sha2-256, ssh-rsa). Types russh does not offer are ignored.
pub fn preferred_host_key_order(known: &[Algorithm]) -> Vec<Algorithm> {
    let is_rsa = |a: &Algorithm| matches!(a, Algorithm::Rsa { .. });
    let is_known = |candidate: &Algorithm| known.iter().any(|k| k == candidate || (is_rsa(k) && is_rsa(candidate)));
    let defaults = russh::Preferred::default().key;
    let (mut first, rest): (Vec<Algorithm>, Vec<Algorithm>) = defaults.iter().cloned().partition(|a| is_known(a));
    first.extend(rest);
    first
}

/// Build a `Log` event; the line is always sanitized.
pub fn log_line(line: impl Into<String>) -> ConnectEvent {
    ConnectEvent::Log { line: sanitize_log_line(&line.into()) }
}

#[derive(Debug, Clone)]
pub struct ConnectTarget {
    pub host: Host,
    pub profile: Option<LoginProfile>,
}

#[derive(Debug, Clone)]
pub struct ConnectRequest {
    pub target: ConnectTarget,
    pub jump: Option<ConnectTarget>,
    pub cols: u16,
    pub rows: u16,
}

pub struct ConnectDeps {
    pub known_hosts: KnownHosts,
    pub secrets: Arc<dyn SecretStore>,
    pub agent_sock: Option<PathBuf>,
}

/// Steps shown in the loading screen. `JumpHost` only with a jump host.
pub fn plan_steps(req: &ConnectRequest) -> Vec<Step> {
    let mut steps = Vec::with_capacity(6);
    if req.jump.is_some() {
        steps.push(Step::JumpHost);
    }
    steps.extend([Step::Tcp, Step::HostKey, Step::Auth, Step::Channel, Step::Shell]);
    steps
}

/// Steps of an SFTP connect: the SSH part, then subsystem and start directory.
pub fn plan_sftp_steps(req: &ConnectRequest) -> Vec<Step> {
    let mut steps = plan_steps(req);
    steps.truncate(steps.len() - 2);
    steps.extend([Step::Subsystem, Step::Directory]);
    steps
}

/// Byte stream an SSH handshake can run over (TCP or a jump tunnel).
trait Transport: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Transport for T {}

const TERM: &str = "xterm-256color";

/// Run `fut` with a deadline of `timeout_s` seconds (0 = no deadline).
/// The deadline is disarmed for good the first time `disarm` fires, so a
/// step that notifies it (e.g. once the server's host key has arrived,
/// before a possibly slow user prompt) is only bounded up to that point.
/// Passing a `Notify` that is never triggered yields a plain, full timeout.
async fn with_setup_timeout<T>(
    timeout_s: u32,
    disarm: Arc<Notify>,
    timeout_message: impl FnOnce() -> String,
    fut: impl Future<Output = Result<T>>,
) -> Result<T> {
    if timeout_s == 0 {
        return fut.await;
    }
    tokio::pin!(fut);
    let sleep = tokio::time::sleep(Duration::from_secs(u64::from(timeout_s)));
    tokio::pin!(sleep);
    let mut armed = true;
    loop {
        tokio::select! {
            result = &mut fut => return result,
            () = &mut sleep, if armed => {
                return Err(network_error(NetworkKind::Timeout, timeout_message()));
            }
            () = disarm.notified() => armed = false,
        }
    }
}

/// Shared pipeline context for one connect attempt.
struct Pipeline {
    known_hosts: Arc<KnownHosts>,
    secrets: Arc<dyn SecretStore>,
    agent_sock: Option<PathBuf>,
    broker: Arc<PromptBroker>,
    emit: EmitFn,
    cancel: CancellationToken,
}

/// An authenticated connection to the target (plus jump connection).
struct Established {
    handle: client::Handle<ClientHandler>,
    jump: Option<client::Handle<ClientHandler>>,
    info: SessionInfo,
}

impl Pipeline {
    fn new(deps: ConnectDeps, broker: Arc<PromptBroker>, emit: EmitFn, cancel: CancellationToken) -> Self {
        Self {
            known_hosts: Arc::new(deps.known_hosts),
            secrets: deps.secrets,
            agent_sock: deps.agent_sock,
            broker,
            emit,
            cancel,
        }
    }

    fn log(&self, line: impl Into<String>) {
        (self.emit)(log_line(line));
    }

    /// Run one step: emits Started/Done/Failed and aborts on cancellation.
    async fn step<T, F>(&self, step: Step, fut: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        (self.emit)(ConnectEvent::StepStarted { step });
        let result = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(CoreError::Cancelled),
            r = fut => r,
        };
        match &result {
            Ok(_) => (self.emit)(ConnectEvent::StepDone { step }),
            Err(error) => (self.emit)(ConnectEvent::StepFailed { step, error: error.clone() }),
        }
        result
    }

    /// Resolve and connect with the host's timeout (0 = no timeout).
    async fn tcp_connect(&self, host: &Host) -> Result<TcpStream> {
        let address = host.address.clone();
        let port = host.port;
        let connect = async {
            self.log(format!("debug1: Connecting to {address} port {port}."));
            let addrs: Vec<_> = tokio::net::lookup_host((address.as_str(), port))
                .await
                .map_err(|e| {
                    network_error(classify_resolve_error(&e), format!("Could not resolve hostname {address}: {e}"))
                })?
                .collect();
            if addrs.is_empty() {
                return Err(network_error(
                    NetworkKind::Dns,
                    format!("Could not resolve hostname {address}: no addresses"),
                ));
            }
            let mut last_error = None;
            for addr in addrs {
                match TcpStream::connect(addr).await {
                    Ok(stream) => {
                        let _ = stream.set_nodelay(true);
                        self.log(format!("debug1: Connection established to {addr}."));
                        return Ok(stream);
                    }
                    Err(e) => {
                        self.log(format!("debug1: connect to {addr} failed: {e}"));
                        last_error = Some(e);
                    }
                }
            }
            let e = last_error.expect("at least one address was tried");
            Err(network_error(classify_io(&e), format!("connect to host {address} port {port}: {e}")))
        };
        if host.timeout_s == 0 {
            return connect.await;
        }
        match tokio::time::timeout(Duration::from_secs(u64::from(host.timeout_s)), connect).await {
            Ok(result) => result,
            Err(_) => Err(network_error(
                NetworkKind::Timeout,
                format!("connect to host {address} port {port}: Connection timed out after {}s", host.timeout_s),
            )),
        }
    }

    fn client_config(host: &Host, host_key_order: Vec<Algorithm>) -> Arc<client::Config> {
        Arc::new(client::Config {
            keepalive_interval: (host.keepalive_s > 0).then(|| Duration::from_secs(u64::from(host.keepalive_s))),
            keepalive_max: 3,
            nodelay: true,
            preferred: russh::Preferred { key: host_key_order.into(), ..russh::Preferred::default() },
            ..Default::default()
        })
    }

    /// SSH handshake incl. host key verification over `stream`. Bounded by
    /// `host.timeout_s` only up to the moment the server's host key reaches
    /// the handler; time spent in a host-key prompt (or the rest of kex
    /// after that) is not counted.
    async fn handshake(
        &self,
        host: &Host,
        stream: Box<dyn Transport>,
        negotiated: InfoSlot,
    ) -> Result<client::Handle<ClientHandler>> {
        let key_seen = Arc::new(Notify::new());
        let handler = ClientHandler::new(
            host.address.clone(),
            host.port,
            self.known_hosts.clone(),
            self.broker.clone(),
            self.emit.clone(),
            self.cancel.clone(),
            key_seen.clone(),
            negotiated,
        );
        // Like OpenSSH: offer the key types known_hosts already holds for this host first, so a
        // host known only by e.g. its ecdsa/rsa key negotiates that type and verifies as Known.
        // A read error only loses the ordering; `check` reports it (fail closed) during kex.
        let known = self.known_hosts.known_algorithms(&host.address, host.port).unwrap_or_default();
        if !known.is_empty() {
            let names: Vec<&str> = known.iter().map(Algorithm::as_str).collect();
            self.log(format!("debug1: preferring known host key types: {}", names.join(",")));
        }
        let config = Self::client_config(host, preferred_host_key_order(&known));
        with_setup_timeout(
            host.timeout_s,
            key_seen,
            || {
                format!(
                    "connect to host {} port {}: connection timed out during SSH handshake",
                    host.address, host.port
                )
            },
            async { client::connect_stream(config, stream, handler).await.map_err(CoreError::from) },
        )
        .await
    }

    async fn authenticate(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        target: &ConnectTarget,
    ) -> Result<Vec<AuthMethod>> {
        let user = auth::effective_user(target)?;
        let (auth_config, owner) = auth::resolve_auth(target)?;
        let ctx = auth::AuthContext {
            user,
            host: target.host.address.clone(),
            auth: auth_config,
            owner,
            secrets: self.secrets.clone(),
            agent_sock: self.agent_sock.as_deref(),
            broker: &self.broker,
            emit: &self.emit,
            cancel: &self.cancel,
        };
        let mut authenticator = auth::HandleAuthenticator { handle };
        auth::authenticate(&mut authenticator, &ctx).await
    }

    /// Full connection to the jump host (own host key check and auth).
    async fn connect_jump(&self, jump: &ConnectTarget) -> Result<client::Handle<ClientHandler>> {
        self.log(format!("debug1: Connecting to jump host {}", jump.host.address));
        let stream = self.tcp_connect(&jump.host).await?;
        let mut handle = self.handshake(&jump.host, Box::new(stream), InfoSlot::default()).await?;
        self.authenticate(&mut handle, jump).await?;
        Ok(handle)
    }

    /// Steps JumpHost? -> Tcp -> HostKey -> Auth.
    async fn establish(&self, req: &ConnectRequest) -> Result<Established> {
        let jump = match &req.jump {
            Some(jump_target) => Some(self.step(Step::JumpHost, self.connect_jump(jump_target)).await?),
            None => None,
        };
        let host = &req.target.host;
        let stream: Box<dyn Transport> = self
            .step(Step::Tcp, async {
                match &jump {
                    Some(jump_handle) => {
                        self.log(format!(
                            "debug1: Opening direct-tcpip channel to {}:{} via jump host",
                            host.address, host.port
                        ));
                        let open = jump_handle.channel_open_direct_tcpip(
                            host.address.clone(),
                            u32::from(host.port),
                            "127.0.0.1",
                            0,
                        );
                        let channel = with_setup_timeout(
                            host.timeout_s,
                            Arc::new(Notify::new()),
                            || {
                                format!(
                                    "jump host could not connect to {}:{}: timed out after {}s",
                                    host.address, host.port, host.timeout_s
                                )
                            },
                            async {
                                open.await.map_err(|e| {
                                    network_error(
                                        NetworkKind::Other,
                                        format!("jump host could not connect to {}:{}: {e}", host.address, host.port),
                                    )
                                })
                            },
                        )
                        .await?;
                        Ok(Box::new(channel.into_stream()) as Box<dyn Transport>)
                    }
                    None => Ok(Box::new(self.tcp_connect(host).await?) as Box<dyn Transport>),
                }
            })
            .await?;
        let negotiated = InfoSlot::default();
        let mut handle = self.step(Step::HostKey, self.handshake(host, stream, negotiated.clone())).await?;
        let auth_methods = self.step(Step::Auth, self.authenticate(&mut handle, &req.target)).await?;
        let mut info = negotiated.lock().unwrap_or_else(PoisonError::into_inner).clone();
        info.auth_methods = auth_methods;
        Ok(Established { handle, jump, info })
    }

    /// Steps Channel -> Shell; returns the running session.
    async fn open_shell(&self, req: &ConnectRequest, est: Established, sink: OutputSink) -> Result<SshSession> {
        let host = &req.target.host;
        let channel = self
            .step(Step::Channel, async {
                self.log("debug1: Requesting session channel");
                let channel = est.handle.channel_open_session().await.map_err(map_russh_error)?;
                self.log(format!("debug1: Requesting PTY {TERM} {}x{}", req.cols, req.rows));
                channel
                    .request_pty(false, TERM, u32::from(req.cols), u32::from(req.rows), 0, 0, &[])
                    .await
                    .map_err(map_russh_error)?;
                for var in &host.env {
                    self.log(format!("debug1: Sending env {}", var.key));
                    channel.set_env(false, var.key.clone(), var.value.clone()).await.map_err(map_russh_error)?;
                }
                Ok(channel)
            })
            .await?;
        self.step(Step::Shell, async {
            self.log("debug1: Requesting shell");
            channel.request_shell(false).await.map_err(map_russh_error)?;
            if let Some(command) = host.start_command.as_deref().filter(|c| !c.trim().is_empty()) {
                self.log("debug1: Sending start command");
                channel.data(format!("{command}\n").as_bytes()).await.map_err(map_russh_error)?;
            }
            Ok(())
        })
        .await?;
        Ok(SshSession::start(channel, est.handle, est.jump, est.info, sink))
    }
}

async fn disconnect(est: Established) {
    let _ = est.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    if let Some(jump) = est.jump {
        let _ = jump.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    }
}

impl Pipeline {
    /// Steps Subsystem -> Directory; closes the connection again on failure.
    async fn open_sftp(
        &self,
        req: &ConnectRequest,
        est: Established,
        on_lost: LostFn,
    ) -> Result<(SftpSession, Listing)> {
        let host = &req.target.host;
        let Established { handle, jump, info } = est;
        let opened = self
            .step(Step::Subsystem, async {
                self.log("debug1: Requesting subsystem sftp");
                let channel = handle.channel_open_session().await.map_err(map_russh_error)?;
                channel.request_subsystem(false, "sftp").await.map_err(map_russh_error)?;
                let raw = RawSftpSession::new(channel.into_stream());
                if host.timeout_s > 0 {
                    raw.set_timeout(u64::from(host.timeout_s));
                }
                let version = raw.init().await.map_err(|e| sftp_session::map_sftp_error(e, ""))?;
                self.log(format!("debug1: SFTP protocol version {}", version.version));
                Ok((raw, version.version))
            })
            .await;
        let (raw, version) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                disconnect(Established { handle, jump, info }).await;
                return Err(error);
            }
        };
        let session = SftpSession::start(raw, version, handle, jump, info, on_lost);
        let listing = self
            .step(Step::Directory, async {
                let start = match host.sftp.remote_start.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    Some("~") => session.home().await?,
                    Some(start) => match start.strip_prefix("~/") {
                        Some(rest) => join(&session.home().await?, rest),
                        None => start.to_string(),
                    },
                    None => session.home().await?,
                };
                self.log(format!("debug1: Listing {start}"));
                session.list(&start).await
            })
            .await;
        match listing {
            Ok(listing) => Ok((session, listing)),
            Err(error) => {
                session.close().await;
                Err(error)
            }
        }
    }
}

/// Run the pipeline up to an SFTP session in the start directory.
/// The caller registers the session and emits `ConnectEvent::Connected`.
pub async fn connect_sftp(
    req: ConnectRequest,
    deps: ConnectDeps,
    broker: Arc<PromptBroker>,
    emit: EmitFn,
    on_lost: LostFn,
    cancel: CancellationToken,
) -> Result<(SftpSession, Listing)> {
    emit(ConnectEvent::Steps { steps: plan_sftp_steps(&req) });
    let pipeline = Pipeline::new(deps, broker, emit.clone(), cancel);
    let result = async {
        let est = pipeline.establish(&req).await?;
        pipeline.open_sftp(&req, est, on_lost).await
    }
    .await;
    if let Err(error) = &result {
        emit(ConnectEvent::Failed { error: error.clone() });
    }
    result
}

/// Run the full pipeline and return the interactive session.
/// The caller registers the session and emits `ConnectEvent::Connected`.
pub async fn connect_ssh(
    req: ConnectRequest,
    deps: ConnectDeps,
    broker: Arc<PromptBroker>,
    emit: Arc<dyn Fn(ConnectEvent) + Send + Sync>,
    sink: OutputSink,
    cancel: CancellationToken,
) -> Result<SshSession> {
    emit(ConnectEvent::Steps { steps: plan_steps(&req) });
    let pipeline = Pipeline::new(deps, broker, emit.clone(), cancel);
    let result = async {
        let est = pipeline.establish(&req).await?;
        pipeline.open_shell(&req, est, sink).await
    }
    .await;
    if let Err(error) = &result {
        emit(ConnectEvent::Failed { error: error.clone() });
    }
    result
}

/// "Verbindung testen": run until authentication succeeded, then disconnect.
pub async fn test_connection(
    req: ConnectRequest,
    deps: ConnectDeps,
    broker: Arc<PromptBroker>,
    emit: Arc<dyn Fn(ConnectEvent) + Send + Sync>,
    cancel: CancellationToken,
) -> Result<()> {
    let steps = plan_steps(&req).into_iter().filter(|s| !matches!(s, Step::Channel | Step::Shell)).collect();
    emit(ConnectEvent::Steps { steps });
    let pipeline = Pipeline::new(deps, broker, emit.clone(), cancel);
    match pipeline.establish(&req).await {
        Ok(est) => {
            disconnect(est).await;
            Ok(())
        }
        Err(error) => {
            emit(ConnectEvent::Failed { error: error.clone() });
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(with_jump: bool) -> ConnectRequest {
        let target = ConnectTarget { host: Host::new("web", "web.example.org"), profile: None };
        ConnectRequest {
            jump: with_jump.then(|| ConnectTarget { host: Host::new("bastion", "bastion.example.org"), profile: None }),
            target,
            cols: 80,
            rows: 24,
        }
    }

    fn default_key_order() -> Vec<Algorithm> {
        russh::Preferred::default().key.to_vec()
    }

    #[test]
    fn preferred_host_key_order_puts_known_types_first() {
        use russh::keys::{EcdsaCurve, HashAlg};
        assert_eq!(preferred_host_key_order(&[]), default_key_order());

        let p256 = Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 };
        let order = preferred_host_key_order(std::slice::from_ref(&p256));
        assert_eq!(order[0], p256);
        let mut rest = default_key_order();
        rest.retain(|a| *a != p256);
        assert_eq!(order[1..], rest[..], "the remaining types keep the default order");

        // A stored ssh-rsa key prefers all RSA signature algorithms, in default order.
        let order = preferred_host_key_order(&[Algorithm::Rsa { hash: None }]);
        assert_eq!(
            order[..3],
            [
                Algorithm::Rsa { hash: Some(HashAlg::Sha512) },
                Algorithm::Rsa { hash: Some(HashAlg::Sha256) },
                Algorithm::Rsa { hash: None }
            ]
        );
        assert_eq!(order.len(), default_key_order().len());
        assert_eq!(order[3], Algorithm::Ed25519);

        // Several known types: all of them first, in default order.
        let order = preferred_host_key_order(&[Algorithm::Rsa { hash: None }, Algorithm::Ed25519]);
        assert_eq!(order[0], Algorithm::Ed25519);
        assert!(order[1..4].iter().all(|a| matches!(a, Algorithm::Rsa { .. })));
    }

    #[test]
    fn sftp_steps_replace_channel_and_shell() {
        assert_eq!(
            plan_sftp_steps(&request(true)),
            vec![Step::JumpHost, Step::Tcp, Step::HostKey, Step::Auth, Step::Subsystem, Step::Directory]
        );
        assert_eq!(plan_sftp_steps(&request(false))[0], Step::Tcp);
    }

    #[test]
    fn plan_steps_without_jump() {
        assert_eq!(plan_steps(&request(false)), vec![Step::Tcp, Step::HostKey, Step::Auth, Step::Channel, Step::Shell]);
    }

    #[test]
    fn plan_steps_with_jump() {
        let steps = plan_steps(&request(true));
        assert_eq!(steps.first(), Some(&Step::JumpHost));
        assert_eq!(steps.len(), 6);
    }

    #[test]
    fn log_line_is_sanitized() {
        match log_line("password: hunter2") {
            ConnectEvent::Log { line } => assert_eq!(line, "password: ***"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn events_serialize_with_kind_tag() {
        let json = serde_json::to_value(ConnectEvent::StepStarted { step: Step::HostKey }).unwrap();
        assert_eq!(json, serde_json::json!({"kind": "stepStarted", "step": "hostKey"}));

        let json = serde_json::to_value(ConnectEvent::Prompt {
            prompt_id: 7,
            prompt: Prompt::Passphrase { key_path: "k".into() },
        })
        .unwrap();
        assert_eq!(json["kind"], "prompt");
        assert_eq!(json["promptId"], 7);
        assert_eq!(json["prompt"]["kind"], "passphrase");

        let json = serde_json::to_value(ConnectEvent::Failed { error: CoreError::Cancelled }).unwrap();
        assert_eq!(json["kind"], "failed");
        assert!(json["error"].is_object());
    }
}

#[cfg(test)]
mod pipeline_tests {
    use super::*;
    use crate::secrets::MemorySecretStore;
    use std::sync::Mutex;

    struct Recorder {
        events: Arc<Mutex<Vec<ConnectEvent>>>,
        emit: EmitFn,
    }

    fn recorder() -> Recorder {
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        let emit: EmitFn = Arc::new(move |event| e2.lock().unwrap().push(event));
        Recorder { events, emit }
    }

    fn deps(dir: &std::path::Path) -> ConnectDeps {
        ConnectDeps {
            known_hosts: KnownHosts::new(dir.join("known_hosts")),
            secrets: Arc::new(MemorySecretStore::new()),
            agent_sock: None,
        }
    }

    fn request(address: &str, port: u16) -> ConnectRequest {
        let mut host = Host::new("t", address);
        host.port = port;
        host.timeout_s = 5;
        ConnectRequest { target: ConnectTarget { host, profile: None }, jump: None, cols: 80, rows: 24 }
    }

    fn noop_sink() -> OutputSink {
        Arc::new(|_| {})
    }

    async fn run(req: ConnectRequest, cancel: CancellationToken) -> (Result<SshSession>, Vec<ConnectEvent>) {
        let dir = tempfile::tempdir().unwrap();
        let rec = recorder();
        let broker = Arc::new(PromptBroker::new(Arc::new(|_| {})));
        let result = connect_ssh(req, deps(dir.path()), broker, rec.emit.clone(), noop_sink(), cancel).await;
        let events = rec.events.lock().unwrap().clone();
        (result, events)
    }

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn refused_port_is_classified_and_reported() {
        let (result, events) = run(request("127.0.0.1", free_port()), CancellationToken::new()).await;
        assert!(matches!(result, Err(CoreError::Network { kind: NetworkKind::Refused, .. })));
        assert!(matches!(&events[0], ConnectEvent::Steps { steps } if steps[0] == Step::Tcp));
        assert!(matches!(events[1], ConnectEvent::StepStarted { step: Step::Tcp }));
        assert!(events.iter().any(|e| matches!(
            e,
            ConnectEvent::StepFailed { step: Step::Tcp, error: CoreError::Network { kind: NetworkKind::Refused, .. } }
        )));
        assert!(matches!(events.last(), Some(ConnectEvent::Failed { .. })));
    }

    #[tokio::test]
    async fn unresolvable_host_is_dns_error() {
        let (result, _) = run(request("nonexistent.invalid", 22), CancellationToken::new()).await;
        assert!(matches!(result, Err(CoreError::Network { kind: NetworkKind::Dns, .. })));
    }

    #[tokio::test]
    async fn cancelled_token_aborts_pipeline() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let (result, events) = run(request("127.0.0.1", free_port()), cancel).await;
        assert!(matches!(result, Err(CoreError::Cancelled)));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ConnectEvent::StepFailed { step: Step::Tcp, error: CoreError::Cancelled }))
        );
    }

    #[tokio::test]
    async fn test_connection_reports_failure() {
        let dir = tempfile::tempdir().unwrap();
        let rec = recorder();
        let broker = Arc::new(PromptBroker::new(Arc::new(|_| {})));
        let result = test_connection(
            request("127.0.0.1", free_port()),
            deps(dir.path()),
            broker,
            rec.emit.clone(),
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(CoreError::Network { kind: NetworkKind::Refused, .. })));
        let events = rec.events.lock().unwrap();
        assert!(matches!(
            &events[0],
            ConnectEvent::Steps { steps } if steps == &vec![Step::Tcp, Step::HostKey, Step::Auth]
        ));
    }

    fn request_with_timeout(address: &str, port: u16, timeout_s: u32) -> ConnectRequest {
        let mut req = request(address, port);
        req.target.host.timeout_s = timeout_s;
        req
    }

    /// A server that accepts the TCP connection but never sends the SSH
    /// identification banner must not hang the `HostKey` step forever: the
    /// handshake has to fail with a `Timeout` once `host.timeout_s` elapses.
    #[tokio::test]
    async fn handshake_without_banner_times_out() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                // Hold the connection open without ever writing a banner.
                let _ = stream;
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });
        let started = tokio::time::Instant::now();
        let (result, events) =
            run(request_with_timeout(&addr.ip().to_string(), addr.port(), 1), CancellationToken::new()).await;
        assert!(started.elapsed() < Duration::from_secs(5), "must fail close to the 1s timeout, not hang");
        assert!(matches!(result, Err(CoreError::Network { kind: NetworkKind::Timeout, .. })));
        assert!(events.iter().any(|e| matches!(
            e,
            ConnectEvent::StepFailed {
                step: Step::HostKey,
                error: CoreError::Network { kind: NetworkKind::Timeout, .. }
            }
        )));
        assert_eq!(events.iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count(), 1);
        assert!(matches!(events.last(), Some(ConnectEvent::Failed { .. })));
    }

    /// `timeout_s == 0` must keep meaning "no timeout" for the handshake
    /// deadline helper too (not just for `tcp_connect`).
    #[tokio::test]
    async fn setup_timeout_zero_means_no_timeout() {
        let disarm = Arc::new(Notify::new());
        let fut = with_setup_timeout(0, disarm, || "unused".to_string(), std::future::pending::<Result<()>>());
        let outcome = tokio::time::timeout(Duration::from_millis(50), fut).await;
        assert!(outcome.is_err(), "with timeout_s = 0 the deadline helper must never resolve on its own");
    }

    /// Once `disarm` has fired (host key received), the remaining wait must
    /// no longer be bounded by the deadline, even though it outlives it.
    /// This stands in for "time spent in the host-key prompt is excluded"
    /// without needing a real prompt or network server.
    #[tokio::test]
    async fn setup_timeout_is_disarmed_once_signalled() {
        let disarm = Arc::new(Notify::new());
        let d2 = disarm.clone();
        let fut = async move {
            d2.notify_one();
            // Outlives the 1s deadline; must still succeed because the
            // signal above disarmed it first.
            tokio::time::sleep(Duration::from_millis(1200)).await;
            Ok::<(), CoreError>(())
        };
        let result = with_setup_timeout(1, disarm, || "must not fire".to_string(), fut).await;
        assert!(result.is_ok(), "deadline must stay disarmed once the signal has fired");
    }
}
