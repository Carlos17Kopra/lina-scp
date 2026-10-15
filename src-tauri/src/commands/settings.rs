use linascp_core::error::{CoreError, Result};
use linascp_core::sshconfig::Mirror;
use linascp_core::store::{
    FIRST_RUN_DONE, SSH_MODE, Store, TERMINAL_AUTO_RECONNECT, TERMINAL_FONT_SIZE, TERMINAL_RENDERER, TERMINAL_SCHEME,
    TERMINAL_SIDEBAR_OPEN, THEME,
};
use linascp_core::validate::{TERMINAL_FONT_SIZES, is_valid_terminal_scheme};
use tauri::State;

use crate::dto::{Settings, SshMode, TerminalRenderer, Theme};
use crate::state::AppState;

/// Store key of the WebKit compatibility switch (`"0"` / `"1"`); not part of core's key list.
pub const COMPAT_RENDERING: &str = "compat_rendering";

fn parse_flag(value: Option<String>) -> Option<bool> {
    value.map(|v| matches!(v.as_str(), "1" | "true"))
}

fn flag_str(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

/// Reads all settings; missing or invalid values fall back to the defaults.
pub(crate) fn load_settings(store: &Store) -> Result<Settings> {
    let d = Settings::default();
    Ok(Settings {
        theme: store.get_setting(THEME)?.as_deref().and_then(Theme::parse).unwrap_or(d.theme),
        ssh_mode: store.get_setting(SSH_MODE)?.as_deref().and_then(SshMode::parse).unwrap_or(d.ssh_mode),
        terminal_renderer: store
            .get_setting(TERMINAL_RENDERER)?
            .as_deref()
            .and_then(TerminalRenderer::parse)
            .unwrap_or(d.terminal_renderer),
        terminal_font_size: store
            .get_setting(TERMINAL_FONT_SIZE)?
            .and_then(|v| v.parse::<u16>().ok())
            .filter(|size| TERMINAL_FONT_SIZES.contains(size))
            .unwrap_or(d.terminal_font_size),
        terminal_scheme: store
            .get_setting(TERMINAL_SCHEME)?
            .filter(|id| is_valid_terminal_scheme(id))
            .unwrap_or(d.terminal_scheme),
        terminal_sidebar_open: parse_flag(store.get_setting(TERMINAL_SIDEBAR_OPEN)?).unwrap_or(d.terminal_sidebar_open),
        terminal_auto_reconnect: parse_flag(store.get_setting(TERMINAL_AUTO_RECONNECT)?)
            .unwrap_or(d.terminal_auto_reconnect),
        first_run_done: parse_flag(store.get_setting(FIRST_RUN_DONE)?).unwrap_or(d.first_run_done),
        compat_rendering: parse_flag(store.get_setting(COMPAT_RENDERING)?).unwrap_or(d.compat_rendering),
    })
}

/// Read when a connection is lost; a store error keeps the default (on).
pub(crate) fn auto_reconnect_enabled(store: &Store) -> bool {
    load_settings(store).map(|settings| settings.terminal_auto_reconnect).unwrap_or(true)
}

fn mirror(state: &AppState) -> Mirror {
    Mirror::new(state.paths.ssh_dir.clone())
}

/// The mirror gets the merged list so `ProxyJump` to ssh-config hosts resolves;
/// `Mirror::render` itself only writes `source = Linascp` hosts.
fn apply_ssh_mode(state: &AppState, mode: SshMode) -> Result<()> {
    match mode {
        SshMode::Mirror => mirror(state).enable(&state.merged_hosts()?, &state.store.list_profiles()?),
        SshMode::Integrate => mirror(state).disable(),
    }
}

/// Persists the settings; enables/disables the ssh-config mirror when `ssh_mode` changes.
pub(crate) fn save_settings(state: &AppState, next: Settings) -> Result<Settings> {
    if !TERMINAL_FONT_SIZES.contains(&next.terminal_font_size) {
        return Err(CoreError::Config {
            message: format!("terminal font size must be between 10 and 20, got {}", next.terminal_font_size),
        });
    }
    if !is_valid_terminal_scheme(&next.terminal_scheme) {
        return Err(CoreError::Config { message: "invalid terminal color scheme id".to_string() });
    }
    let previous = load_settings(&state.store)?;
    if previous.ssh_mode != next.ssh_mode {
        apply_ssh_mode(state, next.ssh_mode)?;
    }
    let store = &state.store;
    store.set_setting(THEME, next.theme.as_str())?;
    store.set_setting(SSH_MODE, next.ssh_mode.as_str())?;
    store.set_setting(TERMINAL_RENDERER, next.terminal_renderer.as_str())?;
    store.set_setting(TERMINAL_FONT_SIZE, &next.terminal_font_size.to_string())?;
    store.set_setting(TERMINAL_SCHEME, &next.terminal_scheme)?;
    store.set_setting(TERMINAL_SIDEBAR_OPEN, flag_str(next.terminal_sidebar_open))?;
    store.set_setting(TERMINAL_AUTO_RECONNECT, flag_str(next.terminal_auto_reconnect))?;
    store.set_setting(FIRST_RUN_DONE, flag_str(next.first_run_done))?;
    store.set_setting(COMPAT_RENDERING, flag_str(next.compat_rendering))?;
    load_settings(store)
}

/// Regenerates `~/.ssh/config.d/linascp.conf` when mirror mode is active.
pub(crate) fn sync_mirror(state: &AppState) -> Result<()> {
    if load_settings(&state.store)?.ssh_mode == SshMode::Mirror {
        mirror(state).write(&state.merged_hosts()?, &state.store.list_profiles()?)?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings> {
    load_settings(&state.store)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_set(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    save_settings(&state, settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use linascp_core::model::Host;

    fn setup() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        (dir, state)
    }

    fn mirror_file(state: &AppState) -> std::path::PathBuf {
        state.paths.ssh_dir.join("config.d").join("linascp.conf")
    }

    #[test]
    fn defaults_when_store_is_empty() {
        let (_dir, state) = setup();
        assert_eq!(load_settings(&state.store).unwrap(), Settings::default());
    }

    #[test]
    fn save_roundtrips_all_fields() {
        let (_dir, state) = setup();
        let next = Settings {
            theme: Theme::Light,
            ssh_mode: SshMode::Integrate,
            terminal_renderer: TerminalRenderer::Dom,
            terminal_font_size: 16,
            terminal_scheme: "nord".into(),
            terminal_sidebar_open: true,
            terminal_auto_reconnect: false,
            first_run_done: true,
            compat_rendering: true,
        };
        assert_eq!(save_settings(&state, next.clone()).unwrap(), next);
        assert_eq!(load_settings(&state.store).unwrap(), next);
        assert_eq!(state.store.get_setting(FIRST_RUN_DONE).unwrap().as_deref(), Some("1"));
        assert_eq!(state.store.get_setting(COMPAT_RENDERING).unwrap().as_deref(), Some("1"));
    }

    #[test]
    fn auto_reconnect_defaults_on_and_roundtrips() {
        let (_dir, state) = setup();
        assert!(load_settings(&state.store).unwrap().terminal_auto_reconnect);
        let next = Settings { terminal_auto_reconnect: false, ..load_settings(&state.store).unwrap() };
        assert!(!save_settings(&state, next).unwrap().terminal_auto_reconnect);
        assert!(!auto_reconnect_enabled(&state.store));
    }

    #[test]
    fn invalid_stored_values_fall_back_to_defaults() {
        let (_dir, state) = setup();
        state.store.set_setting(THEME, "purple").unwrap();
        state.store.set_setting(TERMINAL_FONT_SIZE, "300").unwrap();
        state.store.set_setting(TERMINAL_SCHEME, "../evil").unwrap();
        let s = load_settings(&state.store).unwrap();
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.terminal_font_size, 14);
        assert_eq!(s.terminal_scheme, "auto");
        state.store.set_setting(TERMINAL_FONT_SIZE, "9").unwrap();
        assert_eq!(load_settings(&state.store).unwrap().terminal_font_size, 14);
    }

    #[test]
    fn rejects_out_of_range_font_size() {
        let (_dir, state) = setup();
        let next = Settings { terminal_font_size: 4, ..Settings::default() };
        assert!(matches!(save_settings(&state, next), Err(CoreError::Config { .. })));
    }

    #[test]
    fn rejects_invalid_scheme_and_out_of_range_font_size() {
        let (_dir, state) = setup();
        let bad_scheme = Settings { terminal_scheme: "a b".into(), ..Settings::default() };
        assert!(matches!(save_settings(&state, bad_scheme), Err(CoreError::Config { .. })));
        let too_big = Settings { terminal_font_size: 21, ..Settings::default() };
        assert!(matches!(save_settings(&state, too_big), Err(CoreError::Config { .. })));
    }

    #[test]
    fn switching_ssh_mode_enables_and_disables_the_mirror() {
        let (_dir, state) = setup();
        let mirror_mode = Settings { ssh_mode: SshMode::Mirror, ..Settings::default() };
        save_settings(&state, mirror_mode).unwrap();
        assert!(Mirror::new(state.paths.ssh_dir.clone()).is_enabled());

        save_settings(&state, Settings::default()).unwrap();
        assert!(!Mirror::new(state.paths.ssh_dir.clone()).is_enabled());
    }

    #[test]
    fn sync_mirror_writes_only_in_mirror_mode() {
        let (_dir, state) = setup();
        state.store.save_host(&Host::new("alpha-01", "10.0.0.1")).unwrap();
        sync_mirror(&state).unwrap();
        assert!(!mirror_file(&state).exists());

        save_settings(&state, Settings { ssh_mode: SshMode::Mirror, ..Settings::default() }).unwrap();
        state.store.save_host(&Host::new("beta-02", "10.0.0.2")).unwrap();
        sync_mirror(&state).unwrap();
        let content = std::fs::read_to_string(mirror_file(&state)).unwrap();
        assert!(content.contains("alpha-01"));
        assert!(content.contains("beta-02"));
    }
}
