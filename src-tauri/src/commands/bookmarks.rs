use linascp_core::error::Result;
use linascp_core::model::{Bookmark, Id};
use tauri::State;

use crate::state::AppState;

pub(crate) fn list(state: &AppState, host_id: Option<Id>) -> Result<Vec<Bookmark>> {
    state.store.list_bookmarks(host_id)
}

pub(crate) fn add(state: &AppState, host_id: Option<Id>, path: String) -> Result<Bookmark> {
    state.store.add_bookmark(host_id, &path)
}

pub(crate) fn remove(state: &AppState, id: Id) -> Result<()> {
    state.store.remove_bookmark(id)
}

/// `host_id`: `None` lists the local bookmarks.
#[tauri::command]
#[specta::specta]
pub async fn bookmarks_list(state: State<'_, AppState>, host_id: Option<Id>) -> Result<Vec<Bookmark>> {
    list(&state, host_id)
}

#[tauri::command]
#[specta::specta]
pub async fn bookmark_add(state: State<'_, AppState>, host_id: Option<Id>, path: String) -> Result<Bookmark> {
    add(&state, host_id, path)
}

#[tauri::command]
#[specta::specta]
pub async fn bookmark_remove(state: State<'_, AppState>, id: Id) -> Result<()> {
    remove(&state, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use linascp_core::error::CoreError;

    #[test]
    fn bookmark_commands_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let host = uuid::Uuid::new_v4();
        let local = add(&state, None, "/home/u".into()).unwrap();
        add(&state, Some(host), "/srv".into()).unwrap();
        assert_eq!(list(&state, None).unwrap(), vec![local.clone()]);
        assert_eq!(list(&state, Some(host)).unwrap()[0].path, "/srv");
        remove(&state, local.id).unwrap();
        assert!(list(&state, None).unwrap().is_empty());
    }

    #[test]
    fn bookmark_add_rejects_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        assert!(matches!(add(&state, None, "srv".into()), Err(CoreError::Config { .. })));
    }
}
