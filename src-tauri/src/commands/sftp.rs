//! SFTP commander: remote operations on `SftpSession`s and local operations (left pane).

use std::sync::Arc;

use linascp_core::connect::{ConnectEvent, EmitFn, LostFn, SessionInfo, SftpSession, connect_sftp as run_pipeline};
use linascp_core::error::{CoreError, Result};
use linascp_core::model::Id;
use linascp_core::session::SessionId;
use linascp_core::sftp::{Listing, local};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use super::blocking;
use super::connect::{build_request, channel_emit, connect_deps, fail, find_host, register_connect};
use crate::state::AppState;
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
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SftpEvent {
    Lost { error: CoreError },
}

pub(crate) async fn connect_sftp_inner(
    state: &AppState,
    connect_id: Uuid,
    host_id: Id,
    emit: EmitFn,
    on_lost: LostFn,
) -> Result<SftpConnected> {
    let guard = register_connect(state, connect_id);
    let host = match find_host(state, host_id) {
        Ok(host) => host,
        Err(error) => return fail(&emit, error),
    };
    let request = match build_request(state, host, 80, 24) {
        Ok(request) => request,
        Err(error) => return fail(&emit, error),
    };
    if guard.is_cancelled() {
        return fail(&emit, CoreError::Cancelled);
    }
    let cancel = guard.token();
    let result =
        run_pipeline(request, connect_deps(state), state.broker.clone(), emit.clone(), on_lost, cancel.clone()).await;
    drop(guard);
    match result {
        Ok((session, _)) if cancel.is_cancelled() => {
            session.close().await;
            fail(&emit, CoreError::Cancelled)
        }
        Ok((session, listing)) => {
            let session_id = Uuid::new_v4();
            let connected =
                SftpConnected { session_id, info: session.info().clone(), version: session.version(), listing };
            lock(&state.sftp).insert(session_id, Arc::new(session));
            emit(ConnectEvent::Connected { session_id });
            Ok(connected)
        }
        Err(error) => Err(error),
    }
}

fn session(state: &AppState, id: SessionId) -> Result<Arc<SftpSession>> {
    lock(&state.sftp)
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::Internal { message: format!("unknown sftp session {id}") })
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
    events: Channel<ConnectEvent>,
    sftp_events: Channel<SftpEvent>,
) -> Result<SftpConnected> {
    let on_lost: LostFn = Arc::new(move |error| {
        if let Err(e) = sftp_events.send(SftpEvent::Lost { error }) {
            tracing::debug!(error = %e, "sftp event dropped: channel closed");
        }
    });
    connect_sftp_inner(&state, connect_id, host_id, channel_emit(events), on_lost).await
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

/// Disconnects and forgets the session; unknown ids are fine (already closed).
#[tauri::command]
#[specta::specta]
pub async fn sftp_close(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    let removed = lock(&state.sftp).remove(&session_id);
    if let Some(session) = removed {
        session.close().await;
    }
    Ok(())
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

    #[tokio::test]
    async fn connect_sftp_for_a_missing_host_emits_exactly_one_failed() {
        let (_d, state) = setup();
        let events: Arc<StdMutex<Vec<ConnectEvent>>> = Arc::default();
        let emit: EmitFn = {
            let events = events.clone();
            Arc::new(move |e| events.lock().unwrap().push(e))
        };
        let result = connect_sftp_inner(&state, Uuid::new_v4(), Uuid::new_v4(), emit, Arc::new(|_| {})).await;
        assert!(result.is_err());
        let events = events.lock().unwrap();
        assert_eq!(events.iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count(), 1);
    }
}
