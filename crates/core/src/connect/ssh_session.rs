//! Interactive SSH shell session (implements `Session`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use russh::client::{self, Msg};
use russh::{Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf, Disconnect};
use tokio::task::AbortHandle;

use super::SessionInfo;
use super::classify::{map_russh_error, network_error};
use super::handler::ClientHandler;
use crate::error::{CoreError, NetworkKind, Result};
use crate::session::{OutputSink, Session, SessionEvent, spawn_guarded};

/// A shell channel plus the connection(s) that carry it.
pub struct SshSession {
    writer: ChannelWriteHalf<Msg>,
    handle: client::Handle<ClientHandler>,
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
        Self { writer, handle, jump, reader, info, sink, closing, ended }
    }

    /// Negotiated algorithms, host key and auth methods of this connection.
    pub fn info(&self) -> &SessionInfo {
        &self.info
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
            && !self.closing.load(Ordering::SeqCst)
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
}
