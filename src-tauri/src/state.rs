//! Application state shared by all Tauri commands.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
    /// `Arc`-wrapped (not just the usual `&AppState` borrow) so a session's own output sink
    /// can carry a cheap, 'static-owned handle for self-cleanup on exit/error — see
    /// `commands::sessions::cleanup_fn`.
    pub sessions: Arc<SessionManager>,
    /// Global prompt broker; prompts are published through each call's own emit
    /// (`ask_cancellable`), so its constructor emit is a no-op.
    pub broker: Arc<PromptBroker>,
    /// Running connection attempts keyed by the UI-generated connect id.
    pub connects: Mutex<HashMap<Uuid, CancellationToken>>,
    /// Cancels that arrived before their attempt registered (see `cancel_connect`).
    pub early_cancels: Mutex<HashMap<Uuid, Instant>>,
    /// Output pumps of open sessions (needed by `session_ack` / `session_close`). `Arc`-wrapped
    /// for the same self-cleanup reason as `sessions`.
    pub outputs: Arc<Mutex<HashMap<SessionId, Arc<OutputPump>>>>,
    /// Shell of local terminals instead of `$SHELL`; set by tests so the developer's rc files never run.
    pub local_shell: Option<String>,
}

impl AppState {
    pub fn new(paths: AppPaths, store: Store, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            paths,
            store: Arc::new(store),
            secrets,
            sessions: Arc::new(SessionManager::default()),
            broker: Arc::new(PromptBroker::new(Arc::new(|_event: ConnectEvent| {}))),
            connects: Mutex::new(HashMap::new()),
            early_cancels: Mutex::new(HashMap::new()),
            outputs: Arc::new(Mutex::new(HashMap::new())),
            local_shell: None,
        }
    }

    /// Production state: XDG paths, on-disk SQLite store, OS keyring.
    pub fn init() -> Result<Self> {
        let paths = AppPaths::from_env()?;
        paths.ensure_dirs()?;
        let store = Store::open(&paths.db_path())?;
        Ok(Self::new(paths, store, Arc::new(KeyringSecretStore::new())))
    }

    pub fn connect_env(&self) -> ConnectEnv {
        ConnectEnv { paths: self.paths.clone(), store: self.store.clone(), secrets: self.secrets.clone() }
    }

    pub fn merged_hosts(&self) -> Result<Vec<Host>> {
        self.connect_env().merged_hosts()
    }

    #[cfg(test)]
    pub fn for_tests(root: &std::path::Path) -> Self {
        use linascp_core::secrets::MemorySecretStore;
        let paths = AppPaths::for_root(root);
        std::fs::create_dir_all(&paths.ssh_dir).expect("create test ssh dir");
        let mut state =
            Self::new(paths, Store::open_in_memory().expect("in-memory store"), Arc::new(MemorySecretStore::new()));
        state.local_shell = Some("/bin/sh".into());
        state
    }
}

/// What a connect needs from the app state, owned and cheap to clone (reconnects run long after
/// the command that started the session returned).
#[derive(Clone)]
pub struct ConnectEnv {
    pub paths: AppPaths,
    pub store: Arc<Store>,
    pub secrets: Arc<dyn SecretStore>,
}

impl ConnectEnv {
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
}
