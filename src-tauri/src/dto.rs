//! IPC data-transfer types that exist only in the Tauri layer.

use linascp_core::model::LoginProfile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    Dark,
    Light,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SshMode {
    Integrate,
    Mirror,
}

impl SshMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SshMode::Integrate => "integrate",
            SshMode::Mirror => "mirror",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "integrate" => Some(SshMode::Integrate),
            "mirror" => Some(SshMode::Mirror),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TerminalRenderer {
    Auto,
    Webgl,
    Dom,
}

impl TerminalRenderer {
    pub fn as_str(self) -> &'static str {
        match self {
            TerminalRenderer::Auto => "auto",
            TerminalRenderer::Webgl => "webgl",
            TerminalRenderer::Dom => "dom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(TerminalRenderer::Auto),
            "webgl" => Some(TerminalRenderer::Webgl),
            "dom" => Some(TerminalRenderer::Dom),
            _ => None,
        }
    }
}

/// All user-facing settings, stored as individual keys in the `settings` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: Theme,
    pub ssh_mode: SshMode,
    pub terminal_renderer: TerminalRenderer,
    pub terminal_font_size: u16,
    pub terminal_scheme: String,
    pub terminal_sidebar_open: bool,
    pub auto_reconnect: bool,
    pub first_run_done: bool,
    pub compat_rendering: bool,
    /// Command for editing files; `None` = the system's text editor.
    pub editor_command: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            ssh_mode: SshMode::Integrate,
            terminal_renderer: TerminalRenderer::Auto,
            terminal_font_size: 14,
            terminal_scheme: "auto".to_string(),
            terminal_sidebar_open: false,
            auto_reconnect: true,
            first_run_done: false,
            compat_rendering: false,
            editor_command: None,
        }
    }
}

/// Which program opens a file from the SFTP panes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum OpenApp {
    /// "Bearbeiten": the editor setting, else the `text/plain` default.
    Editor,
    /// "Öffnen": the app remembered for the file type, else the editor.
    Default,
    /// "Öffnen mit": a desktop id; `remember` makes it the default for the type (in LinaSCP only).
    App { id: String, remember: bool },
}

/// The app picked in the system's "Open with" dialog.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChosenApp {
    pub id: String,
    pub remember: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileWithUsage {
    pub profile: LoginProfile,
    pub used_by: u32,
}

/// Result of `connect_ssh`: the registered session and what was negotiated for it.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedSession {
    pub session_id: linascp_core::session::SessionId,
    pub info: linascp_core::connect::SessionInfo,
    /// The host's port-forward rules as they were at connect time (the tunnels run these).
    pub forwards: Vec<linascp_core::model::PortForward>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_roundtrip_through_their_store_strings() {
        for t in [Theme::Dark, Theme::Light] {
            assert_eq!(Theme::parse(t.as_str()), Some(t));
        }
        for m in [SshMode::Integrate, SshMode::Mirror] {
            assert_eq!(SshMode::parse(m.as_str()), Some(m));
        }
        for r in [TerminalRenderer::Auto, TerminalRenderer::Webgl, TerminalRenderer::Dom] {
            assert_eq!(TerminalRenderer::parse(r.as_str()), Some(r));
        }
        assert_eq!(Theme::parse("blue"), None);
    }

    #[test]
    fn settings_serialize_camel_case() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "theme": "dark", "sshMode": "integrate", "terminalRenderer": "auto",
                "terminalFontSize": 14, "terminalScheme": "auto", "terminalSidebarOpen": false,
                "autoReconnect": true, "firstRunDone": false, "compatRendering": false, "editorCommand": null
            })
        );
    }
}
