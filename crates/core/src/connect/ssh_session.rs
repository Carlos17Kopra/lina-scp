//! Interactive SSH shell session (implements `Session`).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use russh::client::{self, Msg};
use russh::{Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf, Disconnect};
use tokio::task::AbortHandle;

use super::SessionInfo;
use super::classify::{map_russh_error, network_error};
use super::handler::ClientHandler;
use crate::error::{CoreError, NetworkKind, Result};
use crate::session::{OutputSink, Session, SessionEvent, spawn_guarded};
use crate::tunnel::{TunnelLink, TunnelStream};

/// A shell channel plus the connection(s) that carry it.
pub struct SshSession {
    writer: ChannelWriteHalf<Msg>,
    handle: Arc<client::Handle<ClientHandler>>,
    /// Keeps the jump host connection (and thus the tunnel) alive.
    jump: Option<client::Handle<ClientHandler>>,
    reader: AbortHandle,
    info: SessionInfo,
    sink: OutputSink,
    /// Set by `close`: the end of the channel is then an `Exit`, never a lost connection.
    closing: Arc<AtomicBool>,
    /// A terminal event (`Exit` / `Lost`) was sent; guards against a second one.
    ended: Arc<AtomicBool>,
}

impl SshSession {
    /// Wrap an opened shell channel; spawns the reader task.
    pub(crate) fn start(
        channel: Channel<Msg>,
        handle: client::Handle<ClientHandler>,
        jump: Option<client::Handle<ClientHandler>>,
        info: SessionInfo,
        sink: OutputSink,
    ) -> Self {
        let (read_half, writer) = channel.split();
        let closing = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));
        let reader =
            spawn_guarded(forward_output(read_half, sink.clone(), closing.clone(), ended.clone()), sink.clone());
        Self { writer, handle: Arc::new(handle), jump, reader, info, sink, closing, ended }
    }

    /// This connection for port forwards. It does not keep the connection alive.
    pub fn link(&self) -> Arc<dyn TunnelLink> {
        Arc::new(SshLink { handle: Arc::downgrade(&self.handle) })
    }

    /// Negotiated algorithms, host key and auth methods of this connection.
    pub fn info(&self) -> &SessionInfo {
        &self.info
    }
}

struct SshLink {
    handle: Weak<client::Handle<ClientHandler>>,
}

impl SshLink {
    fn handle(&self) -> Result<Arc<client::Handle<ClientHandler>>> {
        self.handle
            .upgrade()
            .filter(|handle| !handle.is_closed())
            .ok_or_else(|| network_error(NetworkKind::Other, "nicht verbunden"))
    }
}

#[async_trait::async_trait]
impl TunnelLink for SshLink {
    async fn open_direct(&self, host: &str, port: u16, origin: SocketAddr) -> Result<TunnelStream> {
        let channel = self
            .handle()?
            .channel_open_direct_tcpip(host, u32::from(port), origin.ip().to_string(), u32::from(origin.port()))
            .await
            .map_err(map_russh_error)?;
        Ok(Box::new(channel.into_stream()))
    }

    async fn request_remote(&self, host: &str, port: u16) -> Result<u16> {
        let granted = self.handle()?.tcpip_forward(host, u32::from(port)).await.map_err(|e| match e {
            russh::Error::RequestDenied => {
                network_error(NetworkKind::Other, format!("der Server verweigert die Weiterleitung von {host}:{port}"))
            }
            other => map_russh_error(other),
        })?;
        // The server only names the port when it picked one (port 0).
        Ok(if port == 0 { u16::try_from(granted).unwrap_or(0) } else { port })
    }

    async fn cancel_remote(&self, host: &str, port: u16) -> Result<()> {
        self.handle()?.cancel_tcpip_forward(host, u32::from(port)).await.map_err(map_russh_error)
    }
}

/// Forward channel messages to the sink until the channel closes.
/// Exactly one terminal event is emitted at the end: `Exit` for a regular end, `Lost` otherwise.
async fn forward_output(
    mut read_half: ChannelReadHalf,
    sink: OutputSink,
    closing: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
) {
    let mut code = None;
    let mut saw_close = false;
    while let Some(msg) = read_half.wait().await {
        match msg {
            ChannelMsg::Data { data } => sink(SessionEvent::Output(data.to_vec())),
            ChannelMsg::ExtendedData { data, .. } => sink(SessionEvent::Output(data.to_vec())),
            ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
            ChannelMsg::Close => {
                saw_close = true;
                break;
            }
            _ => {}
        }
    }
    if !ended.swap(true, Ordering::SeqCst) {
        sink(end_event(saw_close, closing.load(Ordering::SeqCst), code));
    }
}

/// Whether a failed write means the connection is gone. A write can also fail because the server
/// just closed the channel (`exit` plus a keystroke); its regular `Exit` follows from the reader.
fn write_failure_is_lost(closing: bool, connection_closed: bool) -> bool {
    !closing && connection_closed
}

/// How a channel ended: the server's `Close` or our own `close` is a regular end; anything else
/// (connection reset, keepalive timeout, server gone) is a lost connection.
fn end_event(saw_close: bool, closing: bool, code: Option<u32>) -> SessionEvent {
    if saw_close || closing {
        SessionEvent::Exit { code }
    } else {
        SessionEvent::Lost(network_error(NetworkKind::Other, "connection lost"))
    }
}

#[async_trait::async_trait]
impl Session for SshSession {
    async fn write(&self, data: &[u8]) -> Result<()> {
        let result = self.writer.data(data).await.map_err(map_russh_error);
        if let Err(error @ CoreError::Network { .. }) = &result
            && write_failure_is_lost(self.closing.load(Ordering::SeqCst), self.handle.is_closed())
            && !self.ended.swap(true, Ordering::SeqCst)
        {
            // Hosts without keepalive notice a dead link only when typing.
            (self.sink)(SessionEvent::Lost(error.clone()));
        }
        result
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.writer.window_change(u32::from(cols), u32::from(rows), 0, 0).await.map_err(map_russh_error)
    }

    async fn close(&self) -> Result<()> {
        self.closing.store(true, Ordering::SeqCst);
        // Best effort: the remote side may already be gone.
        let _ = self.writer.close().await;
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "en").await;
        if let Some(jump) = &self.jump {
            let _ = jump.disconnect(Disconnect::ByApplication, "", "en").await;
        }
        Ok(())
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;

    #[test]
    fn channel_end_without_close_is_a_lost_connection() {
        assert_eq!(end_event(true, false, Some(0)), SessionEvent::Exit { code: Some(0) });
        // We closed it ourselves: never a reconnect, even if the server's Close never arrived.
        assert_eq!(end_event(false, true, None), SessionEvent::Exit { code: None });
        assert!(matches!(end_event(false, false, None), SessionEvent::Lost(CoreError::Network { .. })));
    }

    #[test]
    fn a_write_error_is_a_lost_connection_only_when_the_connection_is_gone() {
        assert!(write_failure_is_lost(false, true));
        // The server closed the channel (`exit` plus a keystroke): its regular end follows.
        assert!(!write_failure_is_lost(false, false));
        assert!(!write_failure_is_lost(true, true));
    }
}
