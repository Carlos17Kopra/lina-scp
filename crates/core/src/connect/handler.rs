//! russh client handler: host key verification against known_hosts.

use std::future::Future;
use std::sync::{Arc, PoisonError};

use russh::client::{self, Msg};
use russh::keys::{PublicKey, PublicKeyOrCertificate};
use russh::{Channel, ChannelOpenFailure};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::InfoSlot;
use super::classify::map_russh_error;
use super::log_line;
use super::prompt::{EmitFn, HostKeyDecision, Prompt, PromptAnswer, PromptBroker};
use crate::error::{CoreError, Result};
use crate::known_hosts::{HostKeyInfo, HostKeyStatus, KnownHosts, fingerprints};
use crate::tunnel::{ForwardRoutes, serve_forwarded};

/// What to do with a server key after the user (or known_hosts) decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyAction {
    /// Continue without touching known_hosts (known key or "accept once").
    Accept,
    /// Continue and append the key (hashed) to known_hosts.
    Trust,
    /// Continue and replace the stored key of the same algorithm.
    Replace,
}

fn rejected(message: &str) -> CoreError {
    CoreError::HostKeyRejected { message: message.to_string() }
}

/// Pure decision logic. `ask` is only invoked for unknown or changed keys.
/// Fail-closed: anything but a valid, explicit acceptance is a rejection.
/// `other_key_types_known` only annotates the `Unknown` prompt (see `Prompt::HostKeyUnknown`).
pub async fn decide_host_key<F, Fut>(
    status: HostKeyStatus,
    host: &str,
    port: u16,
    current: HostKeyInfo,
    other_key_types_known: Vec<String>,
    ask: F,
) -> Result<HostKeyAction>
where
    F: FnOnce(Prompt) -> Fut,
    Fut: Future<Output = PromptAnswer>,
{
    match status {
        HostKeyStatus::Known => Ok(HostKeyAction::Accept),
        HostKeyStatus::Revoked => Err(rejected("host key is marked @revoked in known_hosts; connection refused")),
        HostKeyStatus::Unknown => {
            let prompt = Prompt::HostKeyUnknown { host: host.to_string(), port, info: current, other_key_types_known };
            let answer = ask(prompt).await;
            match answer {
                PromptAnswer::HostKey { decision: HostKeyDecision::AcceptOnce } => Ok(HostKeyAction::Accept),
                PromptAnswer::HostKey { decision: HostKeyDecision::Trust } => Ok(HostKeyAction::Trust),
                _ => Err(rejected("unknown host key was not accepted")),
            }
        }
        HostKeyStatus::Changed { stored } => {
            let answer = ask(Prompt::HostKeyChanged { host: host.to_string(), port, stored, current }).await;
            match answer {
                PromptAnswer::HostKey { decision: HostKeyDecision::Replace } => Ok(HostKeyAction::Replace),
                _ => Err(rejected("REMOTE HOST IDENTIFICATION HAS CHANGED; connection refused")),
            }
        }
    }
}

/// Error type of the russh handler: either a transport error or our own.
#[derive(Debug)]
pub enum HandlerError {
    Ssh(russh::Error),
    Core(CoreError),
}

impl From<russh::Error> for HandlerError {
    fn from(err: russh::Error) -> Self {
        HandlerError::Ssh(err)
    }
}

impl From<HandlerError> for CoreError {
    fn from(err: HandlerError) -> Self {
        match err {
            HandlerError::Ssh(e) => map_russh_error(e),
            HandlerError::Core(e) => e,
        }
    }
}

/// russh handler for one SSH hop (jump host or target).
pub struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<KnownHosts>,
    broker: Arc<PromptBroker>,
    emit: EmitFn,
    cancel: CancellationToken,
    /// Notified as soon as the server's host key reaches `check_server_key`,
    /// so the caller can stop counting its setup timeout against the
    /// (possibly slow) host-key prompt that follows.
    key_seen: Arc<Notify>,
    negotiated: InfoSlot,
    forwarded: Option<ForwardRoutes>,
}

impl ClientHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: String,
        port: u16,
        known_hosts: Arc<KnownHosts>,
        broker: Arc<PromptBroker>,
        emit: EmitFn,
        cancel: CancellationToken,
        key_seen: Arc<Notify>,
        negotiated: InfoSlot,
    ) -> Self {
        Self { host, port, known_hosts, broker, emit, cancel, key_seen, negotiated, forwarded: None }
    }

    /// Accept `forwarded-tcpip` channels for these routes (remote port forwards of this connection).
    pub fn with_forwarded(mut self, forwarded: Option<ForwardRoutes>) -> Self {
        self.forwarded = forwarded;
        self
    }

    async fn verify(&self, key: &PublicKey) -> Result<()> {
        let info = fingerprints(key);
        {
            let mut slot = self.negotiated.lock().unwrap_or_else(PoisonError::into_inner);
            slot.host_key_algorithm = info.algorithm.clone();
            slot.host_key_sha256 = info.sha256.clone();
        }
        (self.emit)(log_line(format!("debug1: Server host key: {} {}", info.algorithm, info.sha256)));
        let status = self.known_hosts.check(&self.host, self.port, key)?;
        let status_text = match &status {
            HostKeyStatus::Known => "known",
            HostKeyStatus::Unknown => "unknown",
            HostKeyStatus::Changed { .. } => "CHANGED",
            HostKeyStatus::Revoked => "REVOKED",
        };
        (self.emit)(log_line(format!("debug1: Host '{}' key is {status_text}", self.host)));
        let other_key_types = match status {
            HostKeyStatus::Unknown => self.known_hosts.other_key_types(&self.host, self.port, key)?,
            _ => Vec::new(),
        };
        if !other_key_types.is_empty() {
            (self.emit)(log_line(format!(
                "debug1: Host '{}' is known with other key types: {}",
                self.host,
                other_key_types.join(",")
            )));
        }
        let action = decide_host_key(status, &self.host, self.port, info, other_key_types, |prompt| {
            self.broker.ask_cancellable(&self.emit, prompt, &self.cancel)
        })
        .await?;
        match action {
            HostKeyAction::Accept => {}
            HostKeyAction::Trust => {
                self.known_hosts.add(&self.host, self.port, key, true)?;
                (self.emit)(log_line(format!(
                    "Warning: Permanently added '{}' to the list of known hosts.",
                    self.host
                )));
            }
            HostKeyAction::Replace => {
                self.known_hosts.replace(&self.host, self.port, key)?;
                (self.emit)(log_line(format!("debug1: Replaced host key for '{}' in known_hosts", self.host)));
            }
        }
        Ok(())
    }
}

impl client::Handler for ClientHandler {
    type Error = HandlerError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        // The server's key has now arrived; a caller-side setup timeout must
        // stop counting from here (the host-key prompt below can be slow).
        self.key_seen.notify_one();
        match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => {
                self.verify(key).await.map_err(HandlerError::Core)?;
                Ok(true)
            }
            PublicKeyOrCertificate::Certificate(_) => {
                Err(HandlerError::Core(rejected("host certificates are not supported")))
            }
        }
    }

    /// Only forwards this connection asked for are accepted (fail closed): the server must not make us connect
    /// anywhere else.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> std::result::Result<(), Self::Error> {
        match self.forwarded.as_ref().and_then(|routes| routes(connected_address, connected_port)) {
            Some((target, token)) => {
                reply.accept().await;
                tokio::spawn(serve_forwarded(Box::new(channel.into_stream()), target, token));
            }
            None => reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await,
        }
        Ok(())
    }

    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        _session: &mut client::Session,
    ) -> std::result::Result<(), Self::Error> {
        let mut slot = self.negotiated.lock().unwrap_or_else(PoisonError::into_inner);
        slot.kex = names.kex.as_ref().to_string();
        slot.cipher = names.cipher.as_ref().to_string();
        slot.mac = names.client_mac.as_ref().to_string();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(sha: &str) -> HostKeyInfo {
        HostKeyInfo { algorithm: "ssh-ed25519".into(), sha256: sha.into(), md5: "MD5:00".into() }
    }

    fn answer(decision: HostKeyDecision) -> PromptAnswer {
        PromptAnswer::HostKey { decision }
    }

    async fn decide(status: HostKeyStatus, reply: PromptAnswer) -> (Result<HostKeyAction>, Option<Prompt>) {
        let mut asked = None;
        let others = vec!["ecdsa-sha2-nistp256".to_string()];
        let result = decide_host_key(status, "example.org", 2222, info("SHA256:new"), others, |p| {
            asked = Some(p);
            async move { reply }
        })
        .await;
        (result, asked)
    }

    #[tokio::test]
    async fn known_key_is_accepted_without_prompt() {
        let result = decide_host_key(HostKeyStatus::Known, "h", 22, info("SHA256:a"), Vec::new(), |_| async {
            panic!("must not ask for known keys")
        })
        .await;
        assert_eq!(result.unwrap(), HostKeyAction::Accept);
    }

    #[tokio::test]
    async fn revoked_key_is_rejected_without_prompt() {
        let result = decide_host_key(HostKeyStatus::Revoked, "h", 22, info("SHA256:a"), Vec::new(), |_| async {
            panic!("must not ask for revoked keys")
        })
        .await;
        assert!(matches!(result, Err(CoreError::HostKeyRejected { .. })));
    }

    #[tokio::test]
    async fn unknown_key_trust_and_accept_once() {
        let (r, asked) = decide(HostKeyStatus::Unknown, answer(HostKeyDecision::Trust)).await;
        assert_eq!(r.unwrap(), HostKeyAction::Trust);
        assert_eq!(
            asked,
            Some(Prompt::HostKeyUnknown {
                host: "example.org".into(),
                port: 2222,
                info: info("SHA256:new"),
                other_key_types_known: vec!["ecdsa-sha2-nistp256".into()],
            })
        );
        let (r, _) = decide(HostKeyStatus::Unknown, answer(HostKeyDecision::AcceptOnce)).await;
        assert_eq!(r.unwrap(), HostKeyAction::Accept);
    }

    #[tokio::test]
    async fn unknown_key_rejected_on_reject_cancel_or_replace() {
        for reply in [
            answer(HostKeyDecision::Reject),
            answer(HostKeyDecision::Replace),
            PromptAnswer::Cancel,
            PromptAnswer::Secret { value: "x".into(), save: false },
        ] {
            let (r, _) = decide(HostKeyStatus::Unknown, reply).await;
            assert!(matches!(r, Err(CoreError::HostKeyRejected { .. })));
        }
    }

    #[tokio::test]
    async fn changed_key_only_replace_is_accepted() {
        let changed = || HostKeyStatus::Changed { stored: info("SHA256:old") };
        let (r, asked) = decide(changed(), answer(HostKeyDecision::Replace)).await;
        assert_eq!(r.unwrap(), HostKeyAction::Replace);
        assert_eq!(
            asked,
            Some(Prompt::HostKeyChanged {
                host: "example.org".into(),
                port: 2222,
                stored: info("SHA256:old"),
                current: info("SHA256:new")
            })
        );
        for reply in [
            answer(HostKeyDecision::Trust),
            answer(HostKeyDecision::AcceptOnce),
            answer(HostKeyDecision::Reject),
            PromptAnswer::Cancel,
        ] {
            let (r, _) = decide(changed(), reply).await;
            assert!(matches!(r, Err(CoreError::HostKeyRejected { .. })));
        }
    }

    #[test]
    fn handler_error_converts_to_core_error() {
        let e: CoreError = HandlerError::Ssh(russh::Error::UnknownKey).into();
        assert!(matches!(e, CoreError::HostKeyRejected { .. }));
        let e: CoreError = HandlerError::Core(CoreError::Cancelled).into();
        assert!(matches!(e, CoreError::Cancelled));
    }
}
