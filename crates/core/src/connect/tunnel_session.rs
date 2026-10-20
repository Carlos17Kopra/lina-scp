//! Shell-less SSH connection that only carries port forwards.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use russh::client;
use tokio::task::AbortHandle;

use super::SessionInfo;
use super::handler::ClientHandler;
use super::sftp_session::{Lost, LostFn, watch_closed};
use super::ssh_session::ssh_link;
use crate::tunnel::TunnelLink;

pub struct TunnelSession {
    handle: Arc<client::Handle<ClientHandler>>,
    /// Keeps the jump host connection (and thus the tunnel) alive.
    jump: Option<client::Handle<ClientHandler>>,
    info: SessionInfo,
    lost: Arc<Lost>,
    watcher: AbortHandle,
}

impl TunnelSession {
    pub(crate) fn start(
        handle: client::Handle<ClientHandler>,
        jump: Option<client::Handle<ClientHandler>>,
        info: SessionInfo,
        on_lost: LostFn,
    ) -> Self {
        let handle = Arc::new(handle);
        let lost = Lost::new(on_lost);
        let watcher = watch_closed(handle.clone(), lost.clone());
        Self { handle, jump, info, lost, watcher }
    }

    /// This connection for port forwards. It does not keep the connection alive.
    pub fn link(&self) -> Arc<dyn TunnelLink> {
        ssh_link(&self.handle)
    }

    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    /// Ends the connection without a `Lost` event.
    pub async fn close(&self) {
        self.lost.fired.store(true, Ordering::SeqCst);
        self.watcher.abort();
        let _ = self.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        if let Some(jump) = &self.jump {
            let _ = jump.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        }
    }
}

#[async_trait::async_trait]
impl super::sftp_reconnect::Connection for TunnelSession {
    fn info(&self) -> SessionInfo {
        self.info.clone()
    }
    async fn close(&self) {
        TunnelSession::close(self).await;
    }
}

impl Drop for TunnelSession {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}
