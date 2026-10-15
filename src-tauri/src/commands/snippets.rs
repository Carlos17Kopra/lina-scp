use linascp_core::error::{CoreError, Result};
use linascp_core::model::{Id, Snippet};
use linascp_core::validate::validate_snippet;
use tauri::State;

use crate::state::AppState;

/// Validates and upserts; returns the stored snippet (with its timestamps).
pub(crate) fn save_snippet(state: &AppState, snippet: Snippet) -> Result<Snippet> {
    validate_snippet(&snippet)?;
    state.store.save_snippet(&snippet)?;
    state
        .store
        .get_snippet(snippet.id)?
        .ok_or_else(|| CoreError::internal(format!("snippet {} vanished after save", snippet.id)))
}

#[tauri::command]
#[specta::specta]
pub async fn snippets_list(state: State<'_, AppState>) -> Result<Vec<Snippet>> {
    state.store.list_snippets()
}

#[tauri::command]
#[specta::specta]
pub async fn snippet_save(state: State<'_, AppState>, snippet: Snippet) -> Result<Snippet> {
    save_snippet(&state, snippet)
}

#[tauri::command]
#[specta::specta]
pub async fn snippet_delete(state: State<'_, AppState>, id: Id) -> Result<()> {
    state.store.delete_snippet(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use linascp_core::model::Host;

    #[test]
    fn save_validates_and_returns_the_stored_snippet() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        assert!(matches!(save_snippet(&state, Snippet::new(" ", "ls", None)), Err(CoreError::Config { .. })));

        let host = Host::new("web", "192.0.2.20");
        state.store.save_host(&host).unwrap();
        let mut snippet = Snippet::new("Logs", "journalctl -f", Some(host.id));
        snippet.updated_at = 0;
        let saved = save_snippet(&state, snippet.clone()).unwrap();

        assert_eq!((saved.id, saved.host_id), (snippet.id, Some(host.id)));
        assert!(saved.updated_at > 0);
        assert_eq!(state.store.list_snippets().unwrap(), vec![saved]);
    }
}
