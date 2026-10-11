### Task 1: Workspace scaffold, errors, paths, model

All code in Tasks 1–7 was compiled and tested against the real crate versions listed here (rustc 1.98, edition 2024). Run every command from the project root `/home/carloskopra/SSD2000/workDir/Developement/OTHER/LinaSFTP`.

**Prerequisites (Debian 13):** `sudo apt install build-essential pkg-config libdbus-1-dev` (`libdbus-1-dev` is needed by the Secret Service backend; SQLite is bundled).

**Contract note: CoreError uses `tag = "type"`.** The contract's default `tag = "kind"` collides with the `kind` field of `CoreError::Network`. `CoreError` is therefore serialized as `{"type":"network","kind":"timeout","message":"..."}`, `{"type":"auth","rejectedMethods":[...],"message":"..."}`, `{"type":"cancelled"}`. UI code switches on `error.type`. All other data-carrying enums keep `tag = "kind"`. Every enum with struct variants also uses `rename_all_fields = "camelCase"` so variant fields are camelCase in JSON (e.g. `HostAuth::Profile` → `{"kind":"profile","profileId":"..."}`).

**Files:**
- Create: `Cargo.toml`, `rustfmt.toml`, `.gitignore`
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`
- Create: `crates/core/src/error.rs`, `crates/core/src/paths.rs`, `crates/core/src/model.rs`
- Test: unit tests inside `error.rs`, `paths.rs`, `model.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `pub enum NetworkKind { Timeout, Refused, Dns, Other }`
  - `pub enum CoreError { Network { kind: NetworkKind, message: String }, HostKeyRejected { message: String }, Auth { rejected_methods: Vec<String>, message: String }, Keyring { message: String }, Store { message: String }, Config { message: String }, Io { message: String }, Cancelled, Internal { message: String } }` + helper constructors `CoreError::store/config/keyring/internal(impl Into<String>)`
  - `pub type Result<T> = std::result::Result<T, CoreError>;` with `From<rusqlite::Error>` (→ Store) and `From<std::io::Error>` (→ Io)
  - `pub struct AppPaths { pub data_dir, pub state_dir, pub ssh_dir, pub home }` with `from_env() -> Result<Self>`, `for_root(&Path) -> Self`, `db_path()`, `known_hosts()`, `ssh_config()`, `log_dir()`, `imported_keys_dir()`, plus `ensure_dirs(&self) -> Result<()>`
  - `model.rs`: `Id`, `AuthMethod`, `AuthConfig` (+ `AuthConfig::new(method)`), `HostAuth`, `HostSource`, `ForwardKind`, `PortForward`, `EnvVar`, `TransferMode`, `SftpSettings` (+ `Default`), `Host` (+ `Host::new`), `LoginProfile` (+ `LoginProfile::new(name, user, auth)`), `Group` (+ `Group::new(name, sort_order)`), `pub fn now_unix() -> i64`

- [ ] **Step 1: Create the workspace scaffold**

Initialize git only if the project is not yet a repository:

```bash
git rev-parse --git-dir >/dev/null 2>&1 || git init
mkdir -p crates/core/src
```

`Cargo.toml` (workspace root; `src-tauri` is added to `members` in Task 13):

```toml
[workspace]
resolver = "3"
members = ["crates/core"]

# Dependencies are optimized even in dev builds: age/scrypt and russh crypto are
# unbearably slow at opt-level 0 (export tests would take minutes).
[profile.dev.package."*"]
opt-level = 2
```

`rustfmt.toml`:

```toml
max_width = 120
use_small_heuristics = "Max"
```

`.gitignore`:

```gitignore
target/
node_modules/
dist/
```

`crates/core/Cargo.toml` (all dependencies needed by Tasks 1–7):

```toml
[package]
name = "linascp-core"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
name = "linascp_core"
path = "src/lib.rs"

[dependencies]
# age 0.12 does not compile together with russh 0.64 (ml-kem 0.2 vs 0.3 share one `kem` 0.3.0) -> stay on 0.11.
age = { version = "0.11", features = ["armor"] }
base64 = "0.22"
# keyring v4 = keyring-core + backend store crates; the `keyring` crate itself is only a v1-compat/CLI wrapper.
dbus-secret-service-keyring-store = { version = "1", features = ["crypto-rust"] }
getrandom = "0.4"
hmac = "0.13"
keyring-core = "1"
md-5 = "0.11"
rusqlite = { version = "0.40", features = ["bundled"] }
russh = "0.64"
secrecy = "0.10"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha1 = "0.11"
specta = { version = "=2.0.0-rc.25", features = ["derive", "uuid"] }
ssh2-config = "0.8"
thiserror = "2"
uuid = { version = "1", features = ["v4", "v5", "serde"] }

[dev-dependencies]
tempfile = "3"
```

`crates/core/src/lib.rs`:

```rust
//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod error;
pub mod model;
pub mod paths;

pub use error::{CoreError, NetworkKind, Result};
pub use model::*;
pub use paths::AppPaths;
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/error.rs` (tests only for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn network_error_serializes_with_type_tag_and_kind_field() {
        let err = CoreError::Network { kind: NetworkKind::Timeout, message: "timed out".into() };
        let value = serde_json::to_value(&err).unwrap();
        assert_eq!(value, json!({"type": "network", "kind": "timeout", "message": "timed out"}));
    }

    #[test]
    fn auth_error_uses_camel_case_fields() {
        let err = CoreError::Auth { rejected_methods: vec!["password".into()], message: "denied".into() };
        let value = serde_json::to_value(&err).unwrap();
        assert_eq!(value, json!({"type": "auth", "rejectedMethods": ["password"], "message": "denied"}));
    }

    #[test]
    fn unit_variant_and_roundtrip() {
        assert_eq!(serde_json::to_value(CoreError::Cancelled).unwrap(), json!({"type": "cancelled"}));
        let err = CoreError::HostKeyRejected { message: "no".into() };
        let back: CoreError = serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(back, err);
    }

    #[test]
    fn io_and_sqlite_errors_convert() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        assert!(matches!(CoreError::from(io), CoreError::Io { .. }));
        let sql = rusqlite::Error::QueryReturnedNoRows;
        assert!(matches!(CoreError::from(sql), CoreError::Store { .. }));
    }

    #[test]
    fn display_contains_message() {
        assert_eq!(CoreError::config("bad").to_string(), "configuration error: bad");
    }
}
```

`crates/core/src/paths.rs` (tests only for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_root_places_everything_below_root() {
        let p = AppPaths::for_root(Path::new("/tmp/x"));
        assert_eq!(p.home, PathBuf::from("/tmp/x/home"));
        assert_eq!(p.db_path(), PathBuf::from("/tmp/x/data/linascp.db"));
        assert_eq!(p.known_hosts(), PathBuf::from("/tmp/x/home/.ssh/known_hosts"));
        assert_eq!(p.ssh_config(), PathBuf::from("/tmp/x/home/.ssh/config"));
        assert_eq!(p.log_dir(), PathBuf::from("/tmp/x/state/logs"));
        assert_eq!(p.imported_keys_dir(), PathBuf::from("/tmp/x/home/.ssh/linascp"));
    }

    #[test]
    fn resolve_uses_xdg_defaults() {
        let p = AppPaths::resolve(Some("/home/u".into()), None, None).unwrap();
        assert_eq!(p.data_dir, PathBuf::from("/home/u/.local/share/linascp"));
        assert_eq!(p.state_dir, PathBuf::from("/home/u/.local/state/linascp"));
        assert_eq!(p.ssh_dir, PathBuf::from("/home/u/.ssh"));
    }

    #[test]
    fn resolve_honours_absolute_xdg_vars_and_ignores_relative_ones() {
        let p = AppPaths::resolve(Some("/home/u".into()), Some("/xdg/data".into()), Some("rel".into())).unwrap();
        assert_eq!(p.data_dir, PathBuf::from("/xdg/data/linascp"));
        assert_eq!(p.state_dir, PathBuf::from("/home/u/.local/state/linascp"));
    }

    #[test]
    fn resolve_without_home_is_config_error() {
        let err = AppPaths::resolve(None, None, None).unwrap_err();
        assert!(matches!(err, CoreError::Config { .. }));
    }

    #[test]
    fn ensure_dirs_creates_data_and_log_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let p = AppPaths::for_root(tmp.path());
        p.ensure_dirs().unwrap();
        assert!(p.data_dir.is_dir());
        assert!(p.log_dir().is_dir());
    }
}
```

`crates/core/src/model.rs` (tests only for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p linascp-core`
Expected: FAIL to compile — `error[E0432]: unresolved imports error::CoreError ...` / `cannot find type CoreError, AppPaths, Host in this scope`.

- [ ] **Step 4: Implement error.rs, paths.rs and model.rs**

Insert above the `#[cfg(test)]` block in `crates/core/src/error.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Classification of network failures, used by the UI to pick the error screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NetworkKind {
    Timeout,
    Refused,
    Dns,
    Other,
}

/// The single error type of `linascp-core`.
///
/// Serialized as an internally tagged object with the tag field `type`
/// (not `kind`, because the `Network` variant already has a field named `kind`):
/// `{"type":"network","kind":"timeout","message":"..."}`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CoreError {
    #[error("network error ({kind:?}): {message}")]
    Network { kind: NetworkKind, message: String },
    #[error("host key rejected: {message}")]
    HostKeyRejected { message: String },
    #[error("authentication failed: {message}")]
    Auth { rejected_methods: Vec<String>, message: String },
    #[error("keyring error: {message}")]
    Keyring { message: String },
    #[error("store error: {message}")]
    Store { message: String },
    #[error("configuration error: {message}")]
    Config { message: String },
    #[error("i/o error: {message}")]
    Io { message: String },
    #[error("cancelled")]
    Cancelled,
    #[error("internal error: {message}")]
    Internal { message: String },
}

pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    pub fn store(message: impl Into<String>) -> Self {
        Self::Store { message: message.into() }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Config { message: message.into() }
    }

    pub fn keyring(message: impl Into<String>) -> Self {
        Self::Keyring { message: message.into() }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal { message: message.into() }
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(err: rusqlite::Error) -> Self {
        Self::store(err.to_string())
    }
}

impl From<std::io::Error> for CoreError {
    fn from(err: std::io::Error) -> Self {
        Self::Io { message: err.to_string() }
    }
}
```

Insert above the `#[cfg(test)]` block in `crates/core/src/paths.rs`:

```rust
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

/// All filesystem locations used by LinaSCP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub ssh_dir: PathBuf,
    pub home: PathBuf,
}

impl AppPaths {
    /// Resolves the real locations from `HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME`.
    pub fn from_env() -> Result<Self> {
        Self::resolve(std::env::var_os("HOME"), std::env::var_os("XDG_DATA_HOME"), std::env::var_os("XDG_STATE_HOME"))
    }

    /// Test layout: everything lives below `root`.
    pub fn for_root(root: &Path) -> Self {
        let home = root.join("home");
        Self { data_dir: root.join("data"), state_dir: root.join("state"), ssh_dir: home.join(".ssh"), home }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("linascp.db")
    }

    pub fn known_hosts(&self) -> PathBuf {
        self.ssh_dir.join("known_hosts")
    }

    pub fn ssh_config(&self) -> PathBuf {
        self.ssh_dir.join("config")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }

    pub fn imported_keys_dir(&self) -> PathBuf {
        self.ssh_dir.join("linascp")
    }

    /// Creates `data_dir` and `log_dir` if they do not exist yet.
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.log_dir())?;
        Ok(())
    }

    fn resolve(home: Option<OsString>, xdg_data: Option<OsString>, xdg_state: Option<OsString>) -> Result<Self> {
        let home = home
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| CoreError::config("HOME is not set"))?;
        let data_base = absolute_or(xdg_data, home.join(".local/share"));
        let state_base = absolute_or(xdg_state, home.join(".local/state"));
        Ok(Self {
            data_dir: data_base.join("linascp"),
            state_dir: state_base.join("linascp"),
            ssh_dir: home.join(".ssh"),
            home,
        })
    }
}

/// XDG base directory spec: relative or empty values must be ignored.
fn absolute_or(value: Option<OsString>, fallback: PathBuf) -> PathBuf {
    value.map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or(fallback)
}
```

Insert above the `#[cfg(test)]` block in `crates/core/src/model.rs`:

```rust
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
    pub source: HostSource,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Host {
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo fmt && cargo test -p linascp-core && cargo clippy -p linascp-core --all-targets`
Expected: PASS — `test result: ok. 14 passed`; clippy without warnings. (The first build compiles russh/age/sqlite and takes a few minutes.)

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock rustfmt.toml .gitignore crates/core
git commit -m "feat(core): scaffold workspace with error, paths and model types" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Store with migrations

**Files:**
- Create: `crates/core/src/store/mod.rs`, `crates/core/src/store/migrations.rs`
- Test: `crates/core/src/store/tests.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `CoreError`, `Result`, all model types, `now_unix()` (Task 1)
- Produces:
  - `pub struct Store` (holds `Mutex<rusqlite::Connection>`)
  - `Store::open(&Path) -> Result<Self>` (creates parent dir, runs migrations), `Store::open_in_memory() -> Result<Self>`
  - `list_hosts / get_host / save_host / delete_host`, `list_profiles / get_profile / save_profile / delete_profile / profile_usage`, `list_groups / save_group / delete_group`, `get_setting / set_setting` — exact signatures as in the contract
  - consts `THEME`, `SSH_MODE`, `TERMINAL_RENDERER`, `TERMINAL_FONT_SIZE`, `FIRST_RUN_DONE` in `linascp_core::store`
  - Behaviour: `save_host`/`save_profile` write `updated_at = now` into the DB (the argument is not mutated — callers that need the saved value call `get_host`/`get_profile`); `created_at` is never changed on update; host lists are sorted by name (case-insensitive), profiles by name, groups by `sort_order, name`; tags keep insertion order (duplicates dropped); forwards keep list order; `delete_host` also sets `jump_host_id = NULL` on hosts that used it as jump host; `delete_group` sets `group_id = NULL`; `delete_profile` fails with `CoreError::Store { message: "profile in use" }` while `profile_usage > 0`.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/store/tests.rs`:

```rust
use super::*;
use crate::model::{EnvVar, SftpSettings, TransferMode};

fn full_host() -> Host {
    let mut h = Host::new("web-1", "10.0.0.5");
    h.port = 2222;
    h.user = Some("deploy".into());
    h.color = Some("#ff8800".into());
    h.notes = "production".into();
    h.tags = vec!["prod".into(), "eu".into()];
    h.protocol_sftp = false;
    h.auth = HostAuth::Custom {
        auth: AuthConfig { method: AuthMethod::Key, key_path: Some("/home/u/.ssh/id_ed25519".into()), has_secret: true },
    };
    h.keepalive_s = 10;
    h.timeout_s = 5;
    h.start_command = Some("tmux attach".into());
    h.env = vec![EnvVar { key: "LANG".into(), value: "C.UTF-8".into() }];
    h.sftp = SftpSettings {
        remote_start: Some("/var/www".into()),
        local_start: None,
        transfer_mode: TransferMode::Auto,
        parallel: 8,
        upload_perms: Some("0644".into()),
        bandwidth_limit_kbps: Some(512),
        sftp_server_cmd: Some("sudo /usr/lib/openssh/sftp-server".into()),
    };
    h.forwards = vec![
        PortForward {
            id: uuid::Uuid::new_v4(),
            kind: ForwardKind::Local,
            bind: "127.0.0.1:8080".into(),
            target: "localhost:80".into(),
            autostart: true,
        },
        PortForward {
            id: uuid::Uuid::new_v4(),
            kind: ForwardKind::Dynamic,
            bind: "1080".into(),
            target: String::new(),
            autostart: false,
        },
    ];
    h
}

#[test]
fn migrations_are_idempotent_and_set_user_version() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sub/linascp.db");
    Store::open(&path).unwrap();
    let store = Store::open(&path).unwrap();
    let version: i64 = store.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, migrations::LATEST_VERSION);
}

#[test]
fn host_roundtrip_with_all_fields() {
    let store = Store::open_in_memory().unwrap();
    let group = Group::new("Prod", 1);
    store.save_group(&group).unwrap();
    let mut host = full_host();
    host.group_id = Some(group.id);
    host.created_at = 1_000;
    host.updated_at = 1_000;
    store.save_host(&host).unwrap();

    let loaded = store.get_host(host.id).unwrap().unwrap();
    assert!(loaded.updated_at >= now_unix() - 5);
    let mut expected = host.clone();
    expected.updated_at = loaded.updated_at;
    assert_eq!(loaded, expected);
    assert_eq!(store.list_hosts().unwrap(), vec![expected]);
}

#[test]
fn save_host_upserts_and_replaces_children() {
    let store = Store::open_in_memory().unwrap();
    let mut host = full_host();
    store.save_host(&host).unwrap();
    host.name = "web-renamed".into();
    host.tags = vec!["staging".into()];
    host.forwards.truncate(1);
    store.save_host(&host).unwrap();

    let loaded = store.get_host(host.id).unwrap().unwrap();
    assert_eq!(loaded.name, "web-renamed");
    assert_eq!(loaded.tags, vec!["staging".to_string()]);
    assert_eq!(loaded.forwards, host.forwards);
    assert_eq!(store.list_hosts().unwrap().len(), 1);
}

#[test]
fn list_hosts_sorted_by_name_case_insensitive() {
    let store = Store::open_in_memory().unwrap();
    for name in ["beta", "Alpha", "gamma"] {
        store.save_host(&Host::new(name, "h")).unwrap();
    }
    let names: Vec<String> = store.list_hosts().unwrap().into_iter().map(|h| h.name).collect();
    assert_eq!(names, vec!["Alpha", "beta", "gamma"]);
}

#[test]
fn get_missing_host_is_none() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.get_host(uuid::Uuid::new_v4()).unwrap().is_none());
}

#[test]
fn delete_host_cascades_children_and_clears_jump_references() {
    let store = Store::open_in_memory().unwrap();
    let jump = full_host();
    let mut target = Host::new("target", "10.0.0.9");
    target.jump_host_id = Some(jump.id);
    store.save_host(&jump).unwrap();
    store.save_host(&target).unwrap();

    store.delete_host(jump.id).unwrap();

    assert!(store.get_host(jump.id).unwrap().is_none());
    assert_eq!(store.get_host(target.id).unwrap().unwrap().jump_host_id, None);
    let tags: i64 = store.conn().query_row("SELECT COUNT(*) FROM host_tags", [], |r| r.get(0)).unwrap();
    let fwds: i64 = store.conn().query_row("SELECT COUNT(*) FROM port_forwards", [], |r| r.get(0)).unwrap();
    assert_eq!((tags, fwds), (0, 0));
}

#[test]
fn profile_roundtrip_usage_and_delete_in_use() {
    let store = Store::open_in_memory().unwrap();
    let profile = LoginProfile::new(
        "admin",
        "root",
        AuthConfig { method: AuthMethod::KeyboardInteractive, key_path: None, has_secret: false },
    );
    store.save_profile(&profile).unwrap();
    let loaded = store.get_profile(profile.id).unwrap().unwrap();
    assert_eq!(loaded.auth, profile.auth);
    assert_eq!(loaded.name, "admin");
    assert_eq!(store.list_profiles().unwrap().len(), 1);

    let mut host = Host::new("uses-profile", "h");
    host.auth = HostAuth::Profile { profile_id: profile.id };
    store.save_host(&host).unwrap();
    assert_eq!(store.get_host(host.id).unwrap().unwrap().auth, host.auth);
    assert_eq!(store.profile_usage(profile.id).unwrap(), 1);

    let err = store.delete_profile(profile.id).unwrap_err();
    assert_eq!(err, CoreError::store("profile in use"));

    store.delete_host(host.id).unwrap();
    assert_eq!(store.profile_usage(profile.id).unwrap(), 0);
    store.delete_profile(profile.id).unwrap();
    assert!(store.get_profile(profile.id).unwrap().is_none());
}

#[test]
fn groups_sorted_and_delete_group_nulls_host_group() {
    let store = Store::open_in_memory().unwrap();
    let b = Group::new("B", 2);
    let a = Group::new("A", 1);
    store.save_group(&b).unwrap();
    store.save_group(&a).unwrap();
    assert_eq!(store.list_groups().unwrap(), vec![a.clone(), b.clone()]);

    let mut host = Host::new("in-group", "h");
    host.group_id = Some(a.id);
    store.save_host(&host).unwrap();
    store.delete_group(a.id).unwrap();

    assert_eq!(store.list_groups().unwrap(), vec![b]);
    assert_eq!(store.get_host(host.id).unwrap().unwrap().group_id, None);
}

#[test]
fn settings_get_and_set() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.get_setting(THEME).unwrap(), None);
    store.set_setting(THEME, "dark").unwrap();
    store.set_setting(THEME, "light").unwrap();
    assert_eq!(store.get_setting(THEME).unwrap(), Some("light".to_string()));
}
```

`crates/core/src/store/mod.rs` (only the test module declaration for now):

```rust
#[cfg(test)]
mod tests;
```

Replace `crates/core/src/lib.rs` with:

```rust
//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod error;
pub mod model;
pub mod paths;
pub mod store;

pub use error::{CoreError, NetworkKind, Result};
pub use model::*;
pub use paths::AppPaths;
pub use store::Store;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core store`
Expected: FAIL to compile — `error[E0432]: unresolved import store::Store` / `cannot find type Store`.

- [ ] **Step 3: Implement migrations**

`crates/core/src/store/migrations.rs`:

```rust
//! Versioned schema migrations. `PRAGMA user_version` holds the applied version.
//! Never edit a released migration; append a new one instead.

use rusqlite::Connection;

use crate::error::{CoreError, Result};

const V1: &str = r#"
CREATE TABLE groups (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE login_profiles (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    user        TEXT NOT NULL,
    method      TEXT NOT NULL CHECK (method IN ('password', 'key', 'agent', 'keyboard_interactive')),
    key_path    TEXT,
    has_secret  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE hosts (
    id                TEXT PRIMARY KEY NOT NULL,
    name              TEXT NOT NULL,
    address           TEXT NOT NULL,
    port              INTEGER NOT NULL DEFAULT 22,
    user              TEXT,
    group_id          TEXT,
    color             TEXT,
    notes             TEXT NOT NULL DEFAULT '',
    protocol_ssh      INTEGER NOT NULL DEFAULT 1,
    protocol_sftp     INTEGER NOT NULL DEFAULT 1,
    login_profile_id  TEXT,
    auth_method       TEXT CHECK (auth_method IN ('password', 'key', 'agent', 'keyboard_interactive')),
    key_path          TEXT,
    has_secret        INTEGER NOT NULL DEFAULT 0,
    jump_host_id      TEXT,
    keepalive_s       INTEGER NOT NULL DEFAULT 30,
    timeout_s         INTEGER NOT NULL DEFAULT 15,
    start_command     TEXT,
    env               TEXT NOT NULL DEFAULT '[]',
    sftp_settings     TEXT NOT NULL DEFAULT '{}',
    source            TEXT NOT NULL DEFAULT 'linascp' CHECK (source IN ('linascp', 'ssh_config')),
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    CHECK ((login_profile_id IS NULL) <> (auth_method IS NULL))
);
CREATE INDEX hosts_group_id ON hosts(group_id);
CREATE INDEX hosts_login_profile_id ON hosts(login_profile_id);

CREATE TABLE host_tags (
    host_id  TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    tag      TEXT NOT NULL,
    PRIMARY KEY (host_id, tag)
);

CREATE TABLE port_forwards (
    id         TEXT PRIMARY KEY NOT NULL,
    host_id    TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    type       TEXT NOT NULL CHECK (type IN ('L', 'R', 'D')),
    bind       TEXT NOT NULL,
    target     TEXT NOT NULL,
    autostart  INTEGER NOT NULL DEFAULT 0,
    position   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX port_forwards_host_id ON port_forwards(host_id);

CREATE TABLE settings (
    key    TEXT PRIMARY KEY NOT NULL,
    value  TEXT NOT NULL
);
"#;

const MIGRATIONS: &[&str] = &[V1];

/// Latest schema version known to this build.
pub(crate) const LATEST_VERSION: i64 = MIGRATIONS.len() as i64;

/// Applies all pending migrations, each in its own transaction.
pub(crate) fn run(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > LATEST_VERSION {
        return Err(CoreError::store(format!(
            "database schema version {current} is newer than supported version {LATEST_VERSION}"
        )));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        tx.commit()?;
    }
    Ok(())
}
```

Note on the schema: `group_id`, `login_profile_id` and `jump_host_id` deliberately have no foreign keys — a LinaSCP host may use an ssh-config host (not stored in the DB, uuid v5 id) as jump host, and the store resets references explicitly in `delete_group`/`delete_host`. `host_tags` and `port_forwards` cascade on host deletion (`PRAGMA foreign_keys = ON` is set per connection).

- [ ] **Step 4: Implement the store**

Replace `crates/core/src/store/mod.rs` with:

```rust
//! SQLite persistence for hosts, login profiles, groups and settings.

mod migrations;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::error::{CoreError, Result};
use crate::model::{
    AuthConfig, AuthMethod, ForwardKind, Group, Host, HostAuth, HostSource, Id, LoginProfile, PortForward, now_unix,
};

/// Setting key: `"dark"` | `"light"`.
pub const THEME: &str = "theme";
/// Setting key: `"integrate"` | `"mirror"`.
pub const SSH_MODE: &str = "ssh_mode";
/// Setting key: `"auto"` | `"webgl"` | `"dom"`.
pub const TERMINAL_RENDERER: &str = "terminal_renderer";
/// Setting key: font size in px as decimal string.
pub const TERMINAL_FONT_SIZE: &str = "terminal_font_size";
/// Setting key: `"true"` once the first-run dialog was completed.
pub const FIRST_RUN_DONE: &str = "first_run_done";

const HOST_COLUMNS: &str = "id, name, address, port, user, group_id, color, notes, protocol_ssh, protocol_sftp, \
    login_profile_id, auth_method, key_path, has_secret, jump_host_id, keepalive_s, timeout_s, start_command, \
    env, sftp_settings, source, created_at, updated_at";

const PROFILE_COLUMNS: &str = "id, name, user, method, key_path, has_secret, created_at, updated_at";

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Opens (and creates/migrates) the database file. Creates the parent directory.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrations::run(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // ---- hosts ----

    pub fn list_hosts(&self) -> Result<Vec<Host>> {
        let conn = self.conn();
        let sql = format!("SELECT {HOST_COLUMNS} FROM hosts ORDER BY name COLLATE NOCASE, id");
        let mut stmt = conn.prepare(&sql)?;
        let mut hosts = stmt.query_map([], host_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        for host in &mut hosts {
            load_host_children(&conn, host)?;
        }
        Ok(hosts)
    }

    pub fn get_host(&self, id: Id) -> Result<Option<Host>> {
        let conn = self.conn();
        let sql = format!("SELECT {HOST_COLUMNS} FROM hosts WHERE id = ?1");
        let host = conn.query_row(&sql, [id.to_string()], host_from_row).optional()?;
        match host {
            Some(mut host) => {
                load_host_children(&conn, &mut host)?;
                Ok(Some(host))
            }
            None => Ok(None),
        }
    }

    /// Upserts the host including tags and port forwards. `updated_at` is set to now.
    pub fn save_host(&self, host: &Host) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        upsert_host(&tx, host)?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes the host; other hosts using it as jump host get `jump_host_id = NULL`.
    pub fn delete_host(&self, id: Id) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("UPDATE hosts SET jump_host_id = NULL WHERE jump_host_id = ?1", [id.to_string()])?;
        tx.execute("DELETE FROM hosts WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    // ---- login profiles ----

    pub fn list_profiles(&self) -> Result<Vec<LoginProfile>> {
        let conn = self.conn();
        let sql = format!("SELECT {PROFILE_COLUMNS} FROM login_profiles ORDER BY name COLLATE NOCASE, id");
        let mut stmt = conn.prepare(&sql)?;
        let profiles = stmt.query_map([], profile_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(profiles)
    }

    pub fn get_profile(&self, id: Id) -> Result<Option<LoginProfile>> {
        let conn = self.conn();
        let sql = format!("SELECT {PROFILE_COLUMNS} FROM login_profiles WHERE id = ?1");
        Ok(conn.query_row(&sql, [id.to_string()], profile_from_row).optional()?)
    }

    /// Upserts the profile. `updated_at` is set to now.
    pub fn save_profile(&self, p: &LoginProfile) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO login_profiles (id, name, user, method, key_path, has_secret, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, user = excluded.user, method = excluded.method, \
             key_path = excluded.key_path, has_secret = excluded.has_secret, updated_at = excluded.updated_at",
            params![
                p.id.to_string(),
                p.name,
                p.user,
                auth_method_to_db(p.auth.method),
                p.auth.key_path,
                p.auth.has_secret,
                p.created_at,
                now_unix(),
            ],
        )?;
        Ok(())
    }

    /// Fails with `Store{"profile in use"}` while any host references the profile.
    pub fn delete_profile(&self, id: Id) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if profile_usage_in(&tx, id)? > 0 {
            return Err(CoreError::store("profile in use"));
        }
        tx.execute("DELETE FROM login_profiles WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    /// Number of hosts that use the profile.
    pub fn profile_usage(&self, id: Id) -> Result<u32> {
        let conn = self.conn();
        profile_usage_in(&conn, id)
    }

    // ---- groups ----

    pub fn list_groups(&self) -> Result<Vec<Group>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT id, name, sort_order FROM groups ORDER BY sort_order, name COLLATE NOCASE, id")?;
        let groups = stmt
            .query_map([], |row| Ok(Group { id: id_col(row, 0)?, name: row.get(1)?, sort_order: row.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(groups)
    }

    pub fn save_group(&self, g: &Group) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO groups (id, name, sort_order) VALUES (?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, sort_order = excluded.sort_order",
            params![g.id.to_string(), g.name, g.sort_order],
        )?;
        Ok(())
    }

    /// Deletes the group; its hosts get `group_id = NULL`.
    pub fn delete_group(&self, id: Id) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("UPDATE hosts SET group_id = NULL WHERE group_id = ?1", [id.to_string()])?;
        tx.execute("DELETE FROM groups WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    // ---- settings ----

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn();
        Ok(conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

fn upsert_host(tx: &Transaction<'_>, host: &Host) -> Result<()> {
    let (profile_id, method, key_path, has_secret) = match &host.auth {
        HostAuth::Profile { profile_id } => (Some(profile_id.to_string()), None, None, false),
        HostAuth::Custom { auth } => (None, Some(auth_method_to_db(auth.method)), auth.key_path.clone(), auth.has_secret),
    };
    let env = serde_json::to_string(&host.env).map_err(|e| CoreError::internal(e.to_string()))?;
    let sftp = serde_json::to_string(&host.sftp).map_err(|e| CoreError::internal(e.to_string()))?;
    let sql = format!(
        "INSERT INTO hosts ({HOST_COLUMNS}) VALUES \
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, address = excluded.address, port = excluded.port, \
         user = excluded.user, group_id = excluded.group_id, color = excluded.color, notes = excluded.notes, \
         protocol_ssh = excluded.protocol_ssh, protocol_sftp = excluded.protocol_sftp, \
         login_profile_id = excluded.login_profile_id, auth_method = excluded.auth_method, \
         key_path = excluded.key_path, has_secret = excluded.has_secret, jump_host_id = excluded.jump_host_id, \
         keepalive_s = excluded.keepalive_s, timeout_s = excluded.timeout_s, \
         start_command = excluded.start_command, env = excluded.env, sftp_settings = excluded.sftp_settings, \
         source = excluded.source, updated_at = excluded.updated_at"
    );
    tx.execute(
        &sql,
        params![
            host.id.to_string(),
            host.name,
            host.address,
            host.port,
            host.user,
            host.group_id.map(|g| g.to_string()),
            host.color,
            host.notes,
            host.protocol_ssh,
            host.protocol_sftp,
            profile_id,
            method,
            key_path,
            has_secret,
            host.jump_host_id.map(|j| j.to_string()),
            host.keepalive_s,
            host.timeout_s,
            host.start_command,
            env,
            sftp,
            source_to_db(host.source),
            host.created_at,
            now_unix(),
        ],
    )?;

    let host_id = host.id.to_string();
    tx.execute("DELETE FROM host_tags WHERE host_id = ?1", [&host_id])?;
    for tag in &host.tags {
        tx.execute("INSERT OR IGNORE INTO host_tags (host_id, tag) VALUES (?1, ?2)", params![host_id, tag])?;
    }
    tx.execute("DELETE FROM port_forwards WHERE host_id = ?1", [&host_id])?;
    for (position, fwd) in host.forwards.iter().enumerate() {
        tx.execute(
            "INSERT INTO port_forwards (id, host_id, type, bind, target, autostart, position) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                fwd.id.to_string(),
                host_id,
                forward_kind_to_db(fwd.kind),
                fwd.bind,
                fwd.target,
                fwd.autostart,
                position as i64,
            ],
        )?;
    }
    Ok(())
}

fn load_host_children(conn: &Connection, host: &mut Host) -> Result<()> {
    let host_id = host.id.to_string();
    let mut stmt = conn.prepare("SELECT tag FROM host_tags WHERE host_id = ?1 ORDER BY rowid")?;
    host.tags = stmt.query_map([&host_id], |row| row.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
    let mut stmt = conn.prepare(
        "SELECT id, type, bind, target, autostart FROM port_forwards WHERE host_id = ?1 ORDER BY position, rowid",
    )?;
    host.forwards = stmt
        .query_map([&host_id], |row| {
            Ok(PortForward {
                id: id_col(row, 0)?,
                kind: forward_kind_from_db(&row.get::<_, String>(1)?).map_err(|e| conv_err(1, e))?,
                bind: row.get(2)?,
                target: row.get(3)?,
                autostart: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(())
}

fn profile_usage_in(conn: &Connection, id: Id) -> Result<u32> {
    Ok(conn.query_row("SELECT COUNT(*) FROM hosts WHERE login_profile_id = ?1", [id.to_string()], |row| row.get(0))?)
}

fn host_from_row(row: &Row<'_>) -> rusqlite::Result<Host> {
    let profile_id = opt_id_col(row, 10)?;
    let auth = match profile_id {
        Some(profile_id) => HostAuth::Profile { profile_id },
        None => {
            let method: Option<String> = row.get(11)?;
            let method = method.unwrap_or_else(|| "password".to_string());
            HostAuth::Custom {
                auth: AuthConfig {
                    method: auth_method_from_db(&method).map_err(|e| conv_err(11, e))?,
                    key_path: row.get(12)?,
                    has_secret: row.get(13)?,
                },
            }
        }
    };
    let env: String = row.get(18)?;
    let sftp: String = row.get(19)?;
    let source: String = row.get(20)?;
    Ok(Host {
        id: id_col(row, 0)?,
        name: row.get(1)?,
        address: row.get(2)?,
        port: row.get(3)?,
        user: row.get(4)?,
        group_id: opt_id_col(row, 5)?,
        color: row.get(6)?,
        notes: row.get(7)?,
        tags: Vec::new(),
        protocol_ssh: row.get(8)?,
        protocol_sftp: row.get(9)?,
        auth,
        jump_host_id: opt_id_col(row, 14)?,
        keepalive_s: row.get(15)?,
        timeout_s: row.get(16)?,
        start_command: row.get(17)?,
        env: serde_json::from_str(&env).map_err(|e| conv_err(18, e))?,
        sftp: serde_json::from_str(&sftp).map_err(|e| conv_err(19, e))?,
        forwards: Vec::new(),
        source: source_from_db(&source).map_err(|e| conv_err(20, e))?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
    })
}

fn profile_from_row(row: &Row<'_>) -> rusqlite::Result<LoginProfile> {
    let method: String = row.get(3)?;
    Ok(LoginProfile {
        id: id_col(row, 0)?,
        name: row.get(1)?,
        user: row.get(2)?,
        auth: AuthConfig {
            method: auth_method_from_db(&method).map_err(|e| conv_err(3, e))?,
            key_path: row.get(4)?,
            has_secret: row.get(5)?,
        },
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn conv_err(idx: usize, err: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(err))
}

fn id_col(row: &Row<'_>, idx: usize) -> rusqlite::Result<Id> {
    let s: String = row.get(idx)?;
    Id::parse_str(&s).map_err(|e| conv_err(idx, e))
}

fn opt_id_col(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<Id>> {
    let s: Option<String> = row.get(idx)?;
    s.map(|s| Id::parse_str(&s).map_err(|e| conv_err(idx, e))).transpose()
}

fn auth_method_to_db(method: AuthMethod) -> &'static str {
    match method {
        AuthMethod::Password => "password",
        AuthMethod::Key => "key",
        AuthMethod::Agent => "agent",
        AuthMethod::KeyboardInteractive => "keyboard_interactive",
    }
}

fn auth_method_from_db(s: &str) -> Result<AuthMethod> {
    match s {
        "password" => Ok(AuthMethod::Password),
        "key" => Ok(AuthMethod::Key),
        "agent" => Ok(AuthMethod::Agent),
        "keyboard_interactive" => Ok(AuthMethod::KeyboardInteractive),
        other => Err(CoreError::store(format!("unknown auth method {other:?}"))),
    }
}

fn forward_kind_to_db(kind: ForwardKind) -> &'static str {
    match kind {
        ForwardKind::Local => "L",
        ForwardKind::Remote => "R",
        ForwardKind::Dynamic => "D",
    }
}

fn forward_kind_from_db(s: &str) -> Result<ForwardKind> {
    match s {
        "L" => Ok(ForwardKind::Local),
        "R" => Ok(ForwardKind::Remote),
        "D" => Ok(ForwardKind::Dynamic),
        other => Err(CoreError::store(format!("unknown forward type {other:?}"))),
    }
}

fn source_to_db(source: HostSource) -> &'static str {
    match source {
        HostSource::Linascp => "linascp",
        HostSource::SshConfig => "ssh_config",
    }
}

fn source_from_db(s: &str) -> Result<HostSource> {
    match s {
        "linascp" => Ok(HostSource::Linascp),
        "ssh_config" => Ok(HostSource::SshConfig),
        other => Err(CoreError::store(format!("unknown host source {other:?}"))),
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo fmt && cargo test -p linascp-core store && cargo clippy -p linascp-core --all-targets`
Expected: PASS — `test result: ok. 9 passed` (filtered to `store`); clippy without warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/core
git commit -m "feat(core): add SQLite store with versioned migrations" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Secret store (keyring + in-memory)

**Files:**
- Create: `crates/core/src/secrets.rs`
- Modify: `crates/core/src/lib.rs`
- Test: unit tests inside `secrets.rs`

**Interfaces:**
- Consumes: `CoreError`, `Result`, `Id`
- Produces:
  - `pub enum SecretKind { Password, Passphrase }` (JSON `"password"`/`"passphrase"`)
  - `pub enum SecretOwner { Host(Id), Profile(Id) }` — **Contract note:** serialized *adjacently* tagged as `{"kind":"host"|"profile","id":"<uuid>"}` (serde cannot internally tag a newtype variant holding a string). Helper `SecretOwner::id(&self) -> Id`.
  - `pub trait SecretStore: Send + Sync { get, set, delete }` exactly as in the contract (`secrecy::SecretString`)
  - `pub struct KeyringSecretStore` with `KeyringSecretStore::new() -> Self` (infallible; D-Bus connection opened lazily on first call and retried after failures; errors → `CoreError::Keyring`)
  - `pub struct MemorySecretStore` with `new()`, `len()`, `is_empty()`
  - `pub fn account_name(owner: SecretOwner, kind: SecretKind) -> String`, `pub fn parse_account_name(&str) -> Option<(SecretOwner, SecretKind)>`, `pub const KEYRING_SERVICE: &str = "linascp"`
  - All methods are blocking. **Async callers (src-tauri, connect pipeline) must call them inside `tokio::task::spawn_blocking`.**

- [ ] **Step 1: Write the failing tests**

`crates/core/src/secrets.rs` (tests only for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    #[test]
    fn account_name_format() {
        let id = Id::parse_str("6f9619ff-8b86-d011-b42d-00c04fc964ff").unwrap();
        assert_eq!(
            account_name(SecretOwner::Host(id), SecretKind::Password),
            "host:6f9619ff-8b86-d011-b42d-00c04fc964ff:password"
        );
        assert_eq!(
            account_name(SecretOwner::Profile(id), SecretKind::Passphrase),
            "profile:6f9619ff-8b86-d011-b42d-00c04fc964ff:passphrase"
        );
    }

    #[test]
    fn parse_account_name_roundtrip_and_rejects_garbage() {
        let id = Id::new_v4();
        for owner in [SecretOwner::Host(id), SecretOwner::Profile(id)] {
            for kind in [SecretKind::Password, SecretKind::Passphrase] {
                assert_eq!(parse_account_name(&account_name(owner, kind)), Some((owner, kind)));
            }
        }
        assert_eq!(parse_account_name("host:nope:password"), None);
        assert_eq!(parse_account_name(&format!("user:{id}:password")), None);
        assert_eq!(parse_account_name(&format!("host:{id}:pin")), None);
        assert_eq!(parse_account_name(&format!("host:{id}:password:x")), None);
    }

    #[test]
    fn secret_owner_serializes_adjacently_tagged() {
        let id = Id::nil();
        let v = serde_json::to_value(SecretOwner::Profile(id)).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "profile", "id": id.to_string()}));
        let back: SecretOwner = serde_json::from_value(v).unwrap();
        assert_eq!(back, SecretOwner::Profile(id));
    }

    #[test]
    fn memory_store_set_get_delete() {
        let store = MemorySecretStore::new();
        let owner = SecretOwner::Host(Id::new_v4());
        assert!(store.get(owner, SecretKind::Password).unwrap().is_none());
        store.set(owner, SecretKind::Password, &secret("hunter2")).unwrap();
        store.set(owner, SecretKind::Passphrase, &secret("pp")).unwrap();
        assert_eq!(store.get(owner, SecretKind::Password).unwrap().unwrap().expose_secret(), "hunter2");
        assert_eq!(store.len(), 2);
        store.delete(owner, SecretKind::Password).unwrap();
        store.delete(owner, SecretKind::Password).unwrap();
        assert!(store.get(owner, SecretKind::Password).unwrap().is_none());
        assert_eq!(store.get(owner, SecretKind::Passphrase).unwrap().unwrap().expose_secret(), "pp");
    }

    #[test]
    #[ignore = "needs a D-Bus session with an unlocked Secret Service keyring"]
    fn keyring_store_roundtrip() {
        let store = KeyringSecretStore::new();
        let owner = SecretOwner::Host(Id::new_v4());
        store.set(owner, SecretKind::Password, &secret("keyring-test")).unwrap();
        assert_eq!(store.get(owner, SecretKind::Password).unwrap().unwrap().expose_secret(), "keyring-test");
        store.delete(owner, SecretKind::Password).unwrap();
        assert!(store.get(owner, SecretKind::Password).unwrap().is_none());
        store.delete(owner, SecretKind::Password).unwrap();
    }
}
```

Replace `crates/core/src/lib.rs` with:

```rust
//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod error;
pub mod model;
pub mod paths;
pub mod secrets;
pub mod store;

pub use error::{CoreError, NetworkKind, Result};
pub use model::*;
pub use paths::AppPaths;
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretKind, SecretOwner, SecretStore};
pub use store::Store;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core secrets`
Expected: FAIL to compile — `unresolved imports secrets::KeyringSecretStore, ...`.

- [ ] **Step 3: Implement secrets.rs**

Insert above the `#[cfg(test)]` block in `crates/core/src/secrets.rs`:

```rust
//! Secret storage (passwords, key passphrases) behind the `SecretStore` trait.
//!
//! All implementations are synchronous. `KeyringSecretStore` talks to the Secret Service
//! over D-Bus and may block (e.g. while the keyring unlock prompt is shown), so async
//! callers must wrap calls in `tokio::task::spawn_blocking`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use keyring_core::api::CredentialStoreApi;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::model::Id;

/// Keyring service name for all LinaSCP entries.
pub const KEYRING_SERVICE: &str = "linascp";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    Password,
    Passphrase,
}

/// Owner of a secret. Serialized adjacently tagged: `{"kind":"host","id":"<uuid>"}`
/// (internally tagged newtype variants holding a string are not representable in serde).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum SecretOwner {
    Host(Id),
    Profile(Id),
}

impl SecretOwner {
    pub fn id(&self) -> Id {
        match self {
            SecretOwner::Host(id) | SecretOwner::Profile(id) => *id,
        }
    }
}

pub trait SecretStore: Send + Sync {
    fn get(&self, owner: SecretOwner, kind: SecretKind) -> Result<Option<SecretString>>;
    fn set(&self, owner: SecretOwner, kind: SecretKind, value: &SecretString) -> Result<()>;
    /// Succeeds when the secret does not exist.
    fn delete(&self, owner: SecretOwner, kind: SecretKind) -> Result<()>;
}

/// Keyring account name: `"<host|profile>:<uuid>:<password|passphrase>"`.
pub fn account_name(owner: SecretOwner, kind: SecretKind) -> String {
    let (prefix, id) = match owner {
        SecretOwner::Host(id) => ("host", id),
        SecretOwner::Profile(id) => ("profile", id),
    };
    let kind = match kind {
        SecretKind::Password => "password",
        SecretKind::Passphrase => "passphrase",
    };
    format!("{prefix}:{id}:{kind}")
}

/// Inverse of [`account_name`]. Returns `None` for malformed names.
pub fn parse_account_name(name: &str) -> Option<(SecretOwner, SecretKind)> {
    let mut parts = name.split(':');
    let prefix = parts.next()?;
    let id = Id::parse_str(parts.next()?).ok()?;
    let kind = match parts.next()? {
        "password" => SecretKind::Password,
        "passphrase" => SecretKind::Passphrase,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    let owner = match prefix {
        "host" => SecretOwner::Host(id),
        "profile" => SecretOwner::Profile(id),
        _ => return None,
    };
    Some((owner, kind))
}

/// In-memory store for tests and as a fallback when no keyring is available.
#[derive(Default)]
pub struct MemorySecretStore {
    entries: Mutex<HashMap<String, String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored secrets.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, owner: SecretOwner, kind: SecretKind) -> Result<Option<SecretString>> {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(entries.get(&account_name(owner, kind)).map(|v| SecretString::from(v.clone())))
    }

    fn set(&self, owner: SecretOwner, kind: SecretKind, value: &SecretString) -> Result<()> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.insert(account_name(owner, kind), value.expose_secret().to_string());
        Ok(())
    }

    fn delete(&self, owner: SecretOwner, kind: SecretKind) -> Result<()> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.remove(&account_name(owner, kind));
        Ok(())
    }
}

type DbusStore = dbus_secret_service_keyring_store::Store;

/// Secret Service (GNOME Keyring / KWallet) backed store via `keyring-core`.
/// The D-Bus connection is opened lazily on first use and retried after failures,
/// so construction never fails even when no keyring daemon is running.
#[derive(Default)]
pub struct KeyringSecretStore {
    store: Mutex<Option<Arc<DbusStore>>>,
}

impl KeyringSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn entry(&self, owner: SecretOwner, kind: SecretKind) -> Result<keyring_core::Entry> {
        let store = {
            let mut guard = self.store.lock().unwrap_or_else(PoisonError::into_inner);
            match guard.as_ref() {
                Some(store) => Arc::clone(store),
                None => {
                    let store = DbusStore::new().map_err(keyring_err)?;
                    *guard = Some(Arc::clone(&store));
                    store
                }
            }
        };
        store.build(KEYRING_SERVICE, &account_name(owner, kind), None).map_err(keyring_err)
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, owner: SecretOwner, kind: SecretKind) -> Result<Option<SecretString>> {
        match self.entry(owner, kind)?.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(err) => Err(keyring_err(err)),
        }
    }

    fn set(&self, owner: SecretOwner, kind: SecretKind, value: &SecretString) -> Result<()> {
        self.entry(owner, kind)?.set_password(value.expose_secret()).map_err(keyring_err)
    }

    fn delete(&self, owner: SecretOwner, kind: SecretKind) -> Result<()> {
        match self.entry(owner, kind)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(err) => Err(keyring_err(err)),
        }
    }
}

fn keyring_err(err: keyring_core::Error) -> CoreError {
    CoreError::keyring(err.to_string())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo fmt && cargo test -p linascp-core secrets && cargo clippy -p linascp-core --all-targets`
Expected: PASS — `test result: ok. 4 passed; 0 failed; 1 ignored`.

Optional on a desktop session with an unlocked keyring: `cargo test -p linascp-core secrets -- --ignored` → `1 passed` (writes and deletes one temporary `linascp` entry).

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): add SecretStore with Secret Service and in-memory backends" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: known_hosts (plain, hashed, `[host]:port`) and private-file helpers

**Files:**
- Create: `crates/core/src/fsutil.rs` (crate-private helpers, reused by Tasks 6 and 7)
- Create: `crates/core/src/known_hosts.rs`
- Modify: `crates/core/src/lib.rs`
- Test: unit tests inside both files

**Interfaces:**
- Consumes: `CoreError`, `Result`; `russh::keys::{PublicKey, HashAlg}` (russh 0.64 re-exports `ssh-key` 0.7; no direct `ssh-key` dependency)
- Produces:
  - `pub(crate) fn fsutil::create_private_dir(&Path) -> Result<()>` (mode 0700, no-op if it exists)
  - `pub(crate) fn fsutil::write_private(&Path, &[u8]) -> Result<()>` (atomic temp file + rename, mode 0600)
  - `pub struct HostKeyInfo { pub algorithm: String, pub sha256: String, pub md5: String }` (camelCase JSON)
  - `pub enum HostKeyStatus { Known, Unknown, Changed { stored: HostKeyInfo } }` (`tag = "kind"`)
  - `pub fn fingerprints(key: &russh::keys::PublicKey) -> HostKeyInfo` — `sha256` = `"SHA256:<base64 no padding>"`, `md5` = `"MD5:aa:bb:..."`, identical to `ssh-keygen -l [-E md5]`
  - `pub struct KnownHosts` with `new(PathBuf)`, `check`, `add`, `replace` exactly as in the contract
  - Behaviour: host names are lowercased (like `ssh`); port != 22 uses `[host]:port`; `check` returns `Known` if any matching entry has the same key, `Changed` only if a matching entry has a *different key of the same algorithm*, else `Unknown`; wildcard (`*`, `?`) and negated (`!`) patterns are honoured; `@cert-authority`/`@revoked` lines are ignored; `add` creates `~/.ssh` (0700) and the file (0600) and inserts a missing trailing newline first; hashed entries use HMAC-SHA1 with a random 20-byte salt (verified readable by `ssh-keygen -F`); `replace` drops hashed lines / removes the matching pattern from plain multi-host lines for entries of the same algorithm, then appends a hashed entry.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/fsutil.rs` (tests only for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn write_private_creates_0600_file_and_replaces_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("f");
        write_private(&path, b"one").unwrap();
        write_private(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn create_private_dir_uses_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/b");
        create_private_dir(&dir).unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
```

`crates/core/src/known_hosts.rs` (tests only for now). The fixture keys were generated with `ssh-keygen -t ed25519|ecdsa -C ""`, the expected fingerprints come from `ssh-keygen -l` / `ssh-keygen -E md5 -l`, and the hashed lines from `ssh-keygen -H`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    // Generated with `ssh-keygen -t ed25519|ecdsa -C ""`; fingerprints from `ssh-keygen -l [-E md5]`.
    const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICi2vUL2Z58TUNsW6eB7m9Vm3RgGtVqEf1QNfW24jbho";
    const KEY_B: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHtwBGjTF/mmhwN4utNSNgAgcIRdABlX4NFv5Pvskb0S";
    const KEY_C: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBGlX0YFfeEtmpRikznN2qniQ1IyC7kaRaI2WTk76J4+jKC7gzqVPyYdfTLLgo3B/i11vZ53woAkukHOLm1kXsPE=";
    // `ssh-keygen -H` output for "example.com" and "[example.com]:2222" with KEY_A.
    const HASHED_22: &str = "|1|pD3/KWOuOWYIdPFBZoa3rqv/k60=|fUYbOJ2iF30CwYg1GZvmBr/l6Xw=";
    const HASHED_2222: &str = "|1|zwh7GbAXhP7NxCON2SJIl2sIg8I=|qG1O0+loDUTsKoROOLb7Yk5EcjQ=";

    fn key(s: &str) -> PublicKey {
        PublicKey::from_openssh(s).unwrap()
    }

    fn kh_with(content: &str) -> (tempfile::TempDir, KnownHosts) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        fs::write(&path, content).unwrap();
        (dir, KnownHosts::new(path))
    }

    #[test]
    fn fingerprints_match_ssh_keygen() {
        let a = fingerprints(&key(KEY_A));
        assert_eq!(a.algorithm, "ssh-ed25519");
        assert_eq!(a.sha256, "SHA256:jNtjcKMOvysttEhG3S6Ai4dCzblEgDjHFgmZE2lh2yI");
        assert_eq!(a.md5, "MD5:86:ea:16:e3:2a:b9:62:c1:70:5c:7c:13:71:85:ca:27");
        let c = fingerprints(&key(KEY_C));
        assert_eq!(c.algorithm, "ecdsa-sha2-nistp256");
        assert_eq!(c.sha256, "SHA256:NYjSAeB7moVPnKeW19QrTTWScnma4eKKubjc1FJ2GVA");
        assert_eq!(c.md5, "MD5:a8:cd:b3:d5:b4:56:9d:43:a4:2c:13:59:3b:55:81:86");
    }

    #[test]
    fn missing_file_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let kh = KnownHosts::new(dir.path().join("nope"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn plain_entries_known_changed_unknown() {
        let (_d, kh) = kh_with(&format!("# comment\n\nfoo.example,Example.COM {KEY_A} me@box\n"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("EXAMPLE.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(
            kh.check("example.com", 22, &key(KEY_B)).unwrap(),
            HostKeyStatus::Changed { stored: fingerprints(&key(KEY_A)) }
        );
        assert_eq!(kh.check("other.example", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
        assert_eq!(kh.check("example.com", 2222, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn different_algorithm_is_unknown_not_changed() {
        let (_d, kh) = kh_with(&format!("example.com {KEY_A}\n"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_C)).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn multiple_keys_per_host() {
        let (_d, kh) = kh_with(&format!("example.com {KEY_C}\nexample.com {KEY_A}\n"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("example.com", 22, &key(KEY_C)).unwrap(), HostKeyStatus::Known);
    }

    #[test]
    fn hashed_entries_from_ssh_keygen() {
        let (_d, kh) = kh_with(&format!("{HASHED_22} {KEY_A}\n{HASHED_2222} {KEY_A}\n"));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("example.com", 2222, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert!(matches!(kh.check("example.com", 22, &key(KEY_B)).unwrap(), HostKeyStatus::Changed { .. }));
        assert_eq!(kh.check("example.org", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn wildcards_negation_and_markers() {
        let (_d, kh) = kh_with(&format!("*.example.com,!bad.example.com {KEY_A}\n@revoked * {KEY_B}\n"));
        assert_eq!(kh.check("web.example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("bad.example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Unknown);
        assert!(wildcard_match("w?b*", "web01"));
        assert!(!wildcard_match("w?b", "wb"));
    }

    #[test]
    fn add_creates_private_file_and_is_found_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ssh/known_hosts");
        let kh = KnownHosts::new(path.clone());
        kh.add("example.com", 22, &key(KEY_A), true).unwrap();
        kh.add("example.com", 2222, &key(KEY_C), false).unwrap();

        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines[0].starts_with("|1|"));
        assert!(!lines[0].contains("example.com"));
        assert!(lines[1].starts_with("[example.com]:2222 ecdsa-sha2-nistp256 "));
        assert_eq!(kh.check("example.com", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("example.com", 2222, &key(KEY_C)).unwrap(), HostKeyStatus::Known);
    }

    #[test]
    fn add_inserts_missing_trailing_newline() {
        let (_d, kh) = kh_with(&format!("other.host {KEY_B}"));
        kh.add("example.com", 22, &key(KEY_A), false).unwrap();
        let content = fs::read_to_string(&kh.path).unwrap();
        assert_eq!(content, format!("other.host {KEY_B}\nexample.com {KEY_A}\n"));
    }

    #[test]
    fn replace_removes_only_same_algorithm_entries_of_that_host() {
        let content = format!(
            "{HASHED_22} {KEY_A}\nexample.com,keep.example {KEY_A}\nexample.com {KEY_C}\nkeep.example {KEY_A}\n"
        );
        let (_d, kh) = kh_with(&content);
        kh.replace("example.com", 22, &key(KEY_B)).unwrap();

        let after = fs::read_to_string(&kh.path).unwrap();
        let lines: Vec<&str> = after.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], format!("keep.example {KEY_A}"));
        assert_eq!(lines[1], format!("example.com {KEY_C}"));
        assert_eq!(lines[2], format!("keep.example {KEY_A}"));
        assert!(lines[3].starts_with("|1|") && lines[3].contains(KEY_B));
        assert_eq!(kh.check("example.com", 22, &key(KEY_B)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("example.com", 22, &key(KEY_C)).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("keep.example", 22, &key(KEY_A)).unwrap(), HostKeyStatus::Known);
        assert_eq!(fs::metadata(&kh.path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
```

Replace `crates/core/src/lib.rs` with:

```rust
//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod error;
mod fsutil;
pub mod known_hosts;
pub mod model;
pub mod paths;
pub mod secrets;
pub mod store;

pub use error::{CoreError, NetworkKind, Result};
pub use known_hosts::{HostKeyInfo, HostKeyStatus, KnownHosts, fingerprints};
pub use model::*;
pub use paths::AppPaths;
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretKind, SecretOwner, SecretStore};
pub use store::Store;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core known_hosts`
Expected: FAIL to compile — `unresolved imports known_hosts::HostKeyInfo, ...` and `cannot find function write_private`.

- [ ] **Step 3: Implement fsutil.rs**

Insert above the `#[cfg(test)]` block in `crates/core/src/fsutil.rs`:

```rust
//! Small filesystem helpers for private (0600/0700) files and atomic writes.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

/// Creates `dir` (and missing parents) with mode 0700. Existing directories are left untouched.
pub(crate) fn create_private_dir(dir: &Path) -> Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    Ok(())
}

/// Atomically replaces `path` with `contents` (temp file + rename), mode 0600.
/// The parent directory must exist.
pub(crate) fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = temp_path(path)?;
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn temp_path(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| CoreError::internal(format!("not a file path: {}", path.display())))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    Ok(path.with_file_name(tmp_name))
}
```

- [ ] **Step 4: Implement known_hosts.rs**

Insert above the `#[cfg(test)]` block in `crates/core/src/known_hosts.rs`:

```rust
//! Reading and writing OpenSSH `known_hosts` (plain, hashed `|1|salt|hash`, `[host]:port`).

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest, Md5};
use russh::keys::{HashAlg, PublicKey};
use serde::{Deserialize, Serialize};
use sha1::Sha1;

use crate::error::{CoreError, Result};
use crate::fsutil;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyInfo {
    /// OpenSSH algorithm name, e.g. `ssh-ed25519`.
    pub algorithm: String,
    /// `SHA256:<base64 without padding>`.
    pub sha256: String,
    /// `MD5:aa:bb:...`.
    pub md5: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HostKeyStatus {
    Known,
    Unknown,
    Changed { stored: HostKeyInfo },
}

pub fn fingerprints(key: &PublicKey) -> HostKeyInfo {
    // Encoding an already parsed key into its wire blob cannot fail in practice.
    let blob = key.to_bytes().unwrap_or_default();
    let md5 = Md5::digest(&blob).iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":");
    HostKeyInfo {
        algorithm: key.algorithm().as_str().to_string(),
        sha256: key.fingerprint(HashAlg::Sha256).to_string(),
        md5: format!("MD5:{md5}"),
    }
}

pub struct KnownHosts {
    path: PathBuf,
}

impl KnownHosts {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// `Known` if any matching entry has exactly this key, `Changed` if a matching entry has
    /// a different key of the same algorithm, otherwise `Unknown`.
    pub fn check(&self, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus> {
        let content = self.read()?;
        let name = lookup_name(host, port);
        let mut changed: Option<PublicKey> = None;
        for line in content.lines() {
            let Some(entry) = parse_line(line) else { continue };
            if !patterns_match(entry.patterns, &name) {
                continue;
            }
            if entry.key.key_data() == key.key_data() {
                return Ok(HostKeyStatus::Known);
            }
            if changed.is_none() && entry.key.algorithm() == key.algorithm() {
                changed = Some(entry.key);
            }
        }
        Ok(match changed {
            Some(stored) => HostKeyStatus::Changed { stored: fingerprints(&stored) },
            None => HostKeyStatus::Unknown,
        })
    }

    /// Appends an entry. Creates the file (0600) and its directory (0700) if missing.
    pub fn add(&self, host: &str, port: u16, key: &PublicKey, hashed: bool) -> Result<()> {
        let name = lookup_name(host, port);
        let host_field = if hashed { hash_name(&name)? } else { name };
        let line = format!("{host_field} {}\n", key_text(key)?);

        if let Some(parent) = self.path.parent() {
            fsutil::create_private_dir(parent)?;
        }
        let needs_newline = match fs::read(&self.path) {
            Ok(bytes) => bytes.last().is_some_and(|b| *b != b'\n'),
            Err(err) if err.kind() == ErrorKind::NotFound => false,
            Err(err) => return Err(err.into()),
        };
        let mut file = OpenOptions::new().create(true).append(true).mode(0o600).open(&self.path)?;
        if needs_newline {
            file.write_all(b"\n")?;
        }
        file.write_all(line.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    /// Removes the entries for this host that hold a key of the same algorithm,
    /// then adds the new key as a hashed entry. Entries of other hosts are kept;
    /// on plain multi-host lines only the matching host pattern is removed.
    pub fn replace(&self, host: &str, port: u16, key: &PublicKey) -> Result<()> {
        let content = self.read()?;
        let name = lookup_name(host, port);
        let mut out = String::with_capacity(content.len());
        for line in content.lines() {
            if let Some(kept) = rewrite_line(line, &name, key) {
                out.push_str(&kept);
                out.push('\n');
            }
        }
        if out != content {
            fsutil::write_private(&self.path, out.as_bytes())?;
        }
        self.add(host, port, key, true)
    }

    fn read(&self) -> Result<String> {
        match fs::read_to_string(&self.path) {
            Ok(content) => Ok(content),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(String::new()),
            Err(err) => Err(err.into()),
        }
    }
}

struct Entry<'a> {
    patterns: &'a str,
    key: PublicKey,
}

/// Parses a known_hosts line. Comments, blank lines, marker lines
/// (`@cert-authority`, `@revoked`) and unparsable keys yield `None`.
fn parse_line(line: &str) -> Option<Entry<'_>> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('@') {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let patterns = parts.next()?;
    let algorithm = parts.next()?;
    let data = parts.next()?;
    let key = PublicKey::from_openssh(&format!("{algorithm} {data}")).ok()?;
    Some(Entry { patterns, key })
}

/// Returns the line to keep (possibly with fewer host patterns) or `None` to drop it.
fn rewrite_line(line: &str, name: &str, key: &PublicKey) -> Option<String> {
    let Some(entry) = parse_line(line) else { return Some(line.to_string()) };
    if entry.key.algorithm() != key.algorithm() || !patterns_match(entry.patterns, name) {
        return Some(line.to_string());
    }
    if entry.patterns.starts_with("|1|") {
        return None;
    }
    let remaining: Vec<&str> = entry
        .patterns
        .split(',')
        .filter(|p| p.starts_with('!') || !wildcard_match(&p.to_ascii_lowercase(), name))
        .collect();
    if remaining.iter().all(|p| p.starts_with('!')) {
        return None;
    }
    let rest = line.trim_start()[entry.patterns.len()..].to_string();
    Some(format!("{}{rest}", remaining.join(",")))
}

/// Name as stored by OpenSSH: lowercase host, `[host]:port` when port != 22.
fn lookup_name(host: &str, port: u16) -> String {
    let host = host.to_ascii_lowercase();
    if port == 22 { host } else { format!("[{host}]:{port}") }
}

fn patterns_match(patterns: &str, name: &str) -> bool {
    if let Some(rest) = patterns.strip_prefix("|1|") {
        return hashed_match(rest, name);
    }
    let mut matched = false;
    for pattern in patterns.split(',') {
        let pattern = pattern.to_ascii_lowercase();
        if let Some(negated) = pattern.strip_prefix('!') {
            if wildcard_match(negated, name) {
                return false;
            }
        } else if wildcard_match(&pattern, name) {
            matched = true;
        }
    }
    matched
}

fn hashed_match(rest: &str, name: &str) -> bool {
    let Some((salt, hash)) = rest.split_once('|') else { return false };
    let (Ok(salt), Ok(hash)) = (B64.decode(salt), B64.decode(hash)) else { return false };
    hmac_sha1(&salt, name).as_slice() == hash.as_slice()
}

fn hash_name(name: &str) -> Result<String> {
    let mut salt = [0u8; 20];
    getrandom::fill(&mut salt).map_err(|e| CoreError::internal(format!("random salt: {e}")))?;
    Ok(format!("|1|{}|{}", B64.encode(salt), B64.encode(hmac_sha1(&salt, name))))
}

fn hmac_sha1(salt: &[u8], name: &str) -> Vec<u8> {
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(salt).expect("HMAC accepts keys of any length");
    mac.update(name.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// `<algorithm> <base64 blob>` without comment.
fn key_text(key: &PublicKey) -> Result<String> {
    let blob = key.to_bytes().map_err(|e| CoreError::internal(format!("encode host key: {e}")))?;
    Ok(format!("{} {}", key.algorithm().as_str(), B64.encode(blob)))
}

/// OpenSSH-style glob: `*` matches any sequence, `?` matches one character.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut backtrack: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            backtrack = Some((pi, ti));
            pi += 1;
        } else if let Some((star_pi, star_ti)) = backtrack {
            pi = star_pi + 1;
            ti = star_ti + 1;
            backtrack = Some((star_pi, star_ti + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo fmt && cargo test -p linascp-core known_hosts && cargo test -p linascp-core fsutil && cargo clippy -p linascp-core --all-targets`
Expected: PASS — `known_hosts`: `test result: ok. 10 passed`; `fsutil`: `test result: ok. 2 passed`; clippy without warnings.

- [ ] **Step 5b: Review-focus test — IPv6 hosts**

Append to the end of `crates/core/src/known_hosts.rs`:

```rust
#[cfg(test)]
mod review_focus_tests {
    use super::*;

    const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICi2vUL2Z58TUNsW6eB7m9Vm3RgGtVqEf1QNfW24jbho";

    #[test]
    fn ipv6_hosts_use_bracket_form_with_port_and_bare_form_without() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let kh = KnownHosts::new(path.clone());
        let key = PublicKey::from_openssh(KEY_A).unwrap();

        kh.add("::1", 2222, &key, false).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("[::1]:2222 ssh-ed25519 "), "{content}");
        assert!(matches!(kh.check("::1", 2222, &key).unwrap(), HostKeyStatus::Known));
        assert!(matches!(kh.check("::1", 22, &key).unwrap(), HostKeyStatus::Unknown));

        kh.add("fe80::1", 22, &key, true).unwrap();
        assert!(matches!(kh.check("fe80::1", 22, &key).unwrap(), HostKeyStatus::Known));
    }
}
```

Run: `cargo test -p linascp-core known_hosts`
Expected: PASS — `11 passed` (if it fails, fix the `[host]:port` formatting/matching in `known_hosts.rs` so IPv6 literals are bracketed exactly like OpenSSH does — never split an IPv6 address on `:`).

- [ ] **Step 6: Commit**

```bash
git add crates/core
git commit -m "feat(core): add known_hosts check/add/replace with hashed entries" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: ssh-config import and merge

**Files:**
- Create: `crates/core/src/sshconfig/mod.rs`, `crates/core/src/sshconfig/import.rs`
- Modify: `crates/core/src/lib.rs`
- Test: unit tests inside `import.rs`

**Interfaces:**
- Consumes: `Host::new`, `AuthConfig`, `AuthMethod`, `HostAuth`, `HostSource`, `Id`, `CoreError::config`; `ssh2_config::{SshConfig, ParseRule}` (0.8: `SshConfig::default().parse(&mut impl BufRead, ParseRule)`, `get_hosts()`, `query(alias) -> HostParams`)
- Produces:
  - `pub fn import_ssh_config(path: &Path) -> Result<Vec<Host>>` — missing file → `Ok(vec![])`; parse error → `CoreError::Config`
  - `pub fn merge_hosts(stored: Vec<Host>, imported: Vec<Host>) -> Vec<Host>` — stored host with the same `name` wins; sorted by name case-insensitively
  - `pub fn import_id(alias: &str) -> Id` — `uuid v5(NAMESPACE_URL, "ssh-config:" + alias)`
  - Mapping: one host per concrete alias of every `Host` line (patterns with `*`, `?`, `!` or negation are skipped; `Host web web-alias` yields two hosts), parameters resolved with `query(alias)` (first match wins, `Host *` defaults apply); `address` = `HostName` or alias; `port` = `Port` or 22; `user` = `User`; first `IdentityFile` → `Custom{Key, key_path}` (ssh2-config expands `~`), otherwise `Custom{Agent}`; `ServerAliveInterval` → `keepalive_s`; `ConnectTimeout` → `timeout_s`; a single-hop `ProxyJump` whose alias (after stripping `user@` and `:port`) is imported from the same file → `jump_host_id = import_id(alias)`; any other `ProxyJump` (multi-hop or unknown host, except `none`) → `notes = "ProxyJump <value> (not imported)"`; `source = SshConfig`; `created_at = updated_at = 0`.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/sshconfig/import.rs` (tests only for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\
Host *
    ServerAliveInterval 45

Host bastion
    HostName bastion.example.com
    User jump
    IdentityFile /keys/id_bastion

Host web web-alias
    HostName 10.0.0.5
    Port 2222
    User deploy
    ProxyJump bastion

Host chained
    HostName 10.0.0.6
    ProxyJump bastion,web

Host db-* !db-old
    User dbadmin

Host plain
    UnknownOption yes
";

    fn import(content: &str) -> Vec<Host> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        std::fs::write(&path, content).unwrap();
        import_ssh_config(&path).unwrap()
    }

    fn by_name<'a>(hosts: &'a [Host], name: &str) -> &'a Host {
        hosts.iter().find(|h| h.name == name).unwrap()
    }

    #[test]
    fn imports_concrete_aliases_only() {
        let hosts = import(CONFIG);
        let names: Vec<&str> = hosts.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["bastion", "web", "web-alias", "chained", "plain"]);
        assert!(hosts.iter().all(|h| h.source == HostSource::SshConfig));
    }

    #[test]
    fn maps_effective_parameters() {
        let hosts = import(CONFIG);
        let bastion = by_name(&hosts, "bastion");
        assert_eq!(bastion.id, import_id("bastion"));
        assert_eq!(bastion.address, "bastion.example.com");
        assert_eq!(bastion.port, 22);
        assert_eq!(bastion.user.as_deref(), Some("jump"));
        assert_eq!(bastion.keepalive_s, 45);
        assert_eq!(
            bastion.auth,
            HostAuth::Custom {
                auth: AuthConfig { method: AuthMethod::Key, key_path: Some("/keys/id_bastion".into()), has_secret: false }
            }
        );

        let web = by_name(&hosts, "web");
        assert_eq!(web.address, "10.0.0.5");
        assert_eq!(web.port, 2222);
        assert_eq!(web.auth, HostAuth::Custom { auth: AuthConfig::new(AuthMethod::Agent) });
        assert_eq!(web.jump_host_id, Some(import_id("bastion")));

        let plain = by_name(&hosts, "plain");
        assert_eq!(plain.address, "plain");
        assert!(plain.user.is_none());
    }

    #[test]
    fn multi_hop_proxy_jump_is_noted_not_linked() {
        let hosts = import(CONFIG);
        let chained = by_name(&hosts, "chained");
        assert_eq!(chained.jump_host_id, None);
        assert_eq!(chained.notes, "ProxyJump bastion,web (not imported)");
    }

    #[test]
    fn ids_are_deterministic() {
        assert_eq!(import_id("web"), import_id("web"));
        assert_ne!(import_id("web"), import_id("web2"));
        assert_eq!(import_id("web").get_version_num(), 5);
    }

    #[test]
    fn missing_file_yields_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(import_ssh_config(&dir.path().join("none")).unwrap().is_empty());
    }

    #[test]
    fn jump_alias_strips_user_and_port() {
        assert_eq!(jump_alias("me@bastion:2200"), "bastion");
        assert_eq!(jump_alias("bastion"), "bastion");
    }

    #[test]
    fn merge_prefers_stored_hosts_and_sorts_by_name() {
        let stored = vec![Host::new("web", "override.example"), Host::new("Zeta", "z")];
        let imported = import(CONFIG);
        let merged = merge_hosts(stored, imported);
        let names: Vec<&str> = merged.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["bastion", "chained", "plain", "web", "web-alias", "Zeta"]);
        let web = by_name(&merged, "web");
        assert_eq!(web.address, "override.example");
        assert_eq!(web.source, HostSource::Linascp);
    }
}
```

`crates/core/src/sshconfig/mod.rs`:

```rust
//! `~/.ssh/config` integration: read-only import and mirror mode.

pub mod import;

pub use import::{import_id, import_ssh_config, merge_hosts};
```

Replace `crates/core/src/lib.rs` with:

```rust
//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod error;
mod fsutil;
pub mod known_hosts;
pub mod model;
pub mod paths;
pub mod secrets;
pub mod sshconfig;
pub mod store;

pub use error::{CoreError, NetworkKind, Result};
pub use known_hosts::{HostKeyInfo, HostKeyStatus, KnownHosts, fingerprints};
pub use model::*;
pub use paths::AppPaths;
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretKind, SecretOwner, SecretStore};
pub use sshconfig::{import_ssh_config, merge_hosts};
pub use store::Store;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core sshconfig`
Expected: FAIL to compile — `unresolved imports import::import_id, import::import_ssh_config, import::merge_hosts`.

- [ ] **Step 3: Implement import.rs**

Insert above the `#[cfg(test)]` block in `crates/core/src/sshconfig/import.rs`:

```rust
//! Read-only import of `~/.ssh/config` host aliases.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, ErrorKind};
use std::path::Path;

use ssh2_config::{ParseRule, SshConfig};

use crate::error::{CoreError, Result};
use crate::model::{AuthConfig, AuthMethod, Host, HostAuth, HostSource, Id};

/// Deterministic id of an imported alias: uuid v5(NAMESPACE_URL, "ssh-config:" + alias).
pub fn import_id(alias: &str) -> Id {
    Id::new_v5(&Id::NAMESPACE_URL, format!("ssh-config:{alias}").as_bytes())
}

/// Imports every concrete `Host` alias (no wildcards, no negation) with its effective
/// settings. A missing file yields an empty list.
pub fn import_ssh_config(path: &Path) -> Result<Vec<Host>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut reader = BufReader::new(file);
    let config = SshConfig::default()
        .parse(&mut reader, ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS)
        .map_err(|e| CoreError::config(format!("cannot parse {}: {e}", path.display())))?;

    let aliases = concrete_aliases(&config);
    let ids: HashMap<&str, Id> = aliases.iter().map(|a| (a.as_str(), import_id(a))).collect();
    Ok(aliases.iter().map(|alias| to_host(&config, alias, &ids)).collect())
}

/// Stored hosts win over imported hosts with the same name. Result is sorted by name
/// (case-insensitive).
pub fn merge_hosts(stored: Vec<Host>, imported: Vec<Host>) -> Vec<Host> {
    let names: HashSet<String> = stored.iter().map(|h| h.name.clone()).collect();
    let mut merged = stored;
    merged.extend(imported.into_iter().filter(|h| !names.contains(&h.name)));
    merged.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
    merged
}

fn concrete_aliases(config: &SshConfig) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut aliases = Vec::new();
    for host in config.get_hosts() {
        for clause in &host.pattern {
            let pattern = clause.pattern.as_str();
            let concrete = !clause.negated && !pattern.is_empty() && !pattern.contains(['*', '?', '!']);
            if concrete && seen.insert(pattern.to_string()) {
                aliases.push(pattern.to_string());
            }
        }
    }
    aliases
}

fn to_host(config: &SshConfig, alias: &str, ids: &HashMap<&str, Id>) -> Host {
    let params = config.query(alias);
    let mut host = Host::new(alias, params.host_name.as_deref().unwrap_or(alias));
    host.id = ids[alias];
    host.source = HostSource::SshConfig;
    host.created_at = 0;
    host.updated_at = 0;
    host.port = params.port.unwrap_or(22);
    host.user = params.user.clone();
    if let Some(interval) = params.server_alive_interval {
        host.keepalive_s = interval.as_secs() as u32;
    }
    if let Some(timeout) = params.connect_timeout {
        host.timeout_s = timeout.as_secs() as u32;
    }
    let identity = params.identity_file.as_ref().and_then(|files| files.first());
    host.auth = HostAuth::Custom {
        auth: match identity {
            Some(path) => {
                AuthConfig { method: AuthMethod::Key, key_path: Some(path.to_string_lossy().into_owned()), has_secret: false }
            }
            None => AuthConfig::new(AuthMethod::Agent),
        },
    };
    if let Some(jumps) = &params.proxy_jump {
        let resolved = match jumps.as_slice() {
            [single] => ids.get(jump_alias(single)).copied(),
            _ => None,
        };
        match resolved {
            Some(id) => host.jump_host_id = Some(id),
            None if jumps.iter().all(|j| j.eq_ignore_ascii_case("none")) => {}
            None => host.notes = format!("ProxyJump {} (not imported)", jumps.join(",")),
        }
    }
    host
}

/// `user@alias:port` -> `alias`.
fn jump_alias(jump: &str) -> &str {
    let without_user = jump.rsplit_once('@').map_or(jump, |(_, rest)| rest);
    without_user.split_once(':').map_or(without_user, |(alias, _)| alias)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo fmt && cargo test -p linascp-core sshconfig && cargo clippy -p linascp-core --all-targets`
Expected: PASS — `test result: ok. 7 passed`; clippy without warnings.

- [ ] **Step 4b: Review-focus test — real-world configs with `Include` and `Match`**

Append to the end of `crates/core/src/sshconfig/import.rs`:

```rust
#[cfg(test)]
mod review_focus_tests {
    use super::*;

    #[test]
    fn config_with_include_and_match_imports_without_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("extra.conf"), "Host extra\n    HostName 10.0.0.9\n").unwrap();
        let config = format!(
            "Include {}/extra.conf\n\nHost main\n    HostName 10.0.0.1\n\nMatch host *.internal\n    User ops\n",
            dir.path().display()
        );
        let path = dir.path().join("config");
        std::fs::write(&path, config).unwrap();

        let hosts = import_ssh_config(&path).expect("Include/Match must not break the import");
        assert!(hosts.iter().any(|h| h.name == "main" && h.address == "10.0.0.1"));
    }
}
```

Run: `cargo test -p linascp-core sshconfig`
Expected: PASS — `8 passed`. If ssh2-config rejects the `Match` block, add this helper to `import.rs`, read the file into a `String`, pass `strip_match_blocks(&content)` as the reader (`&mut content.as_bytes()`) with the same `ParseRule` flags, and re-run:

```rust
/// ssh2-config may reject `Match` blocks; LinaSCP only imports `Host` blocks, so drop them.
fn strip_match_blocks(content: &str) -> String {
    let mut out = String::new();
    let mut in_match = false;
    for line in content.lines() {
        let first = line.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        if first == "match" {
            in_match = true;
            continue;
        }
        if first == "host" {
            in_match = false;
        }
        if !in_match {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}
```

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): import hosts from ~/.ssh/config and merge with stored hosts" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: ssh-config mirror mode

**Files:**
- Create: `crates/core/src/sshconfig/mirror.rs`
- Modify: `crates/core/src/sshconfig/mod.rs`, `crates/core/src/lib.rs`
- Test: unit tests inside `mirror.rs`

**Interfaces:**
- Consumes: `Host`, `LoginProfile`, `HostAuth`, `AuthConfig`, `AuthMethod`, `HostSource`; `fsutil::{create_private_dir, write_private}` (Task 4)
- Produces:
  - `pub const INCLUDE_LINE: &str = "Include ~/.ssh/config.d/linascp.conf";`
  - `pub struct Mirror` with `new(ssh_dir: PathBuf)`, `render(&[Host], &[LoginProfile]) -> String`, `enable`, `write`, `disable`, `is_enabled` exactly as in the contract
  - Render format: first line `# generated by LinaSCP – do not edit`; per `source == Linascp` host a blank line, `Host <name with whitespace runs replaced by '-'>`, `    HostName`, `    User` (host.user, else the profile's user; omitted if neither), `    Port` (always), `    IdentityFile` (only for `Key` auth with a path, from the host or its profile), `    ProxyJump <jump host alias>` (jump host looked up by id in the given list — **callers pass the merged host list** so jump hosts from `~/.ssh/config` resolve). Values containing whitespace are double-quoted.
  - `enable`: creates `ssh_dir` (0700); copies an existing `config` to `config.linascp.bak` only if no backup exists yet; prepends `INCLUDE_LINE` unless a line equal to it (trimmed) is already present; creates `config` (0600) if missing; then `write`. A symlinked `config` (dotfile managers) is resolved and the link target is edited, the link stays.
  - `write`: creates `config.d` (0700) and atomically writes `config.d/linascp.conf` (0600).
  - `disable`: removes every `INCLUDE_LINE` line from `config` and deletes `config.d/linascp.conf` (missing files are fine); the backup is kept.
  - `is_enabled`: include line present **and** mirror file exists.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/sshconfig/mirror.rs` (tests only for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Id;
    use std::os::unix::fs::PermissionsExt;

    fn sample() -> (Vec<Host>, Vec<LoginProfile>) {
        let profile = LoginProfile::new(
            "ops",
            "opsuser",
            AuthConfig { method: AuthMethod::Key, key_path: Some("/keys/ops key".into()), has_secret: false },
        );
        let mut bastion = Host::new("bastion", "bastion.example.com");
        bastion.source = HostSource::SshConfig;
        bastion.id = Id::new_v4();
        let mut web = Host::new("Web Server 1", "10.0.0.5");
        web.port = 2222;
        web.auth = HostAuth::Profile { profile_id: profile.id };
        web.jump_host_id = Some(bastion.id);
        let mut db = Host::new("db", "db.internal");
        db.user = Some("postgres".into());
        (vec![bastion, web, db], vec![profile])
    }

    #[test]
    fn render_writes_linascp_hosts_only() {
        let (hosts, profiles) = sample();
        let expected = "\
# generated by LinaSCP – do not edit

Host Web-Server-1
    HostName 10.0.0.5
    User opsuser
    Port 2222
    IdentityFile \"/keys/ops key\"
    ProxyJump bastion

Host db
    HostName db.internal
    User postgres
    Port 22
";
        assert_eq!(Mirror::render(&hosts, &profiles), expected);
    }

    #[test]
    fn enable_backs_up_prepends_include_once_and_writes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh = tmp.path().join(".ssh");
        fs::create_dir(&ssh).unwrap();
        fs::write(ssh.join("config"), "Host old\n    HostName old.example\n").unwrap();
        let (hosts, profiles) = sample();
        let mirror = Mirror::new(ssh.clone());
        assert!(!mirror.is_enabled());

        mirror.enable(&hosts, &profiles).unwrap();
        mirror.enable(&hosts, &profiles).unwrap();

        let config = fs::read_to_string(ssh.join("config")).unwrap();
        assert_eq!(config, format!("{INCLUDE_LINE}\nHost old\n    HostName old.example\n"));
        assert_eq!(fs::read_to_string(ssh.join("config.linascp.bak")).unwrap(), "Host old\n    HostName old.example\n");
        let file = ssh.join("config.d/linascp.conf");
        assert_eq!(fs::read_to_string(&file).unwrap(), Mirror::render(&hosts, &profiles));
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(ssh.join("config.d")).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(mirror.is_enabled());
    }

    #[test]
    fn enable_creates_missing_config_and_ssh_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh = tmp.path().join(".ssh");
        let mirror = Mirror::new(ssh.clone());
        mirror.enable(&[], &[]).unwrap();
        assert_eq!(fs::read_to_string(ssh.join("config")).unwrap(), format!("{INCLUDE_LINE}\n"));
        assert!(!ssh.join("config.linascp.bak").exists());
        assert_eq!(fs::metadata(&ssh).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(mirror.is_enabled());
    }

    #[test]
    fn write_regenerates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mirror = Mirror::new(tmp.path().to_path_buf());
        mirror.enable(&[], &[]).unwrap();
        let (hosts, profiles) = sample();
        mirror.write(&hosts, &profiles).unwrap();
        let content = fs::read_to_string(tmp.path().join("config.d/linascp.conf")).unwrap();
        assert!(content.contains("Host db\n"));
    }

    #[test]
    fn disable_removes_include_and_file_but_keeps_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh = tmp.path().to_path_buf();
        fs::write(ssh.join("config"), "Host old\n").unwrap();
        let mirror = Mirror::new(ssh.clone());
        mirror.enable(&[], &[]).unwrap();

        mirror.disable().unwrap();

        assert_eq!(fs::read_to_string(ssh.join("config")).unwrap(), "Host old\n");
        assert!(!ssh.join("config.d/linascp.conf").exists());
        assert!(!mirror.is_enabled());
        mirror.disable().unwrap();
    }

    #[test]
    fn enable_follows_symlinked_config() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh = tmp.path().join(".ssh");
        fs::create_dir(&ssh).unwrap();
        let real = tmp.path().join("dotfiles-config");
        fs::write(&real, "Host x\n").unwrap();
        std::os::unix::fs::symlink(&real, ssh.join("config")).unwrap();

        Mirror::new(ssh.clone()).enable(&[], &[]).unwrap();

        assert!(fs::symlink_metadata(ssh.join("config")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), format!("{INCLUDE_LINE}\nHost x\n"));
    }
}
```

Replace `crates/core/src/sshconfig/mod.rs` with:

```rust
//! `~/.ssh/config` integration: read-only import and mirror mode.

pub mod import;
pub mod mirror;

pub use import::{import_id, import_ssh_config, merge_hosts};
pub use mirror::{INCLUDE_LINE, Mirror};
```

In `crates/core/src/lib.rs` replace the line `pub use sshconfig::{import_ssh_config, merge_hosts};` with:

```rust
pub use sshconfig::{INCLUDE_LINE, Mirror, import_ssh_config, merge_hosts};
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core mirror`
Expected: FAIL to compile — `unresolved imports mirror::INCLUDE_LINE, mirror::Mirror`.

- [ ] **Step 3: Implement mirror.rs**

Insert above the `#[cfg(test)]` block in `crates/core/src/sshconfig/mirror.rs`:

```rust
//! Mirror mode: LinaSCP hosts are written to `~/.ssh/config.d/linascp.conf`,
//! which is included from the top of `~/.ssh/config`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::fsutil;
use crate::model::{AuthConfig, AuthMethod, Host, HostAuth, HostSource, LoginProfile};

pub const INCLUDE_LINE: &str = "Include ~/.ssh/config.d/linascp.conf";
const HEADER: &str = "# generated by LinaSCP – do not edit";

pub struct Mirror {
    ssh_dir: PathBuf,
}

impl Mirror {
    pub fn new(ssh_dir: PathBuf) -> Self {
        Self { ssh_dir }
    }

    /// Renders all `source == Linascp` hosts. Pass the full (merged) host list so that
    /// `ProxyJump` can reference jump hosts that come from `~/.ssh/config`.
    pub fn render(hosts: &[Host], profiles: &[LoginProfile]) -> String {
        let mut out = format!("{HEADER}\n");
        for host in hosts.iter().filter(|h| h.source == HostSource::Linascp) {
            let profile = match &host.auth {
                HostAuth::Profile { profile_id } => profiles.iter().find(|p| p.id == *profile_id),
                HostAuth::Custom { .. } => None,
            };
            let auth: Option<&AuthConfig> = match &host.auth {
                HostAuth::Profile { .. } => profile.map(|p| &p.auth),
                HostAuth::Custom { auth } => Some(auth),
            };
            let user = host
                .user
                .clone()
                .filter(|u| !u.is_empty())
                .or_else(|| profile.map(|p| p.user.clone()).filter(|u| !u.is_empty()));

            out.push('\n');
            out.push_str(&format!("Host {}\n", host_alias(&host.name)));
            out.push_str(&format!("    HostName {}\n", quote(&host.address)));
            if let Some(user) = user {
                out.push_str(&format!("    User {}\n", quote(&user)));
            }
            out.push_str(&format!("    Port {}\n", host.port));
            if let Some(AuthConfig { method: AuthMethod::Key, key_path: Some(path), .. }) = auth {
                out.push_str(&format!("    IdentityFile {}\n", quote(path)));
            }
            if let Some(jump) = host.jump_host_id.and_then(|id| hosts.iter().find(|h| h.id == id)) {
                out.push_str(&format!("    ProxyJump {}\n", host_alias(&jump.name)));
            }
        }
        out
    }

    /// Backs up `config` once to `config.linascp.bak`, prepends the include line if absent
    /// (creating `config` if needed) and writes the mirror file.
    pub fn enable(&self, hosts: &[Host], profiles: &[LoginProfile]) -> Result<()> {
        fsutil::create_private_dir(&self.ssh_dir)?;
        let config = self.config_target();
        let existing = read_optional(&config)?;
        let backup = self.ssh_dir.join("config.linascp.bak");
        if existing.is_some() && !backup.exists() {
            fs::copy(&config, &backup)?;
        }
        let content = existing.unwrap_or_default();
        if !has_include(&content) {
            fsutil::write_private(&config, format!("{INCLUDE_LINE}\n{content}").as_bytes())?;
        }
        self.write(hosts, profiles)
    }

    /// Regenerates `config.d/linascp.conf` only.
    pub fn write(&self, hosts: &[Host], profiles: &[LoginProfile]) -> Result<()> {
        let dir = self.ssh_dir.join("config.d");
        fsutil::create_private_dir(&dir)?;
        fsutil::write_private(&self.mirror_file(), Self::render(hosts, profiles).as_bytes())
    }

    /// Removes the include line from `config` and deletes the mirror file.
    pub fn disable(&self) -> Result<()> {
        let config = self.config_target();
        if let Some(content) = read_optional(&config)?
            && has_include(&content)
        {
            let kept: String =
                content.lines().filter(|line| line.trim() != INCLUDE_LINE).map(|line| format!("{line}\n")).collect();
            fsutil::write_private(&config, kept.as_bytes())?;
        }
        match fs::remove_file(self.mirror_file()) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// True when the include line is present and the mirror file exists.
    pub fn is_enabled(&self) -> bool {
        let included = matches!(read_optional(&self.config_target()), Ok(Some(content)) if has_include(&content));
        included && self.mirror_file().is_file()
    }

    fn mirror_file(&self) -> PathBuf {
        self.ssh_dir.join("config.d").join("linascp.conf")
    }

    /// `~/.ssh/config`, resolved through a symlink (dotfile managers) so it is not replaced.
    fn config_target(&self) -> PathBuf {
        let config = self.ssh_dir.join("config");
        fs::canonicalize(&config).unwrap_or(config)
    }
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn has_include(content: &str) -> bool {
    content.lines().any(|line| line.trim() == INCLUDE_LINE)
}

/// ssh_config `Host` aliases cannot contain whitespace.
fn host_alias(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join("-")
}

fn quote(value: &str) -> String {
    if value.contains(char::is_whitespace) { format!("\"{value}\"") } else { value.to_string() }
}
```

(`if let ... && ...` let-chains require edition 2024, set in Task 1.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo fmt && cargo test -p linascp-core mirror && cargo clippy -p linascp-core --all-targets`
Expected: PASS — `test result: ok. 6 passed`; clippy without warnings.

Optional manual check that OpenSSH accepts the output: copy the expected string from `render_writes_linascp_hosts_only` into `/tmp/m.conf` and run `ssh -G -F /tmp/m.conf Web-Server-1 | grep -E '^(hostname|user|port|identityfile|proxyjump) '` → `hostname 10.0.0.5`, `user opsuser`, `port 2222`, `identityfile /keys/ops key`, `proxyjump bastion`.

- [ ] **Step 4b: Review-focus test — names with spaces and umlauts**

Append to the end of `crates/core/src/sshconfig/mirror.rs`:

```rust
#[cfg(test)]
mod review_focus_tests {
    use super::*;
    use crate::model::Host;

    #[test]
    fn render_sanitizes_whitespace_and_keeps_unicode_names() {
        let mut host = Host::new("Kunde Müller  Shop", "shop.example.de");
        host.user = Some("deploy".into());
        let out = Mirror::render(&[host], &[]);
        assert!(out.contains("Host Kunde-Müller-Shop\n"), "{out}");
        assert!(out.contains("HostName shop.example.de"), "{out}");
    }
}
```

Run: `cargo test -p linascp-core sshconfig::mirror`
Expected: PASS — `7 passed` (6 from Step 4 + this one).

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): add ssh-config mirror mode with include line and backup" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Export / import (`.linascp`, age-encrypted secrets)

**Files:**
- Create: `crates/core/src/export.rs`
- Test: `crates/core/src/export/tests.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Store` (+ setting consts), `SecretStore`, `SecretOwner`, `SecretKind`, `account_name`, `parse_account_name`, model types, `fsutil` (Tasks 2–4); `age` 0.11 (`Encryptor::with_user_passphrase`, `Decryptor::new`, `scrypt::Identity`, `armor::{ArmoredWriter, ArmoredReader, Format}`)
- Produces (exactly as in the contract): `FORMAT_VERSION`, `ExportOptions`, `export_data`, `ItemKind`, `Conflict`, `ImportItem`, `ImportPreview`, `Resolution`, `ImportReport`, `import_preview`, `import_apply`
- File format (JSON, camelCase): `{ "formatVersion": 1, "exportedAt": <unix seconds>, "groups": [...], "profiles": [...], "hosts": [...], "settings": { "theme": ..., "terminal_renderer": ..., "terminal_font_size": ... }, "secrets"?: "<ASCII-armored age block>" }`. The age plaintext is `{ "secrets": { "<account_name>": "<value>" }, "keyFiles": { "<original path>": "<base64>" } }`.
- Behaviour:
  - `export_data`: `include_secrets` requires a non-empty passphrase, `include_key_files` requires `include_secrets` (else `CoreError::Config`); collects `Password` and `Passphrase` secrets of every host and profile from the `SecretStore`; key files = `key_path` of `Key` auths (hosts with custom auth + profiles), missing files are skipped. Exported settings: `theme`, `terminal_renderer`, `terminal_font_size` (`ssh_mode` is excluded because switching it needs the mirror confirmation flow; `first_run_done` is device-local). **The caller writes the returned bytes with mode 0600.**
  - `import_preview`: per item (groups, then profiles, then hosts) `conflict = SameId` if the same kind with that id exists, else `SameName` if the same kind with that name exists, else `None`; `has_secrets` = file contains a secrets block. Invalid JSON or `formatVersion > 1` → `CoreError::Config`.
  - `import_apply`: decrypts first (wrong passphrase → `CoreError::Config { message: "wrong passphrase" }`, nothing is written). Items without conflict are inserted as-is; conflicting items use `resolutions[item.id]`, default `Skip`. `Replace` overwrites — for `SameName` the **existing id is kept** and the imported data is written under it. `Duplicate` gets a new uuid v4 and the name suffix `" (Import)"`. References (`group_id`, `jump_host_id`, `HostAuth::Profile.profile_id`) are remapped: to the new id for duplicates, to the existing item for skipped/replaced `SameName` conflicts, unchanged otherwise (e.g. ssh-config jump hosts). Port-forward ids are regenerated when a host's id changes. Secrets are restored for written items only, with remapped owner ids (`secrets_restored` counts them). Key files are written to `key_dir` (0700) as 0600 files named after the original file name (`name`, `name-1`, ... ; identical content is reused) and `key_path`s are rewritten. Without passphrase/secrets block, `has_secret` is cleared on imported auths. Exported settings are applied. `imported`/`skipped` count groups, profiles and hosts.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/export/tests.rs`:

```rust
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::model::{ForwardKind, PortForward};
use crate::secrets::MemorySecretStore;

fn pass(s: &str) -> SecretString {
    SecretString::from(s.to_string())
}

fn no_secrets() -> ExportOptions {
    ExportOptions { include_secrets: false, include_key_files: false, passphrase: None }
}

fn with_secrets(passphrase: &str, key_files: bool) -> ExportOptions {
    ExportOptions { include_secrets: true, include_key_files: key_files, passphrase: Some(pass(passphrase)) }
}

struct Source {
    store: Store,
    secrets: MemorySecretStore,
    group: Group,
    profile: LoginProfile,
    host: Host,
}

/// group + profile (password secret) + host (uses group, profile, forward) + key host.
fn source(key_path: Option<&Path>) -> Source {
    let store = Store::open_in_memory().unwrap();
    let secrets = MemorySecretStore::new();
    let group = Group::new("Prod", 0);
    let profile =
        LoginProfile::new("admin", "root", AuthConfig { method: AuthMethod::Password, key_path: None, has_secret: true });
    let mut host = Host::new("web", "10.0.0.5");
    host.group_id = Some(group.id);
    host.auth = HostAuth::Profile { profile_id: profile.id };
    host.forwards = vec![PortForward {
        id: Id::new_v4(),
        kind: ForwardKind::Local,
        bind: "8080".into(),
        target: "localhost:80".into(),
        autostart: false,
    }];
    store.save_group(&group).unwrap();
    store.save_profile(&profile).unwrap();
    store.save_host(&host).unwrap();
    secrets.set(SecretOwner::Profile(profile.id), SecretKind::Password, &pass("s3cret")).unwrap();
    if let Some(path) = key_path {
        let mut key_host = Host::new("keyhost", "10.0.0.6");
        key_host.auth = HostAuth::Custom {
            auth: AuthConfig {
                method: AuthMethod::Key,
                key_path: Some(path.to_string_lossy().into_owned()),
                has_secret: true,
            },
        };
        store.save_host(&key_host).unwrap();
        secrets.set(SecretOwner::Host(key_host.id), SecretKind::Passphrase, &pass("keypass")).unwrap();
    }
    store.set_setting(store::THEME, "light").unwrap();
    store.set_setting(store::SSH_MODE, "mirror").unwrap();
    Source { store, secrets, group, profile, host }
}

#[test]
fn roundtrip_without_secrets_into_empty_store() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["formatVersion"], 1);
    assert!(json.get("secrets").is_none());
    assert!(!String::from_utf8_lossy(&bytes).contains("s3cret"));

    let dst = Store::open_in_memory().unwrap();
    let dst_secrets = MemorySecretStore::new();
    let preview = import_preview(&bytes, &dst).unwrap();
    assert!(!preview.has_secrets);
    assert_eq!(preview.items.len(), 3);
    assert!(preview.items.iter().all(|i| i.conflict.is_none()));

    let tmp = tempfile::tempdir().unwrap();
    let report = import_apply(&bytes, None, &HashMap::new(), &dst, &dst_secrets, tmp.path()).unwrap();
    assert_eq!(report, ImportReport { imported: 3, skipped: 0, secrets_restored: 0 });

    let host = dst.get_host(src.host.id).unwrap().unwrap();
    assert_eq!(host.group_id, Some(src.group.id));
    assert_eq!(host.auth, HostAuth::Profile { profile_id: src.profile.id });
    assert_eq!(host.forwards, src.host.forwards);
    let profile = dst.get_profile(src.profile.id).unwrap().unwrap();
    assert!(!profile.auth.has_secret, "has_secret cleared when no secrets were restored");
    assert_eq!(dst.get_setting(store::THEME).unwrap().as_deref(), Some("light"));
    assert_eq!(dst.get_setting(store::SSH_MODE).unwrap(), None);
    assert!(dst_secrets.is_empty());
}

#[test]
fn export_with_secrets_requires_passphrase() {
    let src = source(None);
    let opts = ExportOptions { include_secrets: true, include_key_files: false, passphrase: None };
    assert!(matches!(export_data(&src.store, &src.secrets, &opts), Err(CoreError::Config { .. })));
    let opts = ExportOptions { include_secrets: false, include_key_files: true, passphrase: None };
    assert!(matches!(export_data(&src.store, &src.secrets, &opts), Err(CoreError::Config { .. })));
}

#[test]
fn roundtrip_with_secrets_and_key_files() {
    let tmp = tempfile::tempdir().unwrap();
    let key_src = tmp.path().join("id_test");
    fs::write(&key_src, b"PRIVATE KEY DATA").unwrap();
    let src = source(Some(&key_src));

    let bytes = export_data(&src.store, &src.secrets, &with_secrets("correct horse", true)).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("-----BEGIN AGE ENCRYPTED FILE-----"));
    assert!(!text.contains("s3cret") && !text.contains("PRIVATE KEY DATA"));

    let dst = Store::open_in_memory().unwrap();
    let dst_secrets = MemorySecretStore::new();
    assert!(import_preview(&bytes, &dst).unwrap().has_secrets);
    let key_dir = tmp.path().join("imported");
    let report =
        import_apply(&bytes, Some(&pass("correct horse")), &HashMap::new(), &dst, &dst_secrets, &key_dir).unwrap();
    assert_eq!(report, ImportReport { imported: 4, skipped: 0, secrets_restored: 2 });

    let restored = dst_secrets.get(SecretOwner::Profile(src.profile.id), SecretKind::Password).unwrap().unwrap();
    assert_eq!(restored.expose_secret(), "s3cret");

    let key_host = dst.list_hosts().unwrap().into_iter().find(|h| h.name == "keyhost").unwrap();
    let HostAuth::Custom { auth } = &key_host.auth else { panic!("expected custom auth") };
    let new_path = PathBuf::from(auth.key_path.clone().unwrap());
    assert_eq!(new_path, key_dir.join("id_test"));
    assert_eq!(fs::read(&new_path).unwrap(), b"PRIVATE KEY DATA");
    assert_eq!(fs::metadata(&new_path).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(auth.has_secret);
    let passphrase = dst_secrets.get(SecretOwner::Host(key_host.id), SecretKind::Passphrase).unwrap().unwrap();
    assert_eq!(passphrase.expose_secret(), "keypass");
}

#[test]
fn wrong_passphrase_is_config_error_and_imports_nothing() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &with_secrets("right", false)).unwrap();
    let dst = Store::open_in_memory().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let err = import_apply(&bytes, Some(&pass("wrong")), &HashMap::new(), &dst, &MemorySecretStore::new(), tmp.path())
        .unwrap_err();
    assert_eq!(err, CoreError::config("wrong passphrase"));
    assert!(dst.list_hosts().unwrap().is_empty());
}

#[test]
fn invalid_file_is_config_error() {
    let dst = Store::open_in_memory().unwrap();
    assert!(matches!(import_preview(b"not json", &dst), Err(CoreError::Config { .. })));
    let future = br#"{"formatVersion":99,"exportedAt":0,"groups":[],"profiles":[],"hosts":[],"settings":{}}"#;
    assert!(matches!(import_preview(future, &dst), Err(CoreError::Config { .. })));
}

#[test]
fn preview_detects_same_id_before_same_name() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    // Importing into the source store: every item conflicts by id.
    let preview = import_preview(&bytes, &src.store).unwrap();
    assert!(preview.items.iter().all(|i| i.conflict == Some(Conflict::SameId)));

    let other = Store::open_in_memory().unwrap();
    other.save_host(&Host::new("web", "elsewhere")).unwrap();
    let preview = import_preview(&bytes, &other).unwrap();
    let web = preview.items.iter().find(|i| i.kind == ItemKind::Host).unwrap();
    assert_eq!(web.conflict, Some(Conflict::SameName));
    let group = preview.items.iter().find(|i| i.kind == ItemKind::Group).unwrap();
    assert_eq!(group.conflict, None);
}

#[test]
fn conflicts_default_to_skip() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    let mut changed = src.host.clone();
    changed.address = "changed".into();
    src.store.save_host(&changed).unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let report = import_apply(&bytes, None, &HashMap::new(), &src.store, &src.secrets, tmp.path()).unwrap();

    assert_eq!(report, ImportReport { imported: 0, skipped: 3, secrets_restored: 0 });
    assert_eq!(src.store.get_host(src.host.id).unwrap().unwrap().address, "changed");
}

#[test]
fn replace_same_id_overwrites() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    let mut changed = src.host.clone();
    changed.address = "changed".into();
    src.store.save_host(&changed).unwrap();
    let resolutions = HashMap::from([(src.host.id, Resolution::Replace)]);
    let tmp = tempfile::tempdir().unwrap();

    let report = import_apply(&bytes, None, &resolutions, &src.store, &src.secrets, tmp.path()).unwrap();

    assert_eq!(report, ImportReport { imported: 1, skipped: 2, secrets_restored: 0 });
    assert_eq!(src.store.get_host(src.host.id).unwrap().unwrap().address, "10.0.0.5");
}

#[test]
fn replace_same_name_keeps_existing_id() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &no_secrets()).unwrap();
    let dst = Store::open_in_memory().unwrap();
    let existing = Host::new("web", "elsewhere");
    dst.save_host(&existing).unwrap();
    let resolutions = HashMap::from([(src.host.id, Resolution::Replace)]);
    let tmp = tempfile::tempdir().unwrap();

    import_apply(&bytes, None, &resolutions, &dst, &MemorySecretStore::new(), tmp.path()).unwrap();

    let hosts = dst.list_hosts().unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].id, existing.id);
    assert_eq!(hosts[0].address, "10.0.0.5");
    assert_eq!(hosts[0].auth, HostAuth::Profile { profile_id: src.profile.id });
}

#[test]
fn duplicate_gets_new_ids_suffix_and_remapped_secrets() {
    let src = source(None);
    let bytes = export_data(&src.store, &src.secrets, &with_secrets("pw", false)).unwrap();
    let resolutions =
        HashMap::from([(src.profile.id, Resolution::Duplicate), (src.host.id, Resolution::Duplicate)]);
    let tmp = tempfile::tempdir().unwrap();

    let report = import_apply(&bytes, Some(&pass("pw")), &resolutions, &src.store, &src.secrets, tmp.path()).unwrap();

    assert_eq!(report, ImportReport { imported: 2, skipped: 1, secrets_restored: 1 });
    let profiles = src.store.list_profiles().unwrap();
    let dup_profile = profiles.iter().find(|p| p.name == "admin (Import)").unwrap();
    assert_ne!(dup_profile.id, src.profile.id);
    let hosts = src.store.list_hosts().unwrap();
    let dup_host = hosts.iter().find(|h| h.name == "web (Import)").unwrap();
    assert_ne!(dup_host.id, src.host.id);
    assert_eq!(dup_host.auth, HostAuth::Profile { profile_id: dup_profile.id });
    assert_eq!(dup_host.group_id, Some(src.group.id), "skipped group resolves to the stored one");
    assert_ne!(dup_host.forwards[0].id, src.host.forwards[0].id);
    let secret = src.secrets.get(SecretOwner::Profile(dup_profile.id), SecretKind::Password).unwrap().unwrap();
    assert_eq!(secret.expose_secret(), "s3cret");
}
```

`crates/core/src/export.rs` (only the test module declaration for now):

```rust
#[cfg(test)]
mod tests;
```

Replace `crates/core/src/lib.rs` with:

```rust
//! LinaSCP core: storage, secrets, known_hosts, ssh-config, export and sessions.

pub mod error;
pub mod export;
mod fsutil;
pub mod known_hosts;
pub mod model;
pub mod paths;
pub mod secrets;
pub mod sshconfig;
pub mod store;

pub use error::{CoreError, NetworkKind, Result};
pub use export::{
    Conflict, ExportOptions, FORMAT_VERSION, ImportItem, ImportPreview, ImportReport, ItemKind, Resolution,
    export_data, import_apply, import_preview,
};
pub use known_hosts::{HostKeyInfo, HostKeyStatus, KnownHosts, fingerprints};
pub use model::*;
pub use paths::AppPaths;
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretKind, SecretOwner, SecretStore};
pub use sshconfig::{INCLUDE_LINE, Mirror, import_ssh_config, merge_hosts};
pub use store::Store;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core export`
Expected: FAIL to compile — `unresolved imports export::Conflict, export::ExportOptions, ...`.

- [ ] **Step 3: Implement export.rs**

Replace `crates/core/src/export.rs` with:

```rust
//! `.linascp` export/import: JSON document, secrets (and optionally key files) embedded
//! as an ASCII-armored age block encrypted with a passphrase (scrypt).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::iter;
use std::path::{Path, PathBuf};

use age::armor::{ArmoredReader, ArmoredWriter, Format};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::fsutil;
use crate::model::{AuthConfig, AuthMethod, Group, Host, HostAuth, Id, LoginProfile, now_unix};
use crate::secrets::{SecretKind, SecretOwner, SecretStore, account_name, parse_account_name};
use crate::store::{self, Store};

pub const FORMAT_VERSION: u32 = 1;

/// Settings that travel with an export. `ssh_mode` is excluded on purpose: switching it
/// has side effects on `~/.ssh` that need the user's confirmation.
const EXPORTED_SETTINGS: &[&str] = &[store::THEME, store::TERMINAL_RENDERER, store::TERMINAL_FONT_SIZE];

const DUPLICATE_SUFFIX: &str = " (Import)";

pub struct ExportOptions {
    pub include_secrets: bool,
    pub include_key_files: bool,
    pub passphrase: Option<SecretString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Host,
    Profile,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Conflict {
    SameId,
    SameName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub id: Id,
    pub name: String,
    pub kind: ItemKind,
    pub conflict: Option<Conflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub items: Vec<ImportItem>,
    pub has_secrets: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Resolution {
    Skip,
    Replace,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: u32,
    pub skipped: u32,
    pub secrets_restored: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportDoc {
    format_version: u32,
    exported_at: i64,
    groups: Vec<Group>,
    profiles: Vec<LoginProfile>,
    hosts: Vec<Host>,
    settings: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secrets: Option<String>,
}

/// Plaintext of the encrypted block.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretPayload {
    /// keyring account name -> secret value
    secrets: BTreeMap<String, String>,
    /// original key path -> base64 file content
    #[serde(default)]
    key_files: BTreeMap<String, String>,
}

/// Serializes the whole store. Files containing secrets must be written with mode 0600.
pub fn export_data(store: &Store, secrets: &dyn SecretStore, opts: &ExportOptions) -> Result<Vec<u8>> {
    if opts.include_key_files && !opts.include_secrets {
        return Err(CoreError::config("key files can only be exported together with secrets"));
    }
    let groups = store.list_groups()?;
    let profiles = store.list_profiles()?;
    let hosts = store.list_hosts()?;
    let mut settings = BTreeMap::new();
    for key in EXPORTED_SETTINGS {
        if let Some(value) = store.get_setting(key)? {
            settings.insert(key.to_string(), value);
        }
    }

    let secrets_block = if opts.include_secrets {
        let passphrase = opts
            .passphrase
            .as_ref()
            .filter(|p| !p.expose_secret().is_empty())
            .ok_or_else(|| CoreError::config("passphrase required to export secrets"))?;
        let payload = collect_secrets(&hosts, &profiles, secrets, opts.include_key_files)?;
        let json = serde_json::to_vec(&payload).map_err(|e| CoreError::internal(e.to_string()))?;
        Some(encrypt(&json, passphrase)?)
    } else {
        None
    };

    let doc = ExportDoc {
        format_version: FORMAT_VERSION,
        exported_at: now_unix(),
        groups,
        profiles,
        hosts,
        settings,
        secrets: secrets_block,
    };
    serde_json::to_vec_pretty(&doc).map_err(|e| CoreError::internal(e.to_string()))
}

pub fn import_preview(bytes: &[u8], store: &Store) -> Result<ImportPreview> {
    let doc = parse_doc(bytes)?;
    let existing = Existing::load(store)?;
    let mut items = Vec::new();
    for g in &doc.groups {
        items.push(item(g.id, &g.name, ItemKind::Group, existing.conflict(ItemKind::Group, g.id, &g.name)));
    }
    for p in &doc.profiles {
        items.push(item(p.id, &p.name, ItemKind::Profile, existing.conflict(ItemKind::Profile, p.id, &p.name)));
    }
    for h in &doc.hosts {
        items.push(item(h.id, &h.name, ItemKind::Host, existing.conflict(ItemKind::Host, h.id, &h.name)));
    }
    Ok(ImportPreview { items, has_secrets: doc.secrets.is_some() })
}

/// Applies an export file. Conflicting items without an entry in `resolutions` are skipped.
/// Secrets are restored only when the file has a secrets block and `passphrase` is given.
pub fn import_apply(
    bytes: &[u8],
    passphrase: Option<&SecretString>,
    resolutions: &HashMap<Id, Resolution>,
    store: &Store,
    secrets: &dyn SecretStore,
    key_dir: &Path,
) -> Result<ImportReport> {
    let mut doc = parse_doc(bytes)?;
    let payload = match (&doc.secrets, passphrase) {
        (Some(block), Some(pass)) => Some(decrypt_payload(block, pass)?),
        _ => None,
    };

    let mut report = ImportReport { imported: 0, skipped: 0, secrets_restored: 0 };
    match &payload {
        Some(payload) => {
            let paths = write_key_files(&payload.key_files, key_dir)?;
            rewrite_key_paths(&mut doc, &paths);
        }
        None => clear_has_secret(&mut doc),
    }

    let existing = Existing::load(store)?;
    let mut plan = Plan::default();
    for g in &doc.groups {
        plan.decide(&existing, resolutions, ItemKind::Group, g.id, &g.name);
    }
    for p in &doc.profiles {
        plan.decide(&existing, resolutions, ItemKind::Profile, p.id, &p.name);
    }
    for h in &doc.hosts {
        plan.decide(&existing, resolutions, ItemKind::Host, h.id, &h.name);
    }

    for g in &doc.groups {
        match plan.action(g.id) {
            Action::Write { id, name } => {
                store.save_group(&Group { id, name, sort_order: g.sort_order })?;
                report.imported += 1;
            }
            Action::Skip => report.skipped += 1,
        }
    }
    for p in &doc.profiles {
        match plan.action(p.id) {
            Action::Write { id, name } => {
                store.save_profile(&LoginProfile { id, name, ..p.clone() })?;
                report.imported += 1;
            }
            Action::Skip => report.skipped += 1,
        }
    }
    for h in &doc.hosts {
        match plan.action(h.id) {
            Action::Write { id, name } => {
                let mut host = h.clone();
                if id != h.id {
                    for fwd in &mut host.forwards {
                        fwd.id = Id::new_v4();
                    }
                }
                host.id = id;
                host.name = name;
                host.group_id = h.group_id.map(|g| plan.map_id(g));
                host.jump_host_id = h.jump_host_id.map(|j| plan.map_id(j));
                if let HostAuth::Profile { profile_id } = &mut host.auth {
                    *profile_id = plan.map_id(*profile_id);
                }
                store.save_host(&host)?;
                report.imported += 1;
            }
            Action::Skip => report.skipped += 1,
        }
    }

    if let Some(payload) = &payload {
        for (account, value) in &payload.secrets {
            let Some((owner, kind)) = parse_account_name(account) else { continue };
            let Action::Write { id, .. } = plan.action(owner.id()) else { continue };
            let owner = match owner {
                SecretOwner::Host(_) => SecretOwner::Host(id),
                SecretOwner::Profile(_) => SecretOwner::Profile(id),
            };
            secrets.set(owner, kind, &SecretString::from(value.clone()))?;
            report.secrets_restored += 1;
        }
    }

    for (key, value) in &doc.settings {
        if EXPORTED_SETTINGS.contains(&key.as_str()) {
            store.set_setting(key, value)?;
        }
    }
    Ok(report)
}

fn item(id: Id, name: &str, kind: ItemKind, conflict: Option<Conflict>) -> ImportItem {
    ImportItem { id, name: name.to_string(), kind, conflict }
}

fn parse_doc(bytes: &[u8]) -> Result<ExportDoc> {
    let doc: ExportDoc =
        serde_json::from_slice(bytes).map_err(|e| CoreError::config(format!("invalid export file: {e}")))?;
    if doc.format_version > FORMAT_VERSION {
        return Err(CoreError::config(format!("unsupported export format version {}", doc.format_version)));
    }
    Ok(doc)
}

/// Ids and names of what is already stored, per item kind.
struct Existing {
    items: Vec<(ItemKind, Id, String)>,
}

impl Existing {
    fn load(store: &Store) -> Result<Self> {
        let mut items = Vec::new();
        items.extend(store.list_groups()?.into_iter().map(|g| (ItemKind::Group, g.id, g.name)));
        items.extend(store.list_profiles()?.into_iter().map(|p| (ItemKind::Profile, p.id, p.name)));
        items.extend(store.list_hosts()?.into_iter().map(|h| (ItemKind::Host, h.id, h.name)));
        Ok(Self { items })
    }

    fn conflict(&self, kind: ItemKind, id: Id, name: &str) -> Option<Conflict> {
        if self.items.iter().any(|(k, i, _)| *k == kind && *i == id) {
            Some(Conflict::SameId)
        } else if self.same_name(kind, name).is_some() {
            Some(Conflict::SameName)
        } else {
            None
        }
    }

    fn same_name(&self, kind: ItemKind, name: &str) -> Option<Id> {
        self.items.iter().find(|(k, _, n)| *k == kind && n == name).map(|(_, id, _)| *id)
    }
}

#[derive(Clone)]
enum Action {
    Write { id: Id, name: String },
    Skip,
}

/// Per imported item: what to do, and which stored id references to it should point at.
#[derive(Default)]
struct Plan {
    actions: HashMap<Id, Action>,
    id_map: HashMap<Id, Id>,
}

impl Plan {
    fn decide(
        &mut self,
        existing: &Existing,
        resolutions: &HashMap<Id, Resolution>,
        kind: ItemKind,
        id: Id,
        name: &str,
    ) {
        let conflict = existing.conflict(kind, id, name);
        let resolution = resolutions.get(&id).copied().unwrap_or(Resolution::Skip);
        // For a same-name conflict, "replace" and "skip" both resolve references to the stored item.
        let target = match conflict {
            Some(Conflict::SameName) => existing.same_name(kind, name).unwrap_or(id),
            _ => id,
        };
        let (action, mapped) = match (conflict, resolution) {
            (None, _) => (Action::Write { id, name: name.to_string() }, id),
            (Some(_), Resolution::Skip) => (Action::Skip, target),
            (Some(_), Resolution::Replace) => (Action::Write { id: target, name: name.to_string() }, target),
            (Some(_), Resolution::Duplicate) => {
                let new_id = Id::new_v4();
                (Action::Write { id: new_id, name: format!("{name}{DUPLICATE_SUFFIX}") }, new_id)
            }
        };
        self.actions.insert(id, action);
        self.id_map.insert(id, mapped);
    }

    fn action(&self, id: Id) -> Action {
        self.actions.get(&id).cloned().unwrap_or(Action::Skip)
    }

    /// Ids not part of the import (e.g. ssh-config jump hosts) stay unchanged.
    fn map_id(&self, id: Id) -> Id {
        self.id_map.get(&id).copied().unwrap_or(id)
    }
}

fn collect_secrets(
    hosts: &[Host],
    profiles: &[LoginProfile],
    secrets: &dyn SecretStore,
    include_key_files: bool,
) -> Result<SecretPayload> {
    let mut payload = SecretPayload::default();
    let owners =
        hosts.iter().map(|h| SecretOwner::Host(h.id)).chain(profiles.iter().map(|p| SecretOwner::Profile(p.id)));
    for owner in owners {
        for kind in [SecretKind::Password, SecretKind::Passphrase] {
            if let Some(value) = secrets.get(owner, kind)? {
                payload.secrets.insert(account_name(owner, kind), value.expose_secret().to_string());
            }
        }
    }
    if include_key_files {
        let auths = hosts
            .iter()
            .filter_map(|h| match &h.auth {
                HostAuth::Custom { auth } => Some(auth),
                HostAuth::Profile { .. } => None,
            })
            .chain(profiles.iter().map(|p| &p.auth));
        for auth in auths {
            let Some(path) = key_path(auth) else { continue };
            if payload.key_files.contains_key(path) {
                continue;
            }
            match fs::read(path) {
                Ok(content) => {
                    payload.key_files.insert(path.to_string(), B64.encode(content));
                }
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
    }
    Ok(payload)
}

fn key_path(auth: &AuthConfig) -> Option<&str> {
    match auth {
        AuthConfig { method: AuthMethod::Key, key_path: Some(path), .. } if !path.is_empty() => Some(path),
        _ => None,
    }
}

/// Writes key files into `key_dir` (0700) as 0600 files and returns original -> new path.
fn write_key_files(files: &BTreeMap<String, String>, key_dir: &Path) -> Result<HashMap<String, String>> {
    let mut mapping = HashMap::new();
    if files.is_empty() {
        return Ok(mapping);
    }
    fsutil::create_private_dir(key_dir)?;
    for (original, encoded) in files {
        let content =
            B64.decode(encoded).map_err(|e| CoreError::config(format!("invalid key file in export: {e}")))?;
        let base = Path::new(original)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "key".to_string());
        let target = unique_key_path(key_dir, &base, &content)?;
        fsutil::write_private(&target, &content)?;
        mapping.insert(original.clone(), target.to_string_lossy().into_owned());
    }
    Ok(mapping)
}

/// `base`, `base-1`, `base-2`, ...; an existing file with identical content is reused.
fn unique_key_path(dir: &Path, base: &str, content: &[u8]) -> Result<PathBuf> {
    for n in 0u32.. {
        let name = if n == 0 { base.to_string() } else { format!("{base}-{n}") };
        let candidate = dir.join(name);
        match fs::read(&candidate) {
            Ok(existing) if existing == content => return Ok(candidate),
            Ok(_) => continue,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(candidate),
            Err(err) => return Err(err.into()),
        }
    }
    unreachable!("u32 range exhausted while choosing a key file name")
}

fn rewrite_key_paths(doc: &mut ExportDoc, paths: &HashMap<String, String>) {
    let rewrite = |auth: &mut AuthConfig| {
        if let Some(new_path) = auth.key_path.as_ref().and_then(|p| paths.get(p)) {
            auth.key_path = Some(new_path.clone());
        }
    };
    for host in &mut doc.hosts {
        if let HostAuth::Custom { auth } = &mut host.auth {
            rewrite(auth);
        }
    }
    for profile in &mut doc.profiles {
        rewrite(&mut profile.auth);
    }
}

/// Without restored secrets nothing is in the keyring, so `has_secret` must not claim otherwise.
fn clear_has_secret(doc: &mut ExportDoc) {
    for host in &mut doc.hosts {
        if let HostAuth::Custom { auth } = &mut host.auth {
            auth.has_secret = false;
        }
    }
    for profile in &mut doc.profiles {
        profile.auth.has_secret = false;
    }
}

fn encrypt(plaintext: &[u8], passphrase: &SecretString) -> Result<String> {
    let encryptor = age::Encryptor::with_user_passphrase(passphrase.clone());
    let mut armored = Vec::new();
    let armor = ArmoredWriter::wrap_output(&mut armored, Format::AsciiArmor)?;
    let mut writer = encryptor.wrap_output(armor)?;
    writer.write_all(plaintext)?;
    writer.finish()?.finish()?;
    String::from_utf8(armored).map_err(|e| CoreError::internal(e.to_string()))
}

fn decrypt_payload(block: &str, passphrase: &SecretString) -> Result<SecretPayload> {
    let decryptor = age::Decryptor::new(ArmoredReader::new(block.as_bytes()))
        .map_err(|e| CoreError::config(format!("invalid secrets block: {e}")))?;
    let identity = age::scrypt::Identity::new(passphrase.clone());
    let mut reader = decryptor.decrypt(iter::once(&identity as &dyn age::Identity)).map_err(|e| match e {
        age::DecryptError::ExcessiveWork { .. } => {
            CoreError::config(format!("export file cannot be decrypted on this machine: {e}"))
        }
        _ => CoreError::config("wrong passphrase"),
    })?;
    let mut plaintext = Vec::new();
    reader.read_to_end(&mut plaintext)?;
    serde_json::from_slice(&plaintext).map_err(|e| CoreError::config(format!("invalid secrets block: {e}")))
}

#[cfg(test)]
mod tests;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo fmt && cargo test -p linascp-core export && cargo clippy -p linascp-core --all-targets`
Expected: PASS — `test result: ok. 10 passed` (≈3–4 s; age calibrates scrypt to ~1 s per encryption/decryption, which is why the workspace sets `opt-level = 2` for dependencies).

Then the whole crate: `cargo test -p linascp-core` → `test result: ok. 62 passed; 0 failed; 1 ignored`.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): add .linascp export/import with age-encrypted secrets" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
