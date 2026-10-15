//! Interactive SSH shell session (implements `Session`).

use russh::client::{self, Msg};
use russh::{Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf, Disconnect};
use tokio::task::AbortHandle;

use super::SessionInfo;
use super::classify::map_russh_error;
use super::handler::ClientHandler;
use crate::error::Result;
use crate::session::{OutputSink, Session, SessionEvent, spawn_guarded};

/// A shell channel plus the connection(s) that carry it.
pub struct SshSession {
    writer: ChannelWriteHalf<Msg>,
    handle: client::Handle<ClientHandler>,
    /// Keeps the jump host connection (and thus the tunnel) alive.
    jump: Option<client::Handle<ClientHandler>>,
    reader: AbortHandle,
    info: SessionInfo,
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
        let reader = spawn_guarded(forward_output(read_half, sink.clone()), sink);
        Self { writer, handle, jump, reader, info }
    }

    /// Negotiated algorithms, host key and auth methods of this connection.
    pub fn info(&self) -> &SessionInfo {
        &self.info
    }
}

/// Forward channel messages to the sink until the channel closes.
/// Exactly one `Exit` is emitted at the end.
async fn forward_output(mut read_half: ChannelReadHalf, sink: OutputSink) {
    let mut code = None;
    while let Some(msg) = read_half.wait().await {
        match msg {
            ChannelMsg::Data { data } => sink(SessionEvent::Output(data.to_vec())),
            ChannelMsg::ExtendedData { data, .. } => sink(SessionEvent::Output(data.to_vec())),
            ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    sink(SessionEvent::Exit { code });
}

#[async_trait::async_trait]
impl Session for SshSession {
    async fn write(&self, data: &[u8]) -> Result<()> {
        self.writer.data(data).await.map_err(map_russh_error)
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.writer.window_change(u32::from(cols), u32::from(rows), 0, 0).await.map_err(map_russh_error)
    }

    async fn close(&self) -> Result<()> {
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
