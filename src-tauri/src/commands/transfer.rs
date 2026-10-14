use std::collections::HashMap;
use std::fs::{OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use linascp_core::error::{CoreError, Result};
use linascp_core::export::{self as core_export, ExportOptions, ExportReport, ImportPreview, ImportReport, Resolution};
use linascp_core::model::{Host, Id};
use secrecy::SecretString;
use tauri::State;

use super::blocking;
use super::settings::sync_mirror;
use crate::state::AppState;

/// Writes `bytes` with mode 0600 (also tightening an existing file).
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.set_permissions(Permissions::from_mode(0o600))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(crate) async fn export_to(
    state: &AppState,
    path: PathBuf,
    include_secrets: bool,
    include_key_files: bool,
    passphrase: Option<String>,
) -> Result<ExportReport> {
    let store = state.store.clone();
    let secrets = state.secrets.clone();
    let home = state.paths.home.clone();
    blocking(move || {
        let options =
            ExportOptions { include_secrets, include_key_files, passphrase: passphrase.map(SecretString::from), home };
        let output = core_export::export_data(&store, secrets.as_ref(), &options)?;
        write_private(&path, &output.bytes)?;
        Ok(output.report)
    })
    .await
}

pub(crate) async fn preview_from(state: &AppState, path: PathBuf) -> Result<ImportPreview> {
    let store = state.store.clone();
    blocking(move || {
        let bytes = std::fs::read(&path)?;
        core_export::import_preview(&bytes, &store)
    })
    .await
}

pub(crate) async fn apply_from(
    state: &AppState,
    path: PathBuf,
    passphrase: Option<String>,
    resolutions: HashMap<Id, Resolution>,
) -> Result<ImportReport> {
    let store = state.store.clone();
    let secrets = state.secrets.clone();
    let key_dir = state.paths.imported_keys_dir();
    let report = blocking(move || {
        let bytes = std::fs::read(&path)?;
        let passphrase = passphrase.map(SecretString::from);
        core_export::import_apply(&bytes, passphrase.as_ref(), &resolutions, &store, secrets.as_ref(), &key_dir)
    })
    .await?;
    sync_mirror(state)?;
    Ok(report)
}

fn non_empty_path(path: String) -> Result<PathBuf> {
    if path.trim().is_empty() {
        return Err(CoreError::config("no file selected"));
    }
    Ok(PathBuf::from(path))
}

#[tauri::command]
#[specta::specta]
pub async fn export_file(
    state: State<'_, AppState>,
    path: String,
    include_secrets: bool,
    include_key_files: bool,
    passphrase: Option<String>,
) -> Result<ExportReport> {
    export_to(&state, non_empty_path(path)?, include_secrets, include_key_files, passphrase).await
}

#[tauri::command]
#[specta::specta]
pub async fn import_preview(state: State<'_, AppState>, path: String) -> Result<ImportPreview> {
    preview_from(&state, non_empty_path(path)?).await
}

#[tauri::command]
#[specta::specta]
pub async fn import_apply(
    state: State<'_, AppState>,
    path: String,
    passphrase: Option<String>,
    resolutions: HashMap<Id, Resolution>,
) -> Result<ImportReport> {
    apply_from(&state, non_empty_path(path)?, passphrase, resolutions).await
}

/// Re-reads `~/.ssh/config` (it is read on every list call; this is the explicit "Neu einlesen").
#[tauri::command]
#[specta::specta]
pub async fn ssh_config_reload(state: State<'_, AppState>) -> Result<Vec<Host>> {
    state.merged_hosts()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::hosts::save_host;
    use linascp_core::export::ItemKind;
    use linascp_core::model::{AuthConfig, AuthMethod, HostAuth};
    use linascp_core::secrets::{SecretKind, SecretOwner};
    use secrecy::ExposeSecret;
    use std::os::unix::fs::PermissionsExt;

    fn state_in(dir: &tempfile::TempDir, name: &str) -> AppState {
        AppState::for_tests(&dir.path().join(name))
    }

    #[tokio::test]
    async fn export_writes_a_private_file_that_previews_and_imports() {
        let dir = tempfile::tempdir().unwrap();
        let source = state_in(&dir, "a");
        let host = save_host(&source, Host::new("web-01", "192.0.2.80")).unwrap();
        let file = dir.path().join("hosts.linascp");

        export_to(&source, file.clone(), false, false, None).await.unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let target = state_in(&dir, "b");
        let preview = preview_from(&target, file.clone()).await.unwrap();
        let item = preview.items.iter().find(|i| i.id == host.id).expect("host in preview");
        assert!(matches!(item.kind, ItemKind::Host));
        assert!(item.conflict.is_none());
        assert!(!preview.has_secrets);

        let report = apply_from(&target, file, None, HashMap::new()).await.unwrap();
        assert!(report.imported >= 1);
        assert!(target.store.get_host(host.id).unwrap().is_some());
    }

    #[tokio::test]
    async fn secrets_travel_encrypted_with_the_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let source = state_in(&dir, "a");
        let mut host = Host::new("db-01", "192.0.2.81");
        let mut auth = AuthConfig::new(AuthMethod::Password);
        auth.has_secret = true;
        host.auth = HostAuth::Custom { auth };
        let host = save_host(&source, host).unwrap();
        source
            .secrets
            .set(SecretOwner::Host(host.id), SecretKind::Password, &SecretString::from("s3cret".to_string()))
            .unwrap();
        let file = dir.path().join("secret.linascp");
        export_to(&source, file.clone(), true, false, Some("correct horse".into())).await.unwrap();

        let target = state_in(&dir, "b");
        assert!(preview_from(&target, file.clone()).await.unwrap().has_secrets);
        let report = apply_from(&target, file, Some("correct horse".into()), HashMap::new()).await.unwrap();
        assert_eq!(report.secrets_restored, 1);
        let restored = target.secrets.get(SecretOwner::Host(host.id), SecretKind::Password).unwrap().unwrap();
        assert_eq!(restored.expose_secret(), "s3cret");
    }

    #[tokio::test]
    async fn previewing_a_missing_file_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = state_in(&dir, "b");
        let error = preview_from(&target, dir.path().join("missing.linascp")).await.unwrap_err();
        assert!(matches!(error, CoreError::Io { .. }));
    }

    #[test]
    fn write_private_tightens_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("existing.linascp");
        std::fs::write(&file, b"old").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&file, b"new").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
