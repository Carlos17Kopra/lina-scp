use linascp_core::error::{CoreError, Result};
use linascp_core::model::{Group, Host, HostSource, Id};
use linascp_core::secrets::SecretOwner;
use linascp_core::validate::{validate_host, validate_terminal_overrides};
use tauri::State;

use super::secrets::purge_secrets;
use super::settings::sync_mirror;
use crate::state::AppState;

/// Saves (upserts) a host; editing an ssh-config host turns it into an overriding LinaSCP host.
pub(crate) fn save_host(state: &AppState, mut host: Host) -> Result<Host> {
    validate_host(&host)?;
    validate_terminal_overrides(&host)?;
    host.source = HostSource::Linascp;
    state.store.save_host(&host)?;
    let saved = state
        .store
        .get_host(host.id)?
        .ok_or_else(|| CoreError::internal(format!("host {} vanished after save", host.id)))?;
    sync_mirror(state)?;
    Ok(saved)
}

pub(crate) async fn delete_host(state: &AppState, id: Id) -> Result<()> {
    state.store.delete_host(id)?;
    sync_mirror(state)?;
    purge_secrets(state.secrets.clone(), SecretOwner::Host(id)).await;
    Ok(())
}

pub(crate) fn save_group(state: &AppState, group: Group) -> Result<Group> {
    state.store.save_group(&group)?;
    Ok(group)
}

#[tauri::command]
#[specta::specta]
pub async fn hosts_list(state: State<'_, AppState>) -> Result<Vec<Host>> {
    state.merged_hosts()
}

#[tauri::command]
#[specta::specta]
pub async fn host_save(state: State<'_, AppState>, host: Host) -> Result<Host> {
    save_host(&state, host)
}

#[tauri::command]
#[specta::specta]
pub async fn host_delete(state: State<'_, AppState>, id: Id) -> Result<()> {
    delete_host(&state, id).await
}

#[tauri::command]
#[specta::specta]
pub async fn groups_list(state: State<'_, AppState>) -> Result<Vec<Group>> {
    state.store.list_groups()
}

#[tauri::command]
#[specta::specta]
pub async fn group_save(state: State<'_, AppState>, group: Group) -> Result<Group> {
    save_group(&state, group)
}

#[tauri::command]
#[specta::specta]
pub async fn group_delete(state: State<'_, AppState>, id: Id) -> Result<()> {
    state.store.delete_group(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::settings::save_settings;
    use crate::dto::{Settings, SshMode};
    use linascp_core::secrets::{SecretKind, SecretOwner};
    use secrecy::SecretString;

    fn setup() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        (dir, state)
    }

    #[test]
    fn merged_list_contains_stored_and_ssh_config_hosts() {
        let (_dir, state) = setup();
        std::fs::write(state.paths.ssh_config(), "Host from-config\n  HostName 192.0.2.10\n  User root\n").unwrap();
        save_host(&state, Host::new("stored-01", "192.0.2.20")).unwrap();

        let hosts = state.merged_hosts().unwrap();
        let config_host = hosts.iter().find(|h| h.name == "from-config").expect("ssh-config host");
        assert!(matches!(config_host.source, HostSource::SshConfig));
        assert!(hosts.iter().any(|h| h.name == "stored-01"));
    }

    #[test]
    fn merged_list_works_without_ssh_config() {
        let (_dir, state) = setup();
        save_host(&state, Host::new("solo", "192.0.2.30")).unwrap();
        let names: Vec<String> = state.merged_hosts().unwrap().into_iter().map(|h| h.name).collect();
        assert_eq!(names, vec!["solo".to_string()]);
    }

    /// F5: fields that would break out of their ssh_config line are rejected before anything is stored.
    #[test]
    fn save_rejects_newlines_and_quotes() {
        let (_dir, state) = setup();
        let mut newline = Host::new("evil", "192.0.2.41");
        newline.user = Some("root\nProxyCommand touch /tmp/pwned".into());
        assert!(matches!(save_host(&state, newline.clone()), Err(CoreError::Config { .. })));
        let quote = Host::new("evil\"", "192.0.2.42");
        assert!(matches!(save_host(&state, quote.clone()), Err(CoreError::Config { .. })));
        let mut port = Host::new("port-zero", "192.0.2.43");
        port.port = 0;
        assert!(matches!(save_host(&state, port), Err(CoreError::Config { .. })));
        let mut font = Host::new("font", "192.0.2.44");
        font.terminal_font_size = Some(99);
        assert!(matches!(save_host(&state, font), Err(CoreError::Config { .. })));
        let mut scheme = Host::new("scheme", "192.0.2.45");
        scheme.terminal_scheme = Some("../x".into());
        assert!(matches!(save_host(&state, scheme), Err(CoreError::Config { .. })));
        assert!(state.store.list_hosts().unwrap().is_empty());
    }

    #[test]
    fn save_forces_linascp_source() {
        let (_dir, state) = setup();
        let mut host = Host::new("override", "192.0.2.40");
        host.source = HostSource::SshConfig;
        let saved = save_host(&state, host).unwrap();
        assert!(matches!(saved.source, HostSource::Linascp));
        assert!(state.store.get_host(saved.id).unwrap().is_some());
    }

    #[test]
    fn save_in_mirror_mode_rewrites_mirror_file() {
        let (_dir, state) = setup();
        save_settings(&state, Settings { ssh_mode: SshMode::Mirror, ..Settings::default() }).unwrap();
        save_host(&state, Host::new("mirrored-01", "192.0.2.50")).unwrap();
        let content = std::fs::read_to_string(state.paths.ssh_dir.join("config.d").join("linascp.conf")).unwrap();
        assert!(content.contains("mirrored-01"));
    }

    #[tokio::test]
    async fn delete_removes_host_and_its_secrets() {
        let (_dir, state) = setup();
        let saved = save_host(&state, Host::new("gone", "192.0.2.60")).unwrap();
        state
            .secrets
            .set(SecretOwner::Host(saved.id), SecretKind::Password, &SecretString::from("pw".to_string()))
            .unwrap();

        delete_host(&state, saved.id).await.unwrap();

        assert!(state.store.get_host(saved.id).unwrap().is_none());
        assert!(state.secrets.get(SecretOwner::Host(saved.id), SecretKind::Password).unwrap().is_none());
    }

    #[test]
    fn groups_roundtrip() {
        let (_dir, state) = setup();
        let group = Group::new("Produktion", 1);
        save_group(&state, group.clone()).unwrap();
        assert_eq!(state.store.list_groups().unwrap().len(), 1);
        state.store.delete_group(group.id).unwrap();
        assert!(state.store.list_groups().unwrap().is_empty());
    }
}
