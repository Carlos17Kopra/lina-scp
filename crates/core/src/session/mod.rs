//! Session abstraction shared by local PTYs and SSH shells.

pub mod local;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::{CoreError, Result};

pub use local::LocalPtySession;

/// Identifier of a running session (assigned by the caller).
pub type SessionId = uuid::Uuid;

/// Events a session pushes to its consumer.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Raw terminal output bytes.
    Output(Vec<u8>),
    /// The session ended; `code` is the exit status if known.
    Exit { code: Option<u32> },
    /// A fatal error inside the session task.
    Error(CoreError),
}

/// Callback receiving session events. Must be cheap and non-blocking.
pub type OutputSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

/// Common interface of all interactive sessions.
#[async_trait::async_trait]
pub trait Session: Send + Sync {
    /// Send input bytes to the session.
    async fn write(&self, data: &[u8]) -> Result<()>;
    /// Inform the session about a new terminal size.
    async fn resize(&self, cols: u16, rows: u16) -> Result<()>;
    /// Terminate the session. An `Exit` event follows asynchronously.
    async fn close(&self) -> Result<()>;
}

/// Registry of running sessions, keyed by `SessionId`.
#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<SessionId, Arc<dyn Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, id: SessionId, session: Arc<dyn Session>) {
        self.lock().insert(id, session);
    }

    pub fn get(&self, id: SessionId) -> Option<Arc<dyn Session>> {
        self.lock().get(&id).cloned()
    }

    pub fn remove(&self, id: SessionId) -> Option<Arc<dyn Session>> {
        self.lock().remove(&id)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, Arc<dyn Session>>> {
        // A poisoned map is still structurally valid; keep serving it.
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct FakeSession {
        writes: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Session for FakeSession {
        async fn write(&self, _data: &[u8]) -> Result<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn resize(&self, _cols: u16, _rows: u16) -> Result<()> {
            Ok(())
        }
        async fn close(&self) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn manager_insert_get_remove() {
        let manager = SessionManager::new();
        let id = SessionId::new_v4();
        let fake = Arc::new(FakeSession::default());
        manager.insert(id, fake.clone());

        let got = manager.get(id).expect("session registered");
        got.write(b"x").await.unwrap();
        assert_eq!(fake.writes.load(Ordering::SeqCst), 1);

        assert!(manager.remove(id).is_some());
        assert!(manager.get(id).is_none());
        assert!(manager.remove(id).is_none());
    }

    #[test]
    fn manager_get_unknown_is_none() {
        let manager = SessionManager::new();
        assert!(manager.get(SessionId::new_v4()).is_none());
    }
}
