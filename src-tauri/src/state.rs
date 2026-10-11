//! Application state shared by all Tauri commands.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use linascp_core::connect::{ConnectEvent, PromptBroker};
use linascp_core::error::Result;
use linascp_core::model::Host;
use linascp_core::paths::AppPaths;
use linascp_core::secrets::{KeyringSecretStore, SecretStore};
use linascp_core::session::{SessionId, SessionManager};
use linascp_core::sshconfig::{import_ssh_config, merge_hosts};
use linascp_core::store::Store;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::output::OutputPump;

pub struct AppState {
    pub paths: AppPaths,
    pub store: Arc<Store>,
    pub secrets: Arc<dyn SecretStore>,
    pub sessions: SessionManager,
    /// Global prompt broker; prompts are published through each call's own emit
    /// (`ask_cancellable`), so its constructor emit is a no-op.
    pub broker: Arc<PromptBroker>,
    /// Running connection attempts keyed by the UI-generated connect id.
    pub connects: Mutex<HashMap<Uuid, CancellationToken>>,
    /// Output pumps of open sessions (needed by `session_ack` / `session_close`).
    pub outputs: Mutex<HashMap<SessionId, Arc<OutputPump>>>,
}

impl AppState {
    pub fn new(paths: AppPaths, store: Store, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            paths,
            store: Arc::new(store),
            secrets,
            sessions: SessionManager::default(),
            broker: Arc::new(PromptBroker::new(Arc::new(|_event: ConnectEvent| {}))),
            connects: Mutex::new(HashMap::new()),
            outputs: Mutex::new(HashMap::new()),
        }
    }

    /// Production state: XDG paths, on-disk SQLite store, OS keyring.
    pub fn init() -> Result<Self> {
        let paths = AppPaths::from_env()?;
        paths.ensure_dirs()?;
        let store = Store::open(&paths.db_path())?;
        Ok(Self::new(paths, store, Arc::new(KeyringSecretStore::new())))
    }

    /// Stored hosts merged with the read-only `~/.ssh/config` import (stored wins on equal name).
    /// A broken ssh config never blocks the app: it is logged and ignored.
    pub fn merged_hosts(&self) -> Result<Vec<Host>> {
        let stored = self.store.list_hosts()?;
        let path = self.paths.ssh_config();
        let imported = if path.exists() {
            import_ssh_config(&path).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %path.display(), "ignoring unreadable ssh config");
                Vec::new()
            })
        } else {
            Vec::new()
        };
        Ok(merge_hosts(stored, imported))
    }

    #[cfg(test)]
    pub fn for_tests(root: &std::path::Path) -> Self {
        use linascp_core::secrets::MemorySecretStore;
        let paths = AppPaths::for_root(root);
        std::fs::create_dir_all(&paths.ssh_dir).expect("create test ssh dir");
        Self::new(paths, Store::open_in_memory().expect("in-memory store"), Arc::new(MemorySecretStore::new()))
    }
}
