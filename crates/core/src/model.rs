use serde::{Deserialize, Serialize};

pub type Id = uuid::Uuid;

/// Current time as unix seconds.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AuthMethod {
    Password,
    Key,
    Agent,
    KeyboardInteractive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    pub method: AuthMethod,
    pub key_path: Option<String>,
    pub has_secret: bool,
}

impl AuthConfig {
    pub fn new(method: AuthMethod) -> Self {
        Self { method, key_path: None, has_secret: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum HostAuth {
    Profile { profile_id: Id },
    Custom { auth: AuthConfig },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum HostSource {
    Linascp,
    SshConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ForwardKind {
    Local,
    Remote,
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortForward {
    pub id: Id,
    pub kind: ForwardKind,
    pub bind: String,
    pub target: String,
    pub autostart: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TransferMode {
    Binary,
    Text,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SftpSettings {
    pub remote_start: Option<String>,
    pub local_start: Option<String>,
    pub transfer_mode: TransferMode,
    pub parallel: u32,
    pub upload_perms: Option<String>,
    pub bandwidth_limit_kbps: Option<u32>,
    pub sftp_server_cmd: Option<String>,
}

impl Default for SftpSettings {
    fn default() -> Self {
        Self {
            remote_start: None,
            local_start: None,
            transfer_mode: TransferMode::Binary,
            parallel: 4,
            upload_perms: None,
            bandwidth_limit_kbps: None,
            sftp_server_cmd: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: Id,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub user: Option<String>,
    pub group_id: Option<Id>,
    pub color: Option<String>,
    pub notes: String,
    pub tags: Vec<String>,
    pub protocol_ssh: bool,
    pub protocol_sftp: bool,
    pub auth: HostAuth,
    pub jump_host_id: Option<Id>,
    pub keepalive_s: u32,
    pub timeout_s: u32,
    pub start_command: Option<String>,
    pub env: Vec<EnvVar>,
    pub sftp: SftpSettings,
    pub forwards: Vec<PortForward>,
    /// Terminal color scheme override (`"auto"` or a `schemes.json` id); `None` = global setting.
    #[serde(default)]
    pub terminal_scheme: Option<String>,
    /// Terminal font size override in px; `None` = global setting.
    #[serde(default)]
    pub terminal_font_size: Option<u16>,
    pub source: HostSource,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Host {
    /// The host's own auth settings; `None` when it uses a login profile.
    pub fn custom_auth(&self) -> Option<&AuthConfig> {
        match &self.auth {
            HostAuth::Custom { auth } => Some(auth),
            HostAuth::Profile { .. } => None,
        }
    }

    pub fn new(name: &str, address: &str) -> Self {
        let now = now_unix();
        Self {
            id: uuid::Uuid::new_v4(),
            name: name.to_string(),
            address: address.to_string(),
            port: 22,
            user: None,
            group_id: None,
            color: None,
            notes: String::new(),
            tags: Vec::new(),
            protocol_ssh: true,
            protocol_sftp: true,
            auth: HostAuth::Custom { auth: AuthConfig::new(AuthMethod::Password) },
            jump_host_id: None,
            keepalive_s: 30,
            timeout_s: 15,
            start_command: None,
            env: Vec::new(),
            sftp: SftpSettings::default(),
            forwards: Vec::new(),
            terminal_scheme: None,
            terminal_font_size: None,
            source: HostSource::Linascp,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LoginProfile {
    pub id: Id,
    pub name: String,
    pub user: String,
    pub auth: AuthConfig,
    pub created_at: i64,
    pub updated_at: i64,
}

impl LoginProfile {
    pub fn new(name: &str, user: &str, auth: AuthConfig) -> Self {
        let now = now_unix();
        Self {
            id: uuid::Uuid::new_v4(),
            name: name.to_string(),
            user: user.to_string(),
            auth,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: Id,
    pub name: String,
    pub sort_order: i32,
}

impl Group {
    pub fn new(name: &str, sort_order: i32) -> Self {
        Self { id: uuid::Uuid::new_v4(), name: name.to_string(), sort_order }
    }
}

/// A remembered folder: local (`host_id == None`) or on one host's remote side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: Id,
    pub host_id: Option<Id>,
    pub path: String,
    pub position: i64,
    pub created_at: i64,
}

/// A saved command, global (`host_id == None`) or bound to one stored host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: Id,
    pub name: String,
    pub command: String,
    pub host_id: Option<Id>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Snippet {
    pub fn new(name: &str, command: &str, host_id: Option<Id>) -> Self {
        let now = now_unix();
        Self {
            id: uuid::Uuid::new_v4(),
            name: name.to_string(),
            command: command.to_string(),
            host_id,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn host_without_terminal_fields_deserializes_to_none() {
        let mut json = serde_json::to_value(Host::new("a", "a.example")).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("terminalScheme");
        object.remove("terminalFontSize");
        let host: Host = serde_json::from_value(json).unwrap();
        assert_eq!((host.terminal_scheme, host.terminal_font_size), (None, None));
    }

    #[test]
    fn host_new_has_contract_defaults() {
        let h = Host::new("web", "10.0.0.1");
        assert_eq!(h.name, "web");
        assert_eq!(h.address, "10.0.0.1");
        assert_eq!(h.port, 22);
        assert_eq!(h.keepalive_s, 30);
        assert_eq!(h.timeout_s, 15);
        assert!(h.protocol_ssh && h.protocol_sftp);
        assert_eq!(h.auth, HostAuth::Custom { auth: AuthConfig::new(AuthMethod::Password) });
        assert_eq!(h.source, HostSource::Linascp);
        assert_eq!(h.sftp, SftpSettings::default());
        assert!(h.created_at > 0 && h.created_at == h.updated_at);
        assert_ne!(Host::new("a", "b").id, h.id);
    }

    #[test]
    fn sftp_settings_default() {
        let s = SftpSettings::default();
        assert_eq!(s.transfer_mode, TransferMode::Binary);
        assert_eq!(s.parallel, 4);
        assert!(s.remote_start.is_none() && s.bandwidth_limit_kbps.is_none());
    }

    #[test]
    fn host_auth_serializes_with_kind_tag_and_camel_case_fields() {
        let id = uuid::Uuid::nil();
        let v = serde_json::to_value(HostAuth::Profile { profile_id: id }).unwrap();
        assert_eq!(v, json!({"kind": "profile", "profileId": id.to_string()}));
        let v =
            serde_json::to_value(HostAuth::Custom { auth: AuthConfig::new(AuthMethod::KeyboardInteractive) }).unwrap();
        assert_eq!(
            v,
            json!({"kind": "custom", "auth": {"method": "keyboardInteractive", "keyPath": null, "hasSecret": false}})
        );
    }

    #[test]
    fn host_json_roundtrip_uses_camel_case() {
        let h = Host::new("web", "example.com");
        let v = serde_json::to_value(&h).unwrap();
        assert_eq!(v["keepaliveS"], json!(30));
        assert_eq!(v["source"], json!("linascp"));
        assert_eq!(v["sftp"]["transferMode"], json!("binary"));
        let back: Host = serde_json::from_value(v).unwrap();
        assert_eq!(back, h);
    }
}
