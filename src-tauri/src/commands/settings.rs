use linascp_core::error::{CoreError, Result};
use linascp_core::sshconfig::Mirror;
use linascp_core::store::{
    EDITOR_COMMAND, FIRST_RUN_DONE, LOCAL_SHELL, NEW_HOST_KEEPALIVE, NEW_HOST_PORT, NEW_HOST_TIMEOUT, NEW_HOST_USER,
    SSH_MODE, Store, TERMINAL_AUTO_RECONNECT, TERMINAL_CURSOR, TERMINAL_CURSOR_BLINK, TERMINAL_FONT_SIZE,
    TERMINAL_RENDERER, TERMINAL_SCHEME, TERMINAL_SCROLLBACK, TERMINAL_SIDEBAR_OPEN, THEME,
};
use linascp_core::validate::{
    TERMINAL_FONT_SIZES, TERMINAL_SCROLLBACKS, is_shell_path, is_valid_terminal_scheme, validate_editor_command,
    validate_local_shell, validate_user,
};
use tauri::State;

use crate::dto::{Settings, SshMode, TerminalCursor, TerminalRenderer, Theme};

const KEEPALIVE_MAX: u32 = 3600;
const TIMEOUT_MAX: u32 = 600;

/// A trimmed optional text; blank = `None`.
fn blank_none(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}
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
        auto_reconnect: parse_flag(store.get_setting(TERMINAL_AUTO_RECONNECT)?).unwrap_or(d.auto_reconnect),
        first_run_done: parse_flag(store.get_setting(FIRST_RUN_DONE)?).unwrap_or(d.first_run_done),
        compat_rendering: parse_flag(store.get_setting(COMPAT_RENDERING)?).unwrap_or(d.compat_rendering),
        editor_command: store.get_setting(EDITOR_COMMAND)?.filter(|c| validate_editor_command(c).is_ok()),
        terminal_scrollback: store
            .get_setting(TERMINAL_SCROLLBACK)?
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|lines| TERMINAL_SCROLLBACKS.contains(lines))
            .unwrap_or(d.terminal_scrollback),
        terminal_cursor: store
            .get_setting(TERMINAL_CURSOR)?
            .as_deref()
            .and_then(TerminalCursor::parse)
            .unwrap_or(d.terminal_cursor),
        terminal_cursor_blink: parse_flag(store.get_setting(TERMINAL_CURSOR_BLINK)?).unwrap_or(d.terminal_cursor_blink),
        // Only the form is checked here; whether the file runs is checked on save and when a terminal opens.
        local_shell: store.get_setting(LOCAL_SHELL)?.filter(|p| is_shell_path(p)),
        new_host_user: store.get_setting(NEW_HOST_USER)?.filter(|u| validate_user(u).is_ok()),
        new_host_port: store
            .get_setting(NEW_HOST_PORT)?
            .and_then(|v| v.parse::<u16>().ok())
            .filter(|port| *port != 0)
            .unwrap_or(d.new_host_port),
        new_host_keepalive_s: store
            .get_setting(NEW_HOST_KEEPALIVE)?
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|s| *s <= KEEPALIVE_MAX)
            .unwrap_or(d.new_host_keepalive_s),
        new_host_timeout_s: store
            .get_setting(NEW_HOST_TIMEOUT)?
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|s| *s <= TIMEOUT_MAX)
            .unwrap_or(d.new_host_timeout_s),
    })
}

/// Read when a connection is lost; a store error keeps the default (on).
pub(crate) fn auto_reconnect_enabled(store: &Store) -> bool {
    load_settings(store).map(|settings| settings.auto_reconnect).unwrap_or(true)
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
    let editor = next.editor_command.as_deref().map(str::trim).filter(|c| !c.is_empty()).unwrap_or_default();
    if !editor.is_empty() {
        validate_editor_command(editor)?;
    }
    if !TERMINAL_SCROLLBACKS.contains(&next.terminal_scrollback) {
        return Err(CoreError::config("Scrollback: 1 000, 5 000, 10 000, 50 000 oder 100 000 Zeilen"));
    }
    let shell = blank_none(next.local_shell.as_deref()).unwrap_or_default();
    if !shell.is_empty() {
        validate_local_shell(shell)?;
    }
    let user = blank_none(next.new_host_user.as_deref()).unwrap_or_default();
    if !user.is_empty() {
        validate_user(user)?;
    }
    if next.new_host_port == 0 {
        return Err(CoreError::config("Port muss zwischen 1 und 65535 liegen"));
    }
    if next.new_host_keepalive_s > KEEPALIVE_MAX {
        return Err(CoreError::config("Keepalive: höchstens 3600 s"));
    }
    if next.new_host_timeout_s > TIMEOUT_MAX {
        return Err(CoreError::config("Timeout: höchstens 600 s"));
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
    store.set_setting(TERMINAL_AUTO_RECONNECT, flag_str(next.auto_reconnect))?;
    store.set_setting(FIRST_RUN_DONE, flag_str(next.first_run_done))?;
    store.set_setting(COMPAT_RENDERING, flag_str(next.compat_rendering))?;
    store.set_setting(EDITOR_COMMAND, editor)?;
    store.set_setting(TERMINAL_SCROLLBACK, &next.terminal_scrollback.to_string())?;
    store.set_setting(TERMINAL_CURSOR, next.terminal_cursor.as_str())?;
    store.set_setting(TERMINAL_CURSOR_BLINK, flag_str(next.terminal_cursor_blink))?;
    store.set_setting(LOCAL_SHELL, shell)?;
    store.set_setting(NEW_HOST_USER, user)?;
    store.set_setting(NEW_HOST_PORT, &next.new_host_port.to_string())?;
    store.set_setting(NEW_HOST_KEEPALIVE, &next.new_host_keepalive_s.to_string())?;
    store.set_setting(NEW_HOST_TIMEOUT, &next.new_host_timeout_s.to_string())?;
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
    use crate::dto::TerminalCursor;
    use linascp_core::model::Host;
    use linascp_core::store::{
        LOCAL_SHELL, NEW_HOST_KEEPALIVE, NEW_HOST_PORT, NEW_HOST_TIMEOUT, NEW_HOST_USER, TERMINAL_CURSOR,
        TERMINAL_SCROLLBACK,
    };

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
            auto_reconnect: false,
            first_run_done: true,
            compat_rendering: true,
            editor_command: Some("code -w".into()),
            terminal_scrollback: 50_000,
            terminal_cursor: TerminalCursor::Bar,
            terminal_cursor_blink: false,
            local_shell: Some("/bin/sh".into()),
            new_host_user: Some("deploy".into()),
            new_host_port: 2222,
            new_host_keepalive_s: 0,
            new_host_timeout_s: 60,
        };
        assert_eq!(save_settings(&state, next.clone()).unwrap(), next);
        assert_eq!(load_settings(&state.store).unwrap(), next);
        assert_eq!(state.store.get_setting(FIRST_RUN_DONE).unwrap().as_deref(), Some("1"));
        assert_eq!(state.store.get_setting(COMPAT_RENDERING).unwrap().as_deref(), Some("1"));
    }

    #[test]
    fn auto_reconnect_defaults_on_and_roundtrips() {
        let (_dir, state) = setup();
        assert!(load_settings(&state.store).unwrap().auto_reconnect);
        let next = Settings { auto_reconnect: false, ..load_settings(&state.store).unwrap() };
        assert!(!save_settings(&state, next).unwrap().auto_reconnect);
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
    #[test]
    fn auto_reconnect_reads_the_old_key() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::state::AppState::for_tests(dir.path());
        state.store.set_setting(TERMINAL_AUTO_RECONNECT, "false").unwrap();
        assert!(!load_settings(&state.store).unwrap().auto_reconnect);
    }

    #[test]
    fn new_settings_have_defaults_and_fall_back_from_bad_stored_values() {
        let (_dir, state) = setup();
        let d = load_settings(&state.store).unwrap();
        assert_eq!(
            (d.terminal_scrollback, d.terminal_cursor, d.terminal_cursor_blink),
            (10_000, TerminalCursor::Block, true)
        );
        assert_eq!((d.local_shell, d.new_host_user), (None, None));
        assert_eq!((d.new_host_port, d.new_host_keepalive_s, d.new_host_timeout_s), (22, 30, 15));
        for (key, value) in [
            (TERMINAL_SCROLLBACK, "7"),
            (TERMINAL_CURSOR, "blob"),
            (LOCAL_SHELL, "relative/sh"),
            (NEW_HOST_USER, "a'b"),
            (NEW_HOST_PORT, "0"),
            (NEW_HOST_KEEPALIVE, "99999"),
            (NEW_HOST_TIMEOUT, "-1"),
        ] {
            state.store.set_setting(key, value).unwrap();
        }
        assert_eq!(load_settings(&state.store).unwrap(), Settings::default());
    }

    #[test]
    fn invalid_new_settings_are_rejected_without_saving_anything() {
        let (dir, state) = setup();
        let not_executable = dir.path().join("plain");
        std::fs::write(&not_executable, "").unwrap();
        let base = Settings { theme: Theme::Light, ..Settings::default() };
        for bad in [
            Settings { terminal_scrollback: 1234, ..base.clone() },
            Settings { local_shell: Some("sh".into()), ..base.clone() },
            Settings { local_shell: Some("/no/such/shell".into()), ..base.clone() },
            Settings { local_shell: Some(not_executable.display().to_string()), ..base.clone() },
            Settings { local_shell: Some("/bin/s\nh".into()), ..base.clone() },
            Settings { new_host_user: Some("a b'".into()), ..base.clone() },
            Settings { new_host_port: 0, ..base.clone() },
            Settings { new_host_keepalive_s: 3601, ..base.clone() },
            Settings { new_host_timeout_s: 601, ..base.clone() },
        ] {
            assert!(matches!(save_settings(&state, bad.clone()), Err(CoreError::Config { .. })), "{bad:?}");
            assert_eq!(load_settings(&state.store).unwrap().theme, Theme::Dark, "nothing saved");
        }
        let blank =
            save_settings(&state, Settings { local_shell: Some(" ".into()), new_host_user: Some(" ".into()), ..base })
                .unwrap();
        assert_eq!((blank.local_shell, blank.new_host_user), (None, None));
    }

    #[test]
    fn editor_command_roundtrips_and_empty_is_none() {
        let (_dir, state) = setup();
        assert_eq!(load_settings(&state.store).unwrap().editor_command, None);
        let base = load_settings(&state.store).unwrap();
        let saved = save_settings(&state, Settings { editor_command: Some("  kate ".into()), ..base.clone() }).unwrap();
        assert_eq!(saved.editor_command.as_deref(), Some("kate"));
        let saved = save_settings(&state, Settings { editor_command: Some("   ".into()), ..base.clone() }).unwrap();
        assert_eq!(saved.editor_command, None);
        assert!(save_settings(&state, Settings { editor_command: Some("a\nb".into()), ..base }).is_err());
    }
}
