//! Port forwards (TP5a) over the connection of a terminal session: local (`-L`), remote (`-R`) and dynamic SOCKS5
//! (`-D`). `Tunnels` outlives the session's reconnects; each connection is handed in as a [`TunnelLink`].

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::connect::classify::{classify_io, network_error};
use crate::error::{CoreError, NetworkKind, Result};
use crate::model::{ForwardKind, Id, PortForward};
use crate::validate::{Endpoint, ForwardSpec, parse_forward};

/// A byte stream carried by the SSH connection (a `direct-tcpip` or `forwarded-tcpip` channel).
pub trait TunnelIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> TunnelIo for T {}
pub type TunnelStream = Box<dyn TunnelIo>;

/// What a tunnel needs from one SSH connection.
#[async_trait::async_trait]
pub trait TunnelLink: Send + Sync {
    /// Opens a `direct-tcpip` channel to `host:port` on behalf of `origin`.
    async fn open_direct(&self, host: &str, port: u16, origin: SocketAddr) -> Result<TunnelStream>;
    /// Asks the server to listen (`tcpip-forward`); returns the bound port (relevant for port 0).
    async fn request_remote(&self, host: &str, port: u16) -> Result<u16>;
    /// Stops a remote listener (`cancel-tcpip-forward`).
    async fn cancel_remote(&self, host: &str, port: u16) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TunnelState {
    Off,
    Starting,
    /// `port` is the listening port (for a remote forward with port 0: the one the server picked).
    Active {
        port: u16,
    },
    Failed {
        error: CoreError,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelEvent {
    pub rule_id: Id,
    pub state: TunnelState,
}

pub type TunnelEmit = Arc<dyn Fn(TunnelEvent) + Send + Sync>;

/// Where an incoming `forwarded-tcpip` channel goes: its target, and the token that ends it with its rule.
/// `None` = this connection never asked for that address/port; the channel must be rejected.
pub type ForwardRoutes = Arc<dyn Fn(&str, u32) -> Option<(Endpoint, CancellationToken)> + Send + Sync>;

/// The port forwards of one terminal session.
pub struct Tunnels {
    inner: Arc<Inner>,
}

struct Inner {
    rules: Vec<PortForward>,
    emit: TunnelEmit,
    link: Mutex<Option<Link>>,
    /// Autostart ran (on the first connection after the start or an `end`).
    started: AtomicBool,
    /// Numbers the connections; a connection's routes only see forwards granted to it.
    epochs: AtomicU64,
    /// Rules the user wants on, including ones that failed again after a reconnect.
    running: Mutex<HashMap<Id, Running>>,
}

#[derive(Clone)]
struct Link {
    epoch: u64,
    link: Arc<dyn TunnelLink>,
    /// Ends every tunnelled connection that runs over this connection.
    token: CancellationToken,
}

struct Running {
    spec: ForwardSpec,
    /// Ends the listener and every connection of the rule.
    token: CancellationToken,
    /// Connection and port the forward was granted on; `None` while not (yet) granted.
    bound: Option<(u64, u16)>,
    /// A remote forward is being requested right now (a second start must not request it again).
    requesting: bool,
}

/// A handshake that does not finish in time is dropped (it would otherwise hold a task until the rule stops).
const SOCKS_HANDSHAKE: Duration = Duration::from_secs(10);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_connected() -> CoreError {
    network_error(NetworkKind::Other, "nicht verbunden")
}

impl Tunnels {
    pub fn new(rules: Vec<PortForward>, emit: TunnelEmit) -> Self {
        let inner = Inner {
            rules,
            emit,
            link: Mutex::new(None),
            started: AtomicBool::new(false),
            epochs: AtomicU64::new(0),
            running: Mutex::new(HashMap::new()),
        };
        Self { inner: Arc::new(inner) }
    }

    /// For the handler of a new connection: its number (pass it to `attach`) and its routes for incoming
    /// `forwarded-tcpip` channels. They only route forwards granted on that same connection.
    pub fn routes(&self) -> (u64, ForwardRoutes) {
        let epoch = self.inner.epochs.fetch_add(1, Ordering::SeqCst) + 1;
        let inner = Arc::downgrade(&self.inner);
        let routes: ForwardRoutes = Arc::new(move |host: &str, port: u32| {
            let inner = inner.upgrade()?;
            let running = lock(&inner.running);
            running
                .values()
                .find(|run| {
                    run.spec.kind == ForwardKind::Remote
                        && run.spec.bind.host == host
                        && run.bound.is_some_and(|(e, bound)| e == epoch && u32::from(bound) == port)
                })
                .and_then(|run| Some((run.spec.target.clone()?, run.token.clone())))
        });
        (epoch, routes)
    }

    /// Connection `epoch` is up: remote forwards are requested on it again; after the start (or an `end`),
    /// autostart rules start.
    pub async fn attach(&self, epoch: u64, link: Arc<dyn TunnelLink>) {
        let link = Link { epoch, link, token: CancellationToken::new() };
        if let Some(old) = lock(&self.inner.link).replace(link.clone()) {
            old.token.cancel();
        }
        let remotes: Vec<(Id, Endpoint, CancellationToken)> = lock(&self.inner.running)
            .iter_mut()
            .filter(|(_, run)| run.spec.kind == ForwardKind::Remote)
            .map(|(id, run)| {
                run.bound = None;
                run.requesting = true;
                (*id, run.spec.bind.clone(), run.token.clone())
            })
            .collect();
        for (id, bind, token) in remotes {
            let granted = link.link.request_remote(&bind.host, bind.port).await;
            let current = {
                let mut running = lock(&self.inner.running);
                match running.get_mut(&id) {
                    Some(run) if run.token == token => {
                        run.requesting = false;
                        if let Ok(port) = &granted {
                            run.bound = Some((epoch, *port));
                        }
                        true
                    }
                    _ => false,
                }
            };
            match granted {
                Ok(port) if current => self.inner.send(id, TunnelState::Active { port }),
                // Stopped or restarted meanwhile: the grant must not stay open on the server.
                Ok(port) => {
                    let _ = link.link.cancel_remote(&bind.host, port).await;
                }
                Err(error) if current => self.inner.send(id, TunnelState::Failed { error }),
                Err(_) => {}
            }
        }
        if !self.inner.started.swap(true, Ordering::SeqCst) {
            let autostart: Vec<Id> = self.inner.rules.iter().filter(|r| r.autostart).map(|r| r.id).collect();
            for id in autostart {
                let _ = self.start(id).await;
            }
        }
    }

    /// The connection is gone (reconnecting): its tunnelled connections end, new ones are closed until the next
    /// `attach`, and nothing is routed. Listeners stay.
    pub fn detach(&self) {
        if let Some(old) = lock(&self.inner.link).take() {
            old.token.cancel();
        }
        for run in lock(&self.inner.running).values_mut() {
            run.bound = None;
        }
    }

    /// The session gave up or ended: every rule stops. A later connection (manual reconnect) autostarts again.
    pub fn end(&self) {
        self.detach();
        let stopped: Vec<(Id, Running)> = lock(&self.inner.running).drain().collect();
        for (id, run) in stopped {
            run.token.cancel();
            self.inner.send(id, TunnelState::Off);
        }
        self.inner.started.store(false, Ordering::SeqCst);
    }

    pub async fn start(&self, id: Id) -> Result<()> {
        let inner = &self.inner;
        let rule = inner.rules.iter().find(|r| r.id == id).ok_or_else(|| CoreError::internal("unknown tunnel rule"))?;
        let spec =
            parse_forward(rule).inspect_err(|error| inner.send(id, TunnelState::Failed { error: error.clone() }))?;
        let link = lock(&inner.link).clone().ok_or_else(not_connected)?;
        let token = CancellationToken::new();
        {
            let mut running = lock(&inner.running);
            match running.get(&id) {
                Some(run) if run.requesting => return Ok(()),
                // A remote forward without a grant (failed after a reconnect) is requested again.
                Some(run) if run.spec.kind == ForwardKind::Remote && run.bound.is_none() => run.token.cancel(),
                Some(_) => return Ok(()),
                None => {}
            }
            running.insert(id, Running { spec: spec.clone(), token: token.clone(), bound: None, requesting: spec.kind == ForwardKind::Remote });
            // Under the lock: a concurrent `stop` sends its `Off` after this.
            inner.send(id, TunnelState::Starting);
        }
        let started = match spec.kind {
            ForwardKind::Remote => link.link.request_remote(&spec.bind.host, spec.bind.port).await,
            ForwardKind::Local | ForwardKind::Dynamic => {
                match TcpListener::bind((spec.bind.host.as_str(), spec.bind.port)).await {
                    Ok(listener) => {
                        let port = listener.local_addr().map(|a| a.port()).unwrap_or(spec.bind.port);
                        tokio::spawn(serve(listener, Arc::downgrade(inner), spec.clone(), token.clone()));
                        Ok(port)
                    }
                    Err(e) => {
                        Err(network_error(classify_io(&e), format!("{}:{}: {e}", spec.bind.host, spec.bind.port)))
                    }
                }
            }
        };
        match started {
            Ok(port) => {
                let current = match lock(&inner.running).get_mut(&id) {
                    Some(run) if run.token == token => {
                        run.requesting = false;
                        run.bound = Some((link.epoch, port));
                        inner.send(id, TunnelState::Active { port });
                        true
                    }
                    _ => false,
                };
                if !current && spec.kind == ForwardKind::Remote {
                    // Stopped meanwhile: the grant must not stay open on the server.
                    let _ = link.link.cancel_remote(&spec.bind.host, port).await;
                }
                Ok(())
            }
            Err(error) => {
                token.cancel();
                let mut running = lock(&inner.running);
                if running.get(&id).is_some_and(|run| run.token == token) {
                    running.remove(&id);
                    inner.send(id, TunnelState::Failed { error: error.clone() });
                }
                Err(error)
            }
        }
    }

    pub async fn stop(&self, id: Id) {
        let Some(run) = lock(&self.inner.running).remove(&id) else {
            return;
        };
        run.token.cancel();
        let link = lock(&self.inner.link).clone();
        if let (ForwardKind::Remote, Some((epoch, port)), Some(link)) = (run.spec.kind, run.bound, link)
            && link.epoch == epoch
        {
            let _ = link.link.cancel_remote(&run.spec.bind.host, port).await;
        }
        self.inner.send(id, TunnelState::Off);
    }
}

impl Drop for Tunnels {
    fn drop(&mut self) {
        if let Some(link) = lock(&self.inner.link).take() {
            link.token.cancel();
        }
        for run in lock(&self.inner.running).values() {
            run.token.cancel();
        }
    }
}

impl Inner {
    fn send(&self, rule_id: Id, state: TunnelState) {
        (self.emit)(TunnelEvent { rule_id, state });
    }
}

/// Accept loop of a local or dynamic forward.
async fn serve(listener: TcpListener, inner: Weak<Inner>, spec: ForwardSpec, token: CancellationToken) {
    loop {
        let accepted = tokio::select! {
            () = token.cancelled() => return,
            accepted = listener.accept() => accepted,
        };
        let (socket, origin) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                // E.g. out of file descriptors: do not spin.
                tracing::debug!(%error, "tunnel accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Some(inner) = inner.upgrade() else { return };
        let link = lock(&inner.link).clone();
        drop(inner);
        // Without a connection (reconnecting) the socket is simply closed.
        let Some(Link { link, token: link_token, .. }) = link else { continue };
        let spec = spec.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let mut socket = socket;
            let opened = match &spec.target {
                Some(target) => link.open_direct(&target.host, target.port, origin).await.ok(),
                None => tokio::time::timeout(SOCKS_HANDSHAKE, socks(&mut socket, &*link, origin)).await.ok().flatten(),
            };
            if let Some(channel) = opened {
                pipe(socket, channel, &token, &link_token).await;
            }
        });
    }
}

const SOCKS_OK: u8 = 0;
const SOCKS_FAILURE: u8 = 1;
const SOCKS_REFUSED: u8 = 5;
const SOCKS_BAD_COMMAND: u8 = 7;
const SOCKS_BAD_ADDRESS: u8 = 8;

async fn socks_reply(socket: &mut TcpStream, code: u8) -> Option<()> {
    socket.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await.ok()
}

/// SOCKS5 without authentication, `CONNECT` only (RFC 1928). Replies success only once the channel is open;
/// `None` = refused (an error reply was sent where the protocol allows one).
async fn socks(socket: &mut TcpStream, link: &dyn TunnelLink, origin: SocketAddr) -> Option<TunnelStream> {
    let mut head = [0u8; 2];
    socket.read_exact(&mut head).await.ok()?;
    if head[0] != 5 {
        return None;
    }
    let mut methods = vec![0u8; usize::from(head[1])];
    socket.read_exact(&mut methods).await.ok()?;
    if !methods.contains(&0) {
        let _ = socket.write_all(&[5, 0xFF]).await;
        return None;
    }
    socket.write_all(&[5, 0]).await.ok()?;
    let mut request = [0u8; 4];
    socket.read_exact(&mut request).await.ok()?;
    if request[0] != 5 {
        return None;
    }
    let host = match request[3] {
        1 => {
            let mut ip = [0u8; 4];
            socket.read_exact(&mut ip).await.ok()?;
            std::net::Ipv4Addr::from(ip).to_string()
        }
        4 => {
            let mut ip = [0u8; 16];
            socket.read_exact(&mut ip).await.ok()?;
            std::net::Ipv6Addr::from(ip).to_string()
        }
        3 => {
            let len = socket.read_u8().await.ok()?;
            let mut name = vec![0u8; usize::from(len)];
            socket.read_exact(&mut name).await.ok()?;
            match String::from_utf8(name) {
                Ok(name) if !name.is_empty() => name,
                _ => {
                    socks_reply(socket, SOCKS_BAD_ADDRESS).await;
                    return None;
                }
            }
        }
        _ => {
            socks_reply(socket, SOCKS_BAD_ADDRESS).await;
            return None;
        }
    };
    let port = socket.read_u16().await.ok()?;
    if request[1] != 1 {
        socks_reply(socket, SOCKS_BAD_COMMAND).await;
        return None;
    }
    match link.open_direct(&host, port, origin).await {
        Ok(channel) => {
            socks_reply(socket, SOCKS_OK).await?;
            Some(channel)
        }
        Err(error) => {
            let code = match error {
                CoreError::Network { kind: NetworkKind::Refused, .. } => SOCKS_REFUSED,
                _ => SOCKS_FAILURE,
            };
            socks_reply(socket, code).await;
            None
        }
    }
}

/// Copies both ways until one side ends, the rule stops or the connection goes.
async fn pipe(mut a: impl TunnelIo, mut b: impl TunnelIo, token: &CancellationToken, link: &CancellationToken) {
    tokio::select! {
        () = token.cancelled() => {}
        () = link.cancelled() => {}
        _ = tokio::io::copy_bidirectional(&mut a, &mut b) => {}
    }
}

/// An incoming `forwarded-tcpip` channel: connect to the rule's target and copy.
pub async fn serve_forwarded(channel: TunnelStream, target: Endpoint, token: CancellationToken) {
    match TcpStream::connect((target.host.as_str(), target.port)).await {
        // The channel itself ends with its connection.
        Ok(socket) => pipe(socket, channel, &token, &CancellationToken::new()).await,
        Err(error) => tracing::debug!(%error, "remote forward target unreachable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    /// Test link: "the server" is this machine, channels are plain TCP connections.
    #[derive(Default)]
    struct DirectLink {
        requested: Mutex<Vec<(String, u16)>>,
        cancelled: Mutex<Vec<(String, u16)>>,
        refuse_remote: AtomicBool,
        /// Holds `request_remote` until notified.
        gate: Option<Arc<tokio::sync::Notify>>,
        opened: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl TunnelLink for DirectLink {
        async fn open_direct(&self, host: &str, port: u16, _origin: SocketAddr) -> Result<TunnelStream> {
            self.opened.fetch_add(1, Ordering::SeqCst);
            let stream =
                TcpStream::connect((host, port)).await.map_err(|e| network_error(classify_io(&e), e.to_string()))?;
            Ok(Box::new(stream))
        }
        async fn request_remote(&self, host: &str, port: u16) -> Result<u16> {
            if let Some(gate) = &self.gate {
                self.requested.lock().unwrap().push((host.into(), port));
                gate.notified().await;
                return Ok(if port == 0 { 41234 } else { port });
            }
            if self.refuse_remote.load(Ordering::SeqCst) {
                return Err(network_error(NetworkKind::Other, "tcpip-forward refused"));
            }
            self.requested.lock().unwrap().push((host.into(), port));
            Ok(if port == 0 { 41234 } else { port })
        }
        async fn cancel_remote(&self, host: &str, port: u16) -> Result<()> {
            self.cancelled.lock().unwrap().push((host.into(), port));
            Ok(())
        }
    }

    /// Echo server; returns its port.
    async fn echo() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });
        port
    }

    /// A new connection: its routes, then the link.
    async fn connect(tunnels: &Tunnels, link: Arc<DirectLink>) -> ForwardRoutes {
        let (epoch, routes) = tunnels.routes();
        tunnels.attach(epoch, link).await;
        routes
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    fn rule(kind: ForwardKind, bind: String, target: String, autostart: bool) -> PortForward {
        PortForward { id: uuid::Uuid::new_v4(), kind, bind, target, autostart }
    }

    fn recorder() -> (TunnelEmit, Arc<Mutex<Vec<TunnelEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        (Arc::new(move |e| sink.lock().unwrap().push(e)), events)
    }

    fn last_state(events: &Mutex<Vec<TunnelEvent>>, id: Id) -> Option<TunnelState> {
        events.lock().unwrap().iter().rev().find(|e| e.rule_id == id).map(|e| e.state.clone())
    }

    async fn roundtrip(stream: &mut TcpStream, data: &[u8]) -> Vec<u8> {
        stream.write_all(data).await.unwrap();
        let mut buf = vec![0; data.len()];
        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut buf)).await.unwrap().unwrap();
        buf
    }

    async fn closed(stream: &mut TcpStream) -> bool {
        let mut buf = [0u8; 16];
        matches!(tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await, Ok(Ok(0) | Err(_)))
    }

    #[tokio::test]
    async fn a_local_forward_carries_data_both_ways_and_stops() {
        let target = echo().await;
        let port = free_port();
        let r = rule(ForwardKind::Local, port.to_string(), format!("127.0.0.1:{target}"), false);
        let (emit, events) = recorder();
        let tunnels = Tunnels::new(vec![r.clone()], emit);
        connect(&tunnels, Arc::new(DirectLink::default())).await;
        assert_eq!(last_state(&events, r.id), None, "no autostart");
        tunnels.start(r.id).await.unwrap();
        assert_eq!(last_state(&events, r.id), Some(TunnelState::Active { port }));
        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        assert_eq!(roundtrip(&mut client, b"hello").await, b"hello");
        tunnels.stop(r.id).await;
        assert_eq!(last_state(&events, r.id), Some(TunnelState::Off));
        assert!(closed(&mut client).await, "running connections end with the rule");
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err(), "listener closed");
    }

    #[tokio::test]
    async fn autostart_rules_start_on_the_first_connection_only() {
        let target = echo().await;
        let r = rule(ForwardKind::Local, free_port().to_string(), format!("127.0.0.1:{target}"), true);
        let (emit, events) = recorder();
        let tunnels = Tunnels::new(vec![r.clone()], emit);
        connect(&tunnels, Arc::new(DirectLink::default())).await;
        assert!(matches!(last_state(&events, r.id), Some(TunnelState::Active { .. })));
        tunnels.stop(r.id).await;
        connect(&tunnels, Arc::new(DirectLink::default())).await;
        assert_eq!(last_state(&events, r.id), Some(TunnelState::Off), "a reconnect does not restart a stopped rule");
    }

    #[tokio::test]
    async fn a_busy_port_or_a_broken_rule_fails_and_the_rule_stays_startable() {
        let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = busy.local_addr().unwrap().port();
        let r = rule(ForwardKind::Local, port.to_string(), "127.0.0.1:1".into(), false);
        let broken = rule(ForwardKind::Local, "x".into(), "y".into(), false);
        let (emit, events) = recorder();
        let tunnels = Tunnels::new(vec![r.clone(), broken.clone()], emit);
        connect(&tunnels, Arc::new(DirectLink::default())).await;
        assert!(tunnels.start(r.id).await.is_err());
        assert!(matches!(last_state(&events, r.id), Some(TunnelState::Failed { .. })));
        assert!(matches!(tunnels.start(broken.id).await, Err(CoreError::Config { .. })));
        drop(busy);
        tunnels.start(r.id).await.unwrap();
        assert!(tunnels.start(uuid::Uuid::new_v4()).await.is_err(), "unknown rule");
    }

    #[tokio::test]
    async fn without_a_connection_start_fails_and_an_open_connection_is_closed() {
        let r = rule(ForwardKind::Local, free_port().to_string(), "127.0.0.1:1".into(), false);
        let (emit, _) = recorder();
        let tunnels = Tunnels::new(vec![r.clone()], emit);
        assert!(tunnels.start(r.id).await.is_err());
        let link = Arc::new(DirectLink::default());
        connect(&tunnels, link.clone()).await;
        tunnels.start(r.id).await.unwrap();
        tunnels.detach();
        assert!(tunnels.start(uuid::Uuid::new_v4()).await.is_err());
        let port: u16 = r.bind.parse().unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        assert!(closed(&mut client).await);
        assert_eq!(link.opened.load(Ordering::SeqCst), 0);
    }

    async fn socks_connect(port: u16, request: &[u8]) -> (TcpStream, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(&[5, 1, 0]).await.unwrap();
        let mut hello = [0u8; 2];
        s.read_exact(&mut hello).await.unwrap();
        assert_eq!(hello, [5, 0]);
        s.write_all(request).await.unwrap();
        let mut reply = vec![0u8; 10];
        s.read_exact(&mut reply).await.unwrap();
        (s, reply)
    }

    #[tokio::test]
    async fn a_dynamic_forward_speaks_socks5_connect_only() {
        let target = echo().await;
        let port = free_port();
        let r = rule(ForwardKind::Dynamic, port.to_string(), String::new(), false);
        let (emit, _) = recorder();
        let tunnels = Tunnels::new(vec![r.clone()], emit);
        connect(&tunnels, Arc::new(DirectLink::default())).await;
        tunnels.start(r.id).await.unwrap();

        let [hi, lo] = target.to_be_bytes();
        let mut domain = vec![5, 1, 0, 3, 9];
        domain.extend_from_slice(b"127.0.0.1");
        domain.extend_from_slice(&[hi, lo]);
        let (mut s, reply) = socks_connect(port, &domain).await;
        assert_eq!(reply[..2], [5, 0]);
        assert_eq!(roundtrip(&mut s, b"via socks").await, b"via socks");

        let (mut s, reply) = socks_connect(port, &[5, 1, 0, 1, 127, 0, 0, 1, hi, lo]).await;
        assert_eq!(reply[..2], [5, 0]);
        assert_eq!(roundtrip(&mut s, b"ipv4").await, b"ipv4");

        let (mut s, reply) = socks_connect(port, &[5, 2, 0, 1, 127, 0, 0, 1, hi, lo]).await;
        assert_eq!(reply[..2], [5, 7], "BIND is not supported");
        assert!(closed(&mut s).await);

        let (_, reply) = socks_connect(port, &[5, 1, 0, 1, 127, 0, 0, 1, 0, 1]).await;
        assert_ne!(reply[1], 0, "an unreachable target is an error reply");

        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(&[5, 1, 2]).await.unwrap();
        let mut hello = [0u8; 2];
        s.read_exact(&mut hello).await.unwrap();
        assert_eq!(hello, [5, 0xFF], "only no-auth is offered");
        assert!(closed(&mut s).await);
    }

    #[tokio::test]
    async fn a_remote_forward_routes_only_what_it_requested_and_comes_back_after_a_reconnect() {
        let r = rule(ForwardKind::Remote, "0".into(), "127.0.0.1:5432".into(), true);
        let (emit, events) = recorder();
        let tunnels = Tunnels::new(vec![r.clone()], emit);
        let (_, unused) = tunnels.routes();
        assert!(unused("localhost", 41234).is_none(), "nothing requested yet");
        let first = Arc::new(DirectLink::default());
        let routes = connect(&tunnels, first.clone()).await;
        assert_eq!(last_state(&events, r.id), Some(TunnelState::Active { port: 41234 }));
        assert_eq!(*first.requested.lock().unwrap(), vec![("localhost".to_string(), 0)]);
        let (target, _) = routes("localhost", 41234).expect("requested forward is routed");
        assert_eq!(target, Endpoint { host: "127.0.0.1".into(), port: 5432 });
        assert!(routes("localhost", 22).is_none());
        assert!(routes("0.0.0.0", 41234).is_none());

        let second = Arc::new(DirectLink::default());
        let routes = connect(&tunnels, second.clone()).await;
        assert_eq!(second.requested.lock().unwrap().len(), 1, "requested again on the new connection");
        assert!(routes("localhost", 41234).is_some());
        assert!(unused("localhost", 41234).is_none(), "only the connection that asked gets the channel");

        tunnels.detach();
        assert!(routes("localhost", 41234).is_none(), "nothing is routed while reconnecting");
        let refusing = Arc::new(DirectLink::default());
        refusing.refuse_remote.store(true, Ordering::SeqCst);
        let routes = connect(&tunnels, refusing.clone()).await;
        assert!(matches!(last_state(&events, r.id), Some(TunnelState::Failed { .. })));
        assert!(routes("localhost", 41234).is_none(), "a failed rule routes nothing");

        refusing.refuse_remote.store(false, Ordering::SeqCst);
        tunnels.start(r.id).await.unwrap();
        assert_eq!(last_state(&events, r.id), Some(TunnelState::Active { port: 41234 }), "a failed rule starts again");
        tunnels.stop(r.id).await;
        assert_eq!(*refusing.cancelled.lock().unwrap(), vec![("localhost".to_string(), 41234)]);
        assert!(routes("localhost", 41234).is_none());
    }

    #[tokio::test]
    async fn end_stops_every_rule_and_a_later_connection_autostarts_again() {
        let target = echo().await;
        let port = free_port();
        let r = rule(ForwardKind::Local, port.to_string(), format!("127.0.0.1:{target}"), true);
        let (emit, events) = recorder();
        let tunnels = Tunnels::new(vec![r.clone()], emit);
        connect(&tunnels, Arc::new(DirectLink::default())).await;
        tunnels.end();
        assert_eq!(last_state(&events, r.id), Some(TunnelState::Off));
        assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
        assert!(tunnels.start(r.id).await.is_err(), "no connection after the end");
        connect(&tunnels, Arc::new(DirectLink::default())).await;
        assert!(matches!(last_state(&events, r.id), Some(TunnelState::Active { .. })));
    }

    #[tokio::test]
    async fn losing_the_connection_cuts_open_connections_but_keeps_the_listener() {
        let target = echo().await;
        let port = free_port();
        let r = rule(ForwardKind::Local, port.to_string(), format!("127.0.0.1:{target}"), true);
        let (emit, _) = recorder();
        let tunnels = Tunnels::new(vec![r.clone()], emit);
        connect(&tunnels, Arc::new(DirectLink::default())).await;
        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        assert_eq!(roundtrip(&mut client, b"a").await, b"a");
        tunnels.detach();
        assert!(closed(&mut client).await, "an idle connection does not hang on a dead link");
        connect(&tunnels, Arc::new(DirectLink::default())).await;
        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        assert_eq!(roundtrip(&mut client, b"b").await, b"b");
    }

    async fn requested(link: &DirectLink, n: usize) {
        for _ in 0..500 {
            if link.requested.lock().unwrap().len() >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        panic!("request_remote was not called");
    }

    #[tokio::test]
    async fn a_second_start_while_a_remote_forward_is_requested_sends_no_second_request() {
        let r = rule(ForwardKind::Remote, "0".into(), "127.0.0.1:5432".into(), false);
        let (emit, events) = recorder();
        let tunnels = Arc::new(Tunnels::new(vec![r.clone()], emit));
        let gate = Arc::new(tokio::sync::Notify::new());
        let link = Arc::new(DirectLink { gate: Some(gate.clone()), ..DirectLink::default() });
        connect(&tunnels, link.clone()).await;

        let first = tokio::spawn({
            let tunnels = tunnels.clone();
            async move { tunnels.start(r.id).await }
        });
        requested(&link, 1).await;
        let second = tokio::time::timeout(Duration::from_secs(1), tunnels.start(r.id)).await;
        assert!(matches!(second, Ok(Ok(()))), "the second start returns at once");
        gate.notify_one();
        first.await.unwrap().unwrap();
        assert_eq!(link.requested.lock().unwrap().len(), 1);
        assert_eq!(last_state(&events, r.id), Some(TunnelState::Active { port: 41234 }));
    }

    #[tokio::test]
    async fn a_remote_forward_stopped_while_it_is_requested_is_cancelled_on_the_server() {
        let r = rule(ForwardKind::Remote, "0".into(), "127.0.0.1:5432".into(), false);
        let (emit, events) = recorder();
        let tunnels = Arc::new(Tunnels::new(vec![r.clone()], emit));
        let gate = Arc::new(tokio::sync::Notify::new());
        let link = Arc::new(DirectLink { gate: Some(gate.clone()), ..DirectLink::default() });
        let routes = connect(&tunnels, link.clone()).await;

        let starting = tokio::spawn({
            let tunnels = tunnels.clone();
            async move { tunnels.start(r.id).await }
        });
        requested(&link, 1).await;
        tunnels.stop(r.id).await;
        gate.notify_one();
        starting.await.unwrap().unwrap();
        assert_eq!(*link.cancelled.lock().unwrap(), vec![("localhost".to_string(), 41234)]);
        assert_eq!(last_state(&events, r.id), Some(TunnelState::Off));
        assert!(routes("localhost", 41234).is_none());

        // The same during the re-request after a reconnect.
        let plain = Arc::new(DirectLink::default());
        connect(&tunnels, plain).await;
        tunnels.start(r.id).await.unwrap();
        let (epoch, _) = tunnels.routes();
        let gated = Arc::new(DirectLink { gate: Some(gate.clone()), ..DirectLink::default() });
        let attaching = tokio::spawn({
            let tunnels = tunnels.clone();
            let gated = gated.clone();
            async move { tunnels.attach(epoch, gated).await }
        });
        requested(&gated, 1).await;
        tunnels.stop(r.id).await;
        gate.notify_one();
        attaching.await.unwrap();
        assert_eq!(*gated.cancelled.lock().unwrap(), vec![("localhost".to_string(), 41234)]);
        assert_eq!(last_state(&events, r.id), Some(TunnelState::Off));
    }
}
