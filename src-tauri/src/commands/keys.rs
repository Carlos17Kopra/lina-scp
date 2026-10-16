//! Schlüsselbund (TP5b): the user's SSH key files and ssh-copy-id.

use linascp_core::connect::{ConnectEvent, EmitFn, copy_public_key};
use linascp_core::error::{CoreError, Result};
use linascp_core::keys::{KeyDefaults, KeyInfo, KeyKind, generate_key, key_defaults, list_keys};
use linascp_core::model::Id;
use secrecy::SecretString;
use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use super::blocking;
use super::connect::{build_request, channel_emit, connect_deps, fail, find_host, register_connect};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn keys_list(state: State<'_, AppState>) -> Result<Vec<KeyInfo>> {
    let dir = state.paths.ssh_dir.clone();
    blocking(move || list_keys(&dir)).await
}

#[tauri::command]
#[specta::specta]
pub async fn key_defaults_get(state: State<'_, AppState>, kind: KeyKind) -> Result<KeyDefaults> {
    Ok(key_defaults(&state.paths.ssh_dir, kind))
}

pub(crate) async fn generate_inner(
    state: &AppState,
    kind: KeyKind,
    name: String,
    comment: String,
    passphrase: Option<String>,
) -> Result<KeyInfo> {
    let dir = state.paths.ssh_dir.clone();
    let passphrase = passphrase.map(SecretString::from);
    blocking(move || generate_key(&dir, kind, &name, &comment, passphrase.as_ref())).await
}

/// Writes `~/.ssh/<name>` and `<name>.pub`; never overwrites.
#[tauri::command]
#[specta::specta]
pub async fn key_generate(
    state: State<'_, AppState>,
    kind: KeyKind,
    name: String,
    comment: String,
    passphrase: Option<String>,
) -> Result<KeyInfo> {
    generate_inner(&state, kind, name, comment, passphrase).await
}

pub(crate) async fn copy_id_inner(
    state: &AppState,
    connect_id: Uuid,
    host_id: Id,
    public_key: String,
    emit: EmitFn,
) -> Result<bool> {
    let guard = register_connect(state, connect_id);
    let request = match find_host(state, host_id).and_then(|host| build_request(state, host, 80, 24)) {
        Ok(request) => request,
        Err(error) => return fail(&emit, error),
    };
    if guard.is_cancelled() {
        return fail(&emit, CoreError::Cancelled);
    }
    let cancel = guard.token();
    // core emits `Failed` on errors; success is the resolved value.
    copy_public_key(request, connect_deps(state), state.broker.clone(), emit, cancel, &public_key).await
}

/// ssh-copy-id: `true` = added, `false` = it was there already. Prompts arrive on `events`; cancel with
/// `connect_cancel(connect_id)`.
#[tauri::command]
#[specta::specta]
pub async fn key_copy_id(
    state: State<'_, AppState>,
    connect_id: Uuid,
    host_id: Id,
    public_key: String,
    events: Channel<ConnectEvent>,
) -> Result<bool> {
    copy_id_inner(&state, connect_id, host_id, public_key, channel_emit(events)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn generated_keys_are_listed_and_an_unknown_host_fails_once() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let key =
            generate_inner(&state, KeyKind::Ed25519, "id_x".into(), "c".into(), Some(String::new())).await.unwrap();
        assert_eq!(key.encrypted, Some(false), "an empty passphrase means none");
        let listed = list_keys(&state.paths.ssh_dir).unwrap();
        assert_eq!(listed, vec![key.clone()]);

        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let emit: EmitFn = Arc::new(move |e| seen.lock().unwrap().push(e));
        let result = copy_id_inner(&state, Uuid::new_v4(), Uuid::new_v4(), key.public_key, emit).await;
        assert!(matches!(result, Err(CoreError::Config { .. })));
        let failed = events.lock().unwrap().iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count();
        assert_eq!(failed, 1);
    }
}
