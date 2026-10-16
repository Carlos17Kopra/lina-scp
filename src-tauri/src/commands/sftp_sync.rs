//! Compare and synchronize the two panes of an SFTP tab (TP4b).

use std::path::PathBuf;

use linascp_core::error::Result;
use linascp_core::session::SessionId;
use linascp_core::sftp::local;
use linascp_core::transfer::{
    BatchId, Difference, Direction, LocalSide, SyncDeleteReport, SyncItem, compare, delete_unchanged, enqueue_sync,
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
pub(crate) async fn delete_local_inner(trash: PathBuf, base: String, items: Vec<SyncItem>) -> Result<SyncDeleteReport> {
    delete_unchanged(&LocalSide, &base, items, |dir: String, name: String| {
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
) -> Result<Vec<BatchId>> {
    let entry = lookup(&state, session_id)?;
    enqueue_sync(&entry.queue, direction, &local_dir, &remote_dir, items).await
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
) -> Result<SyncDeleteReport> {
    match side {
        PaneSide::Local => delete_local_inner(state.paths.trash_dir(), base_dir, items).await,
        PaneSide::Remote => {
            let session = lookup(&state, session_id)?.conn.current()?;
            delete_unchanged(&*session, &base_dir, items, |dir: String, name: String| {
                let session = session.clone();
                async move { session.delete(&dir, &[name]).await }
            })
            .await
        }
    }
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
            SyncItem { rel: "old".into(), expected: stamp("old") },
            SyncItem { rel: "edited".into(), expected: stamp("edited") },
        ];
        std::fs::write(base.join("edited"), b"edited meanwhile").unwrap();
        let report = delete_local_inner(trash.clone(), base.to_string_lossy().into_owned(), items).await.unwrap();
        assert_eq!((report.deleted, report.skipped, report.failed), (1, 1, 0));
        assert!(trash.join("files/old").exists() && base.join("edited").exists());
    }
}
