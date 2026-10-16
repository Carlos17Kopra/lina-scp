//! Editing files from both SFTP panes.

use std::path::Path;

use linascp_core::edit::{EditId, EditManager, Launcher};
use linascp_core::error::{CoreError, Result};
use linascp_core::session::SessionId;
use tauri::State;

use super::sftp_transfers::entry;
use crate::dto::{OpenApp, OpenWithApps};
use crate::editor::{SystemLauncher, apps_for, resolve_app};
use crate::state::AppState;

pub(crate) fn with_edits<T>(state: &AppState, id: SessionId, f: impl FnOnce(&EditManager) -> Result<T>) -> Result<T> {
    f(&entry(state, id)?.edits)
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or_default()
}

pub(crate) async fn open(state: &AppState, id: SessionId, path: String, how: OpenApp) -> Result<EditId> {
    let edits = entry(state, id)?.edits.clone();
    let app = resolve_app(&state.store, file_name(&path), &how)?;
    edits.open_with(&path, app).await
}

pub(crate) async fn stop(state: &AppState, id: SessionId, edit_id: EditId) -> Result<Option<String>> {
    let edits = entry(state, id)?.edits.clone();
    Ok(edits.stop(edit_id).await?.map(|p| p.to_string_lossy().into_owned()))
}

/// A local file opens in place: no copy, no watch.
pub(crate) fn local_edit_inner(state: &AppState, path: String, how: OpenApp) -> Result<()> {
    let app = resolve_app(&state.store, file_name(&path), &how)?;
    let path = Path::new(&path);
    if !path.is_absolute() {
        return Err(CoreError::config("path must be absolute"));
    }
    if !std::fs::metadata(path)?.is_file() {
        return Err(CoreError::fs(linascp_core::error::FsKind::Other, path.to_string_lossy(), "not a file"));
    }
    SystemLauncher::new(state.store.clone()).open(path, app.as_deref())
}

/// Downloads a remote file, opens it in the editor and uploads every save.
#[tauri::command]
#[specta::specta]
pub async fn edit_open(
    state: State<'_, AppState>,
    session_id: SessionId,
    path: String,
    how: OpenApp,
) -> Result<EditId> {
    open(&state, session_id, path, how).await
}

#[tauri::command]
#[specta::specta]
pub async fn local_edit(state: State<'_, AppState>, path: String, how: OpenApp) -> Result<()> {
    local_edit_inner(&state, path, how)
}

/// The apps offered by "Öffnen mit" for a file name.
#[tauri::command]
#[specta::specta]
pub async fn open_with_apps(state: State<'_, AppState>, name: String) -> Result<OpenWithApps> {
    apps_for(&state.store, &name)
}

#[tauri::command]
#[specta::specta]
pub async fn edit_resolve(
    state: State<'_, AppState>,
    session_id: SessionId,
    edit_id: EditId,
    overwrite: bool,
) -> Result<()> {
    with_edits(&state, session_id, |e| e.resolve(edit_id, overwrite))
}

#[tauri::command]
#[specta::specta]
pub async fn edit_reopen(state: State<'_, AppState>, session_id: SessionId, edit_id: EditId) -> Result<()> {
    with_edits(&state, session_id, |e| e.reopen(edit_id))
}

/// Returns the path of a copy that was kept because it holds changes the server does not have.
#[tauri::command]
#[specta::specta]
pub async fn edit_stop(state: State<'_, AppState>, session_id: SessionId, edit_id: EditId) -> Result<Option<String>> {
    stop(&state, session_id, edit_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_sessions_and_edit_ids_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let id = uuid::Uuid::new_v4();
        assert!(open(&state, id, "/etc/hosts".into(), OpenApp::Editor).await.is_err());
        assert!(with_edits(&state, id, |e| e.resolve(1, true)).is_err());
        assert!(with_edits(&state, id, |e| e.reopen(1)).is_err());
        assert!(stop(&state, id, 1).await.is_err());
    }

    #[test]
    fn local_edit_rejects_relative_and_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        assert!(matches!(local_edit_inner(&state, "rel.txt".into(), OpenApp::Editor), Err(CoreError::Config { .. })));
        let missing = dir.path().join("missing.txt");
        assert!(local_edit_inner(&state, missing.to_str().unwrap().into(), OpenApp::Editor).is_err());
        assert!(local_edit_inner(&state, dir.path().to_str().unwrap().into(), OpenApp::Editor).is_err(), "a directory");
    }
}
