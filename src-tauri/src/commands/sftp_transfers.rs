//! Transfer queue and reconnect commands of an SFTP tab (TP3b).

use std::sync::Arc;

use linascp_core::error::Result;
use linascp_core::session::SessionId;
use linascp_core::transfer::{BatchId, CancelTarget, Decision, Direction, JobId};
use tauri::State;

use super::sftp::{SftpEntry, lookup};
use crate::state::AppState;

pub(crate) fn entry(state: &AppState, id: SessionId) -> Result<Arc<SftpEntry>> {
    lookup(state, id)
}

pub(crate) fn with_entry<T>(state: &AppState, id: SessionId, f: impl FnOnce(&SftpEntry) -> Result<T>) -> Result<T> {
    let entry = entry(state, id)?;
    f(&entry)
}

pub(crate) async fn enqueue(
    state: &AppState,
    id: SessionId,
    direction: Direction,
    sources: Vec<String>,
    target_dir: String,
) -> Result<BatchId> {
    let entry = entry(state, id)?;
    entry.queue.enqueue(direction, sources, target_dir).await
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_enqueue(
    state: State<'_, AppState>,
    session_id: SessionId,
    direction: Direction,
    sources: Vec<String>,
    target_dir: String,
) -> Result<BatchId> {
    enqueue(&state, session_id, direction, sources, target_dir).await
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_resolve(
    state: State<'_, AppState>,
    session_id: SessionId,
    job_id: JobId,
    decision: Decision,
    apply_to_all: bool,
) -> Result<()> {
    with_entry(&state, session_id, |e| e.queue.resolve(job_id, decision, apply_to_all))
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_cancel(state: State<'_, AppState>, session_id: SessionId, target: CancelTarget) -> Result<()> {
    with_entry(&state, session_id, |e| e.queue.cancel(target))
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_retry(state: State<'_, AppState>, session_id: SessionId, job_id: JobId) -> Result<()> {
    with_entry(&state, session_id, |e| e.queue.retry(job_id))
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_clear(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    with_entry(&state, session_id, |e| {
        e.queue.clear();
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_reconnect_now(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    with_entry(&state, session_id, |e| {
        e.conn.reconnect_now();
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_stop_reconnect(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    with_entry(&state, session_id, |e| {
        e.conn.stop();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use linascp_core::transfer::{CancelTarget, Decision, Direction};

    #[tokio::test]
    async fn unknown_sessions_are_errors_not_panics() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let id = uuid::Uuid::new_v4();
        assert!(enqueue(&state, id, Direction::Upload, vec!["/a".into()], "/b".into()).await.is_err());
        assert!(entry(&state, id).is_err());
        assert!(with_entry(&state, id, |e| e.queue.resolve(1, Decision::Skip, false)).is_err());
        assert!(with_entry(&state, id, |e| e.queue.cancel(CancelTarget::Job { id: 1 })).is_err());
    }
}
