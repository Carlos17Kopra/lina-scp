use linascp_core::error::{CoreError, Result};
use linascp_core::model::{Id, LoginProfile};
use tauri::State;

use linascp_core::secrets::SecretOwner;
use linascp_core::validate::validate_profile;

use super::secrets::purge_secrets;
use super::settings::sync_mirror;
use crate::dto::ProfileWithUsage;
use crate::state::AppState;

pub(crate) fn list_profiles(state: &AppState) -> Result<Vec<ProfileWithUsage>> {
    state
        .store
        .list_profiles()?
        .into_iter()
        .map(|profile| Ok(ProfileWithUsage { used_by: state.store.profile_usage(profile.id)?, profile }))
        .collect()
}

pub(crate) fn save_profile(state: &AppState, profile: LoginProfile) -> Result<LoginProfile> {
    validate_profile(&profile)?;
    state.store.save_profile(&profile)?;
    let saved = state
        .store
        .get_profile(profile.id)?
        .ok_or_else(|| CoreError::internal(format!("profile {} vanished after save", profile.id)))?;
    sync_mirror(state)?;
    Ok(saved)
}

/// Fails with `CoreError::Store` while hosts still use the profile (core rule).
pub(crate) async fn delete_profile(state: &AppState, id: Id) -> Result<()> {
    state.store.delete_profile(id)?;
    sync_mirror(state)?;
    purge_secrets(state.secrets.clone(), SecretOwner::Profile(id)).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn profiles_list(state: State<'_, AppState>) -> Result<Vec<ProfileWithUsage>> {
    list_profiles(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn profile_save(state: State<'_, AppState>, profile: LoginProfile) -> Result<LoginProfile> {
    save_profile(&state, profile)
}

#[tauri::command]
#[specta::specta]
pub async fn profile_delete(state: State<'_, AppState>, id: Id) -> Result<()> {
    delete_profile(&state, id).await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::commands::hosts::save_host;
    use linascp_core::model::{AuthConfig, AuthMethod, Host, HostAuth};

    /// Test fixture, also used by the connect tests (Task 14).
    pub(crate) fn profile(name: &str) -> LoginProfile {
        let mut auth = AuthConfig::new(AuthMethod::Key);
        auth.key_path = Some("~/.ssh/id_ed25519".into());
        LoginProfile::new(name, "deploy", auth)
    }

    /// F5: a profile's user/key path end up in the mirror file and must not contain line breaks or quotes.
    #[test]
    fn save_rejects_newlines_and_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let mut bad_user = profile("bad-user");
        bad_user.user = "deploy\nProxyCommand touch /tmp/pwned".into();
        assert!(matches!(save_profile(&state, bad_user), Err(CoreError::Config { .. })));
        let mut bad_key = profile("bad-key");
        bad_key.auth.key_path = Some("/keys/\"id".into());
        assert!(matches!(save_profile(&state, bad_key), Err(CoreError::Config { .. })));
        assert!(state.store.list_profiles().unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_reports_usage_and_delete_in_use_fails() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let p = save_profile(&state, profile("deploy")).unwrap();
        let mut host = Host::new("web-01", "192.0.2.70");
        host.auth = HostAuth::Profile { profile_id: p.id };
        save_host(&state, host.clone()).unwrap();

        let listed = list_profiles(&state).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].profile.id, p.id);
        assert_eq!(listed[0].used_by, 1);

        assert!(matches!(delete_profile(&state, p.id).await, Err(CoreError::Store { .. })));

        state.store.delete_host(host.id).unwrap();
        delete_profile(&state, p.id).await.unwrap();
        assert!(list_profiles(&state).unwrap().is_empty());
    }
}
