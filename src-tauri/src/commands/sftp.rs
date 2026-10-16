//! SFTP commander: remote operations on `SftpSession`s and local operations (left pane).

use std::sync::Arc;
use std::time::Duration;

use linascp_core::connect::{
    ConnConnector, ConnStatus, ConnectEvent, EmitFn, PromptBroker, Reconnecting, SessionInfo, SftpSession,
    connect_sftp as run_pipeline,
};
use linascp_core::edit::{EditEvent, EditManager};
use linascp_core::error::{CoreError, Result};
use linascp_core::model::Id;
use linascp_core::session::SessionId;
use linascp_core::sftp::{Listing, local};
use linascp_core::transfer::{LocalSide, Side, TransferEvent, TransferOptions, TransferQueue};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use super::blocking;
use super::connect::{
    build_request, build_request_in, channel_emit, connect_deps, connect_deps_in, fail, find_host, find_host_in,
    register_connect,
};
use super::settings::auto_reconnect_enabled;
use crate::editor::SystemLauncher;
use crate::state::{AppState, ConnectEnv};
use crate::util::lock;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SftpConnected {
    pub session_id: SessionId,
    pub info: SessionInfo,
    pub version: u32,
    pub listing: Listing,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SftpEvent {
    Reconnecting {
        attempt: u32,
        max_attempts: u32,
        retry_in_ms: u32,
        reason: CoreError,
    },
    Reconnected {
        info: SessionInfo,
    },
    /// Reconnecting gave up (or is off); `connect_sftp` with this session id re-attaches.
    Lost {
        error: CoreError,
    },
    Transfer {
        event: TransferEvent,
    },
    Edit {
        event: EditEvent,
    },
}

pub(crate) type SftpEmit = Arc<dyn Fn(SftpEvent) + Send + Sync>;

/// One SFTP tab: the self-healing connection, its transfer queue and the files being edited.
pub struct SftpEntry {
    pub conn: Reconnecting<SftpSession>,
    pub queue: TransferQueue,
    pub edits: EditManager,
}

/// How often edited copies are checked for a save.
const EDIT_POLL: Duration = Duration::from_secs(1);

fn status_event(status: ConnStatus) -> SftpEvent {
    match status {
        ConnStatus::Reconnecting { attempt, max_attempts, retry_in, reason } => SftpEvent::Reconnecting {
            attempt,
            max_attempts,
            retry_in_ms: u32::try_from(retry_in.as_millis()).unwrap_or(u32::MAX),
            reason,
        },
        ConnStatus::Reconnected { info } => SftpEvent::Reconnected { info },
        ConnStatus::Lost { error } => SftpEvent::Lost { error },
    }
}

/// Reconnect attempts: pipeline without prompts; a refused prompt is `InputRequired`.
fn sftp_connector(env: ConnectEnv, host_id: Id) -> ConnConnector<SftpSession> {
    Arc::new(move |on_lost, cancel| {
        let env = env.clone();
        Box::pin(async move {
            let host = find_host_in(&env, host_id)?;
            let request = build_request_in(&env, host, 80, 24)?;
            let broker = Arc::new(PromptBroker::non_interactive());
            let quiet: EmitFn = Arc::new(|_| {});
            match run_pipeline(request, connect_deps_in(&env), broker.clone(), quiet, on_lost, cancel).await {
                Ok((session, _)) => Ok(Arc::new(session)),
                Err(_) if broker.refused_input() => Err(CoreError::InputRequired),
                Err(error) => Err(error),
            }
        })
    })
}

fn new_entry(state: &AppState, host: &linascp_core::model::Host, sftp_emit: SftpEmit) -> Arc<SftpEntry> {
    let queue = {
        let emit = sftp_emit.clone();
        TransferQueue::new(
            Arc::new(LocalSide),
            TransferOptions::from_settings(&host.sftp),
            Arc::new(move |event| emit(SftpEvent::Transfer { event })),
        )
    };
    let edits = {
        let emit = sftp_emit.clone();
        EditManager::new(
            queue.clone(),
            state.paths.edit_dir(),
            Arc::new(SystemLauncher::new(state.store.clone())),
            Arc::new(move |event| emit(SftpEvent::Edit { event })),
            EDIT_POLL,
        )
    };
    let store = state.store.clone();
    let conn = Reconnecting::new(
        sftp_connector(state.connect_env(), host.id),
        Arc::new(move || auto_reconnect_enabled(&store)),
        Arc::new(move |status| sftp_emit(status_event(status))),
        {
            let queue = queue.clone();
            Arc::new(move |session: Option<Arc<SftpSession>>| queue.set_remote(session.map(|s| s as Arc<dyn Side>)))
        },
    );
    Arc::new(SftpEntry { conn, queue, edits })
}

/// `existing`: re-attach a fresh, interactive connection to that tab's session (queue survives).
pub(crate) async fn connect_sftp_inner(
    state: &AppState,
    connect_id: Uuid,
    host_id: Id,
    existing: Option<SessionId>,
    emit: EmitFn,
    sftp_emit: SftpEmit,
) -> Result<SftpConnected> {
    let guard = register_connect(state, connect_id);
    let host = match find_host(state, host_id) {
        Ok(host) => host,
        Err(error) => return fail(&emit, error),
    };
    let (session_id, entry) = match existing {
        Some(id) => match lookup(state, id) {
            Ok(entry) => (id, entry),
            Err(error) => return fail(&emit, error),
        },
        None => (Uuid::new_v4(), new_entry(state, &host, sftp_emit)),
    };
    let request = match build_request(state, host, 80, 24) {
        Ok(request) => request,
        Err(error) => return fail(&emit, error),
    };
    if guard.is_cancelled() {
        return fail(&emit, CoreError::Cancelled);
    }
    let cancel = guard.token();
    let (generation, on_lost) = entry.conn.begin();
    let result =
        run_pipeline(request, connect_deps(state), state.broker.clone(), emit.clone(), on_lost, cancel.clone()).await;
    drop(guard);
    match result {
        Ok((session, _)) if cancel.is_cancelled() => {
            session.close().await;
            if existing.is_some() {
                entry.conn.abandon(generation, CoreError::Cancelled);
            }
            fail(&emit, CoreError::Cancelled)
        }
        Ok((session, listing)) => {
            let connected =
                SftpConnected { session_id, info: session.info().clone(), version: session.version(), listing };
            entry.conn.attach(generation, Arc::new(session));
            lock(&state.sftp).insert(session_id, entry);
            emit(ConnectEvent::Connected { session_id });
            Ok(connected)
        }
        Err(error) => {
            // A failed first connect has no tab yet: only a re-attach reports `Lost` again.
            if existing.is_some() {
                entry.conn.abandon(generation, error.clone());
            }
            Err(error)
        }
    }
}

pub(crate) fn lookup(state: &AppState, id: SessionId) -> Result<Arc<SftpEntry>> {
    lock(&state.sftp)
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::Internal { message: format!("unknown sftp session {id}") })
}

fn session(state: &AppState, id: SessionId) -> Result<Arc<SftpSession>> {
    lookup(state, id)?.conn.current()
}

pub(crate) async fn sftp_list_inner(state: &AppState, id: SessionId, path: String) -> Result<Listing> {
    session(state, id)?.list(&path).await
}

pub(crate) async fn local_list_inner(path: String) -> Result<Listing> {
    blocking(move || local::list(&path)).await
}
pub(crate) async fn local_mkdir_inner(dir: String, name: String) -> Result<()> {
    blocking(move || local::mkdir(&dir, &name)).await
}
pub(crate) async fn local_rename_inner(dir: String, from: String, to: String) -> Result<()> {
    blocking(move || local::rename(&dir, &from, &to)).await
}
pub(crate) async fn local_trash_inner(state: &AppState, dir: String, names: Vec<String>) -> Result<()> {
    let root = state.paths.trash_dir();
    blocking(move || local::trash(&root, &dir, &names)).await
}
pub(crate) fn local_home_inner(state: &AppState, host_id: Option<Id>) -> Result<String> {
    let start = match host_id {
        Some(id) => find_host(state, id)?.sftp.local_start,
        None => None,
    };
    Ok(local::home(start.as_deref(), &state.paths.home))
}

#[tauri::command]
#[specta::specta]
pub async fn connect_sftp(
    state: State<'_, AppState>,
    connect_id: Uuid,
    host_id: Id,
    session_id: Option<SessionId>,
    events: Channel<ConnectEvent>,
    sftp_events: Channel<SftpEvent>,
) -> Result<SftpConnected> {
    let sftp_emit: SftpEmit = Arc::new(move |event| {
        if let Err(e) = sftp_events.send(event) {
            tracing::debug!(error = %e, "sftp event dropped: channel closed");
        }
    });
    connect_sftp_inner(&state, connect_id, host_id, session_id, channel_emit(events), sftp_emit).await
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_list(state: State<'_, AppState>, session_id: SessionId, path: String) -> Result<Listing> {
    sftp_list_inner(&state, session_id, path).await
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_mkdir(state: State<'_, AppState>, session_id: SessionId, dir: String, name: String) -> Result<()> {
    session(&state, session_id)?.mkdir(&dir, &name).await
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    session_id: SessionId,
    dir: String,
    from: String,
    to: String,
) -> Result<()> {
    session(&state, session_id)?.rename(&dir, &from, &to).await
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_delete(
    state: State<'_, AppState>,
    session_id: SessionId,
    dir: String,
    names: Vec<String>,
) -> Result<()> {
    session(&state, session_id)?.delete(&dir, &names).await
}

/// Disconnects and forgets the session; unknown ids are fine (already closed). Returns the
/// edited copies that were kept because they hold changes the server does not have.
#[tauri::command]
#[specta::specta]
pub async fn sftp_close(state: State<'_, AppState>, session_id: SessionId) -> Result<Vec<String>> {
    let removed = lock(&state.sftp).remove(&session_id);
    let Some(entry) = removed else { return Ok(Vec::new()) };
    let kept = entry.edits.close().await;
    entry.queue.close().await;
    entry.conn.close().await;
    Ok(kept.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn local_list(path: String) -> Result<Listing> {
    local_list_inner(path).await
}

#[tauri::command]
#[specta::specta]
pub async fn local_home(state: State<'_, AppState>, host_id: Option<Id>) -> Result<String> {
    local_home_inner(&state, host_id)
}

#[tauri::command]
#[specta::specta]
pub async fn local_mkdir(dir: String, name: String) -> Result<()> {
    local_mkdir_inner(dir, name).await
}

#[tauri::command]
#[specta::specta]
pub async fn local_rename(dir: String, from: String, to: String) -> Result<()> {
    local_rename_inner(dir, from, to).await
}

#[tauri::command]
#[specta::specta]
pub async fn local_delete(dir: String, names: Vec<String>) -> Result<()> {
    blocking(move || local::delete(&dir, &names)).await
}

#[tauri::command]
#[specta::specta]
pub async fn local_trash(state: State<'_, AppState>, dir: String, names: Vec<String>) -> Result<()> {
    local_trash_inner(&state, dir, names).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn setup() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        (dir, state)
    }

    #[tokio::test]
    async fn unknown_sftp_session_is_an_error() {
        let (_d, state) = setup();
        let result = sftp_list_inner(&state, Uuid::new_v4(), "/".into()).await;
        assert!(matches!(result, Err(CoreError::Internal { .. })));
    }

    #[tokio::test]
    async fn local_commands_work_on_a_temp_dir() {
        let (d, state) = setup();
        let root = d.path().canonicalize().unwrap().to_string_lossy().into_owned();
        local_mkdir_inner(root.clone(), "x".into()).await.unwrap();
        local_rename_inner(root.clone(), "x".into(), "y".into()).await.unwrap();
        local_trash_inner(&state, root.clone(), vec!["y".into()]).await.unwrap();
        assert!(state.paths.trash_dir().join("files/y").is_dir());
        let listing = local_list_inner(root.clone()).await.unwrap();
        assert!(!listing.entries.iter().any(|e| e.name == "y"));
    }

    #[tokio::test]
    async fn local_home_uses_the_hosts_local_start() {
        let (_d, state) = setup();
        let mut host = linascp_core::model::Host::new("h", "h.example");
        host.sftp.local_start = Some("/srv".into());
        let host = crate::commands::hosts::save_host(&state, host).unwrap();
        assert_eq!(local_home_inner(&state, Some(host.id)).unwrap(), "/srv");
        assert_eq!(local_home_inner(&state, None).unwrap(), state.paths.home.to_string_lossy());
    }

    fn quiet_sftp() -> SftpEmit {
        Arc::new(|_| {})
    }

    #[tokio::test]
    async fn connect_sftp_for_a_missing_host_emits_exactly_one_failed() {
        let (_d, state) = setup();
        let events: Arc<StdMutex<Vec<ConnectEvent>>> = Arc::default();
        let emit: EmitFn = {
            let events = events.clone();
            Arc::new(move |e| events.lock().unwrap().push(e))
        };
        let result = connect_sftp_inner(&state, Uuid::new_v4(), Uuid::new_v4(), None, emit, quiet_sftp()).await;
        assert!(result.is_err());
        let events = events.lock().unwrap();
        assert_eq!(events.iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count(), 1);
    }

    #[tokio::test]
    async fn reattaching_to_an_unknown_session_fails_once() {
        let (_d, state) = setup();
        let host = crate::commands::hosts::save_host(&state, linascp_core::model::Host::new("h", "h.example")).unwrap();
        let events: Arc<StdMutex<Vec<ConnectEvent>>> = Arc::default();
        let emit: EmitFn = {
            let events = events.clone();
            Arc::new(move |e| events.lock().unwrap().push(e))
        };
        let result =
            connect_sftp_inner(&state, Uuid::new_v4(), host.id, Some(Uuid::new_v4()), emit, quiet_sftp()).await;
        assert!(result.is_err());
        assert_eq!(events.lock().unwrap().iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count(), 1);
    }

    #[test]
    fn sftp_events_serialize_for_the_ui() {
        let event =
            SftpEvent::Reconnecting { attempt: 2, max_attempts: 8, retry_in_ms: 2000, reason: CoreError::Cancelled };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "kind": "reconnecting", "attempt": 2, "maxAttempts": 8, "retryInMs": 2000, "reason": { "type": "cancelled" } })
        );
        let event =
            SftpEvent::Transfer { event: linascp_core::transfer::TransferEvent::Progress { job_id: 1, done: 2 } };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "kind": "transfer", "event": { "kind": "progress", "jobId": 1, "done": 2 } })
        );
        let file = linascp_core::edit::EditFile {
            id: 3,
            name: "a.conf".into(),
            remote_path: "/etc/a.conf".into(),
            state: linascp_core::edit::EditState::Synced,
            dirty: false,
            error: None,
        };
        let event = SftpEvent::Edit { event: linascp_core::edit::EditEvent::File { file } };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "kind": "edit", "event": { "kind": "file", "file": {
                "id": 3, "name": "a.conf", "remotePath": "/etc/a.conf", "state": "synced", "dirty": false, "error": null
            } } })
        );
    }
}
