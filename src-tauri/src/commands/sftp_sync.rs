//! Compare and synchronize the two panes of an SFTP tab (TP4b).

use std::path::PathBuf;
use std::time::Duration;

use linascp_core::error::Result;
use linascp_core::session::SessionId;
use linascp_core::sftp::local;
use linascp_core::transfer::{
    BatchId, Difference, Direction, LocalSide, SyncDeleteReport, SyncItem, compare, delete_unchanged, enqueue_sync,
    keep_up_to_date,
};
use serde::Deserialize;
use tauri::State;
use tokio_util::sync::CancellationToken;

use super::blocking;
use super::sftp::lookup;
use crate::state::AppState;
use crate::util::lock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PaneSide {
    Local,
    Remote,
}

/// Local deletes go to the trash.
pub(crate) async fn delete_local_inner(
    trash: PathBuf,
    base: String,
    items: Vec<SyncItem>,
    exclude: Vec<String>,
) -> Result<SyncDeleteReport> {
    delete_unchanged(&LocalSide, &base, items, &exclude, |dir: String, name: String| {
        let trash = trash.clone();
        blocking(move || local::trash(&trash, &dir, &[name]))
    })
    .await
}

/// One compare per session at a time: a new one cancels the previous.
#[tauri::command]
#[specta::specta]
pub async fn sync_compare(
    state: State<'_, AppState>,
    session_id: SessionId,
    local_dir: String,
    remote_dir: String,
    exclude: Vec<String>,
) -> Result<Vec<Difference>> {
    let entry = lookup(&state, session_id)?;
    let session = entry.conn.current()?;
    let cancel = CancellationToken::new();
    if let Some(previous) = lock(&entry.compare).replace(cancel.clone()) {
        previous.cancel();
    }
    compare(&LocalSide, &*session, &local_dir, &remote_dir, &exclude, &cancel).await
}

#[tauri::command]
#[specta::specta]
pub async fn sync_cancel(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    if let Some(cancel) = lock(&lookup(&state, session_id)?.compare).take() {
        cancel.cancel();
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn sync_enqueue(
    state: State<'_, AppState>,
    session_id: SessionId,
    direction: Direction,
    local_dir: String,
    remote_dir: String,
    items: Vec<SyncItem>,
    exclude: Vec<String>,
) -> Result<Vec<BatchId>> {
    let entry = lookup(&state, session_id)?;
    enqueue_sync(&entry.queue, direction, &local_dir, &remote_dir, items, &exclude).await
}

/// Remote deletes are permanent; the preview announced them in red.
#[tauri::command]
#[specta::specta]
pub async fn sync_delete(
    state: State<'_, AppState>,
    session_id: SessionId,
    side: PaneSide,
    base_dir: String,
    items: Vec<SyncItem>,
    exclude: Vec<String>,
) -> Result<SyncDeleteReport> {
    match side {
        PaneSide::Local => delete_local_inner(state.paths.trash_dir(), base_dir, items, exclude).await,
        PaneSide::Remote => {
            let session = lookup(&state, session_id)?.conn.current()?;
            delete_unchanged(&*session, &base_dir, items, &exclude, |dir: String, name: String| {
                let session = session.clone();
                async move { session.delete(&dir, &[name]).await }
            })
            .await
        }
    }
}

/// How often a kept-up-to-date local directory is read.
const KEEP_POLL: Duration = Duration::from_secs(2);

pub(crate) async fn keep_start_inner(
    state: &AppState,
    session_id: SessionId,
    local_dir: String,
    remote_dir: String,
    exclude: Vec<String>,
) -> Result<()> {
    let entry = lookup(state, session_id)?;
    lock(&entry.keep).take();
    let handle =
        keep_up_to_date(entry.queue.clone(), local_dir, remote_dir, exclude, KEEP_POLL, entry.keep_emit.clone())
            .await?;
    *lock(&entry.keep) = Some(handle);
    Ok(())
}

/// Uploads every local change from now on (TP4c); one per tab, a new one replaces the running one.
#[tauri::command]
#[specta::specta]
pub async fn keep_start(
    state: State<'_, AppState>,
    session_id: SessionId,
    local_dir: String,
    remote_dir: String,
    exclude: Vec<String>,
) -> Result<()> {
    keep_start_inner(&state, session_id, local_dir, remote_dir, exclude).await
}

#[tauri::command]
#[specta::specta]
pub async fn keep_stop(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    lock(&lookup(&state, session_id)?.keep).take();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use linascp_core::transfer::FileStamp;
    use std::os::unix::fs::MetadataExt;

    #[tokio::test]
    async fn local_sync_deletes_go_to_the_trash_and_skip_changed_files() {
        let d = tempfile::tempdir().unwrap();
        let (base, trash) = (d.path().join("base"), d.path().join("trash"));
        std::fs::create_dir(&base).unwrap();
        std::fs::write(base.join("old"), b"x").unwrap();
        std::fs::write(base.join("edited"), b"x").unwrap();
        let stamp = |name: &str| {
            let meta = std::fs::metadata(base.join(name)).unwrap();
            Some(FileStamp { size: meta.len(), mtime: Some(meta.mtime()) })
        };
        let items = vec![
            SyncItem { rel: "old".into(), expected: stamp("old"), tree: None },
            SyncItem { rel: "edited".into(), expected: stamp("edited"), tree: None },
        ];
        std::fs::write(base.join("edited"), b"edited meanwhile").unwrap();
        let report =
            delete_local_inner(trash.clone(), base.to_string_lossy().into_owned(), items, Vec::new()).await.unwrap();
        assert_eq!((report.deleted, report.skipped, report.failed), (1, 1, 0));
        assert!(trash.join("files/old").exists() && base.join("edited").exists());
    }

    #[tokio::test]
    async fn keep_start_needs_a_known_session() {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(d.path());
        let result = keep_start_inner(&state, uuid::Uuid::new_v4(), "/l".into(), "/r".into(), vec![]).await;
        assert!(matches!(result, Err(linascp_core::error::CoreError::Internal { .. })));
    }
}
