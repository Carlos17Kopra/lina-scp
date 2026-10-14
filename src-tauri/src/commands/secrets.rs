use std::sync::Arc;

use linascp_core::error::Result;
use linascp_core::secrets::{SecretKind, SecretOwner, SecretStore};
use secrecy::SecretString;
use tauri::State;

use super::blocking;
use crate::state::AppState;

/// Keyring access is blocking D-Bus IO, so it always runs on the blocking pool.
pub(crate) async fn set_secret(
    secrets: Arc<dyn SecretStore>,
    owner: SecretOwner,
    kind: SecretKind,
    value: String,
) -> Result<()> {
    blocking(move || secrets.set(owner, kind, &SecretString::from(value))).await
}

pub(crate) async fn delete_secret(secrets: Arc<dyn SecretStore>, owner: SecretOwner, kind: SecretKind) -> Result<()> {
    blocking(move || secrets.delete(owner, kind)).await
}

/// Best-effort removal of all secrets of a deleted host/profile.
pub(crate) async fn purge_secrets(secrets: Arc<dyn SecretStore>, owner: SecretOwner) {
    let result = blocking(move || {
        for kind in [SecretKind::Password, SecretKind::Passphrase] {
            secrets.delete(owner, kind)?;
        }
        Ok(())
    })
    .await;
    if let Err(error) = result {
        tracing::warn!(%error, ?owner, "failed to purge secrets");
    }
}

#[tauri::command]
#[specta::specta]
pub async fn secret_set(state: State<'_, AppState>, owner: SecretOwner, kind: SecretKind, value: String) -> Result<()> {
    set_secret(state.secrets.clone(), owner, kind, value).await
}

#[tauri::command]
#[specta::specta]
pub async fn secret_delete(state: State<'_, AppState>, owner: SecretOwner, kind: SecretKind) -> Result<()> {
    delete_secret(state.secrets.clone(), owner, kind).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use linascp_core::secrets::SecretOwner;
    use secrecy::ExposeSecret;

    fn read(state: &AppState, id: uuid::Uuid, kind: SecretKind) -> Option<String> {
        state.secrets.get(SecretOwner::Host(id), kind).unwrap().map(|s| s.expose_secret().to_string())
    }

    #[tokio::test]
    async fn set_delete_and_purge() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let id = uuid::Uuid::new_v4();
        let owner = SecretOwner::Host(id);

        set_secret(state.secrets.clone(), owner, SecretKind::Password, "pw-1".into()).await.unwrap();
        set_secret(state.secrets.clone(), owner, SecretKind::Passphrase, "pp-1".into()).await.unwrap();
        assert_eq!(read(&state, id, SecretKind::Password).as_deref(), Some("pw-1"));

        delete_secret(state.secrets.clone(), owner, SecretKind::Password).await.unwrap();
        assert_eq!(read(&state, id, SecretKind::Password), None);
        assert_eq!(read(&state, id, SecretKind::Passphrase).as_deref(), Some("pp-1"));

        purge_secrets(state.secrets.clone(), owner).await;
        assert_eq!(read(&state, id, SecretKind::Passphrase), None);
    }
}
