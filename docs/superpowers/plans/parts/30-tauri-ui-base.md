# Part 30 — Tauri layer + UI foundation (Tasks 13–18)

Conventions used in this part:

- A bold path line (e.g. **`src-tauri/src/state.rs`**) directly above a code block means: write the **complete** file with exactly this content (create it, or replace it entirely if it already exists). `(tests)` after the path = the file initially contains only its test module (red step); `(implementation — insert above the #[cfg(test)] module)` = put this code at the top of that file, keeping the tests below (green step).
- Verification: all code of this part was compiled and run in a scratch workspace — `src-tauri` against the core code of parts 10/20 (43 Rust tests, clippy `-D warnings` clean; intermediate states after Task 13 and 14 pass too), the UI with the listed npm versions (81 Vitest tests, `tsc` 7, `vite build`).
- All `cargo` commands run from the repository root. All `bun` commands run from the repository root (there is exactly one `package.json`, at the root; see Task 16).
- Verified versions (2026-10-11): `tauri` 2.12.3, `tauri-build` 2.7.2, `tauri-specta` **=2.0.0-rc.25**, `specta` **=2.0.0-rc.25**, `specta-typescript` **=0.0.12**, `tauri-plugin-dialog` 2.8.2, `tracing-appender` 0.2.5, `tokio-util` 0.7.20, `secrecy` 0.10.3; npm: `@tauri-apps/api` 2.12.3, `@tauri-apps/cli` 2.12.2, `@tauri-apps/plugin-dialog` 2.8.2, `react` 19.3.0, `vite` 8.3.4, `@vitejs/plugin-react` 6.1.2, `typescript` 7.0.2, `zustand` 5.0.15, `vitest` 4.1.11, `jsdom` 29.1.1 (vitest 5 / jsdom 30 need Node ≥ 22; Debian 13 ships Node 20.19 — both versions here run on it), `@testing-library/react` 16.3.3, `@testing-library/jest-dom` 7.0.1, `@testing-library/user-event` 14.6.7, `lucide-react` 1.55.0, `@fontsource/*` 5.3.0.
- **Assumptions about `crates/core` (Tasks 1–12, verified against the core plan parts 10/20):** edition 2024; every IPC type derives `specta::Type` with exactly `specta = { version = "=2.0.0-rc.25", features = ["derive", "uuid"] }` (tauri-specta rc.25 pins this version). Items are addressed by module path `linascp_core::{error, paths, model, store, secrets, known_hosts, sshconfig, export, session, connect}::…`; `sshconfig` re-exports `import_ssh_config`, `merge_hosts`, `Mirror`; `connect` re-exports `connect_ssh`, `test_connection`, `ConnectDeps`, `ConnectEvent`, `ConnectRequest`, `ConnectTarget`, `EmitFn`, `Prompt`, `PromptAnswer`, `PromptBroker`. `SessionManager: Default` (`&self` methods, `remove` returns `Option`), `MemorySecretStore::new()`, `KeyringSecretStore::new()` (infallible, **blocking** D-Bus calls), `AppPaths::ensure_dirs()`, `LoginProfile::new(name, user, AuthConfig)`, `AuthConfig::new(method)`, `Group::new(name, sort_order)`. `SecretOwner` serializes as `{ "kind": "host" | "profile", "id": "<uuid>" }`. `PromptBroker::ask_cancellable(&emit, prompt, &cancel)` publishes prompts through the per-call `emit`, so **one global broker** serves all connects. `connect_ssh` does not emit `Connected`.
- Build prerequisites (Debian): `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev pkg-config`.

## Design decisions / contract adjustments made in this part

1. **Raw output channel type.** `tauri::ipc::Channel<InvokeResponseBody>` is not usable in a `#[specta::specta]` command (`InvokeResponseBody` does not implement `specta::Type`). `src-tauri/src/output.rs` defines `pub struct RawChunk(pub Vec<u8>)` which implements `tauri::ipc::IpcResponse` (body = `InvokeResponseBody::Raw`, i.e. no JSON encoding) and `specta::Type` (exported as TS `ArrayBuffer`). Commands take `output: Channel<RawChunk>`; generated TS signature is `output: Channel<ArrayBuffer>`. The wire format is identical to `Channel<InvokeResponseBody>` + `InvokeResponseBody::Raw` (confirmed by the performance spike: 300–370 MB/s). Never `Channel<Vec<u8>>` (JSON number array). Verified by compiling a prototype against tauri 2.12.3 + tauri-specta rc.25.
2. **BigInt.** `u64` (`promptId`) and `i64` (`createdAt`/`updatedAt`) are exported as TS `number` via `Builder::dangerously_cast_bigints_to_number()` (values stay far below 2^53).
3. **Prompt routing (as in the contract).** `AppState.broker: Arc<PromptBroker>` is global; `connect_answer(prompt_id, answer) -> bool`. `AppState.connects: Mutex<HashMap<Uuid, CancellationToken>>` keyed by the UI-generated `connect_id`.
4. **`connect_test` takes `connect_id`** (`connect_test(connect_id, host, events)`, deviation) so that a running test can be cancelled with `connect_cancel`. It calls core's `test_connection` (stops after Auth).
5. **Session id.** `src-tauri` generates the `SessionId` after a successful connect, registers the session, emits `ConnectEvent::Connected { sessionId }` on the events channel and returns the id from the command.
6. **Flow control (new command).** `session_ack(session_id, bytes: u32)`. Output is coalesced by `OutputPump` (flush every 8 ms or at 64 KiB). The pump blocks the session's reader once unacknowledged bytes reach 1 MiB and releases it at ≤ 256 KiB.
7. **Secrets.** Commands take core's `SecretOwner` directly (TS `{ kind: "host"; id: string } | { kind: "profile"; id: string }`). All keyring calls run in `spawn_blocking`. Deleting a host/profile also deletes its keyring secrets (best effort).
8. **Mirror.** `Mirror::enable/write` receive the **merged** host list (store + ssh-config) so `ProxyJump` to ssh-config hosts resolves.
9. **Settings.** `Settings` additionally has `compatRendering: boolean` (store key `compat_rendering`, `"0"`/`"1"`, default `"0"`). `firstRunDone` is stored as `"1"`/`"0"`.
10. **WebKit workaround.** `WEBKIT_DISABLE_DMABUF_RENDERER=1` is only set when `compatRendering` is on (and the variable is not already set). The flag is read from the SQLite store, which `AppState::init()` opens in `run()` **before** `tauri::Builder` (and before any thread is spawned) — no extra file needed. Changing it takes effect at the next start.
11. **Dialog plugin.** `tauri-plugin-dialog` is registered (Task 15) so the export/import screens can use `save()`/`open()` from `@tauri-apps/plugin-dialog`.
12. **Bindings.** `ui/src/ipc/bindings.ts` is generated (and committed) by `cargo test -p linascp exports_bindings`; debug builds also regenerate it on start.

## Conventions for the screen tasks (19–24)

- **IPC:** import only from `ui/src/ipc/api.ts` (functions + types) and `ui/src/ipc/errors.ts` (`isCoreError`, `errorMessage`, `errorDetails`; UI switches on `error.type`). Never import `bindings.ts` directly. New ids: `newId()`.
- **Tests mock IPC with automock:** `vi.mock("<relative>/ipc/api")` + `import * as api from "<relative>/ipc/api"` + `vi.mocked(api.hostsList).mockResolvedValue([...])` (vitest `clearMocks: true`; implementations persist within a file, so set them in `beforeEach`). `errors.ts` is never mocked. Reset stores in `beforeEach` with `useXStore.setState(useXStore.getInitialState(), true)`. Components using `useToast` need `<ToastProvider>` in the test render.
- **Streams:** create the output channel before connecting with `createOutputStream()`; pass `stream.channel` to `connectSsh`/`sessionOpenLocal`, call `stream.bind(sessionId)` when the promise resolves, `stream.attach(bytes => term.write(bytes, () => stream.ack(bytes.length)))` in `TerminalView`. Events: `createEventChannel<ConnectEvent>(handler)`, `createEventChannel<SessionEventDto>(handler)`. Keystrokes: `sessionWrite(sessionId, data /* string from xterm onData */)`.
- **Terminal rendering:** WebGL addon only on the visible terminal (WebKit allows ~16 WebGL contexts) — dispose it when a tab is hidden, recreate when shown; fall back to the DOM renderer on `onContextLoss` or when `settings.terminalRenderer === "dom"`. Opaque background (no transparency). The terminal container must carry `data-terminal` so Ctrl+K/Ctrl+N stay with the shell. `TerminalScreen` stays mounted while hidden (AppShell uses `hidden`).
- **Shell contracts:** replace the stub files listed in Task 18 keeping their export names/props (`HostsScreen()`, `TerminalScreen()`, `EditorModal({ target })`, `ConnectScreen()`, `SettingsDialog()`, `FirstRunDialog()`, `ExportDialog()`, `ImportDialog()`); dialogs close via `useUiStore.getState().closeModal()`; the host search input focuses itself whenever `useUiStore().searchFocusToken` changes; `FirstRunDialog` must save `firstRunDone: true` via `useSettingsStore().save(...)`. `useSessionsStore.openLocal()` is implemented by Task 22.
- **Styling:** CSS Modules next to the component (`X.module.css`), only design tokens (`var(--…)`) from `tokens.css`; radii chip 6 / input 8 / button 10 / card 12 / dialog 16; heights button 34/40/44, input 40; `cx()` from `components`; `.mono` and `.visually-hidden` are global helpers. Every UI string lives in `ui/src/i18n/de.ts` — add one top-level group per screen (`t.hosts`, `t.editor`, `t.connect`, `t.terminal`, `t.settings`).

---

### Task 13: src-tauri scaffold, AppState, data commands, bindings export

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/app-icon.svg`, `src-tauri/icons/*` (generated), `src-tauri/.gitignore`
- Create: `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/state.rs`, `src-tauri/src/dto.rs`
- Create: `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/settings.rs`, `src-tauri/src/commands/hosts.rs`, `src-tauri/src/commands/profiles.rs`, `src-tauri/src/commands/secrets.rs`
- Create (generated): `ui/src/ipc/bindings.ts`
- Test: unit tests inside each `commands/*.rs`, `dto.rs`, `lib.rs`

**Interfaces:**
- Consumes (core): `AppPaths::{from_env, for_root, ensure_dirs, db_path, ssh_config, ssh_dir}`, `Store::{open, open_in_memory, list_hosts, get_host, save_host, delete_host, list_profiles, get_profile, save_profile, delete_profile, profile_usage, list_groups, save_group, delete_group, get_setting, set_setting}`, store keys `THEME, SSH_MODE, TERMINAL_RENDERER, TERMINAL_FONT_SIZE, FIRST_RUN_DONE`, `SecretStore`, `KeyringSecretStore`, `MemorySecretStore`, `SecretOwner`, `SecretKind`, `import_ssh_config`, `merge_hosts`, `Mirror::{new, enable, write, disable, is_enabled}`, `SessionManager`, model types.
- Produces (Rust):
  - `pub struct AppState { pub paths: AppPaths, pub store: Arc<Store>, pub secrets: Arc<dyn SecretStore>, pub sessions: SessionManager }` with `AppState::new(paths, store, secrets)`, `AppState::init() -> Result<AppState>`, `#[cfg(test)] AppState::for_tests(root: &Path)`.
  - DTOs: `Theme { Dark, Light }`, `SshMode { Integrate, Mirror }`, `TerminalRenderer { Auto, Webgl, Dom }`, `Settings { theme, ssh_mode, terminal_renderer, terminal_font_size: u16, first_run_done: bool, compat_rendering: bool }`, `ProfileWithUsage { profile: LoginProfile, used_by: u32 }`.
  - Helpers: `commands::blocking(f)`, `commands::settings::{load_settings(&Store), save_settings(&AppState, Settings), sync_mirror(&AppState), COMPAT_RENDERING}`, `AppState::merged_hosts(&self) -> Result<Vec<Host>>`, `commands::hosts::{save_host, delete_host, save_group}`, `commands::profiles::{list_profiles, save_profile, delete_profile}`, `commands::secrets::{set_secret, delete_secret, purge_secrets}`.
  - Commands: `hosts_list, host_save, host_delete, groups_list, group_save, group_delete, profiles_list, profile_save, profile_delete, secret_set, secret_delete, settings_get, settings_set`.
  - `linascp_lib::{specta_builder(), export_bindings(&Builder), BINDINGS_PATH, run()}`.
- Produces (TS, generated): `commands.hostsList()`, … returning `{status:"ok",data}|{status:"error",error: CoreError}`.

- [ ] **Step 1: Register the crate in the workspace and create the manifest**

In the root `Cargo.toml`, change only the members line (keep `resolver` and the `[profile.dev.package."*"]` section) so it reads:

```toml
members = ["crates/core", "src-tauri"]
```

**`src-tauri/Cargo.toml`**
```toml
[package]
name = "linascp"
version = "0.1.0"
description = "LinaSCP – SSH/SFTP client for Linux"
edition = "2024"
publish = false

[lib]
name = "linascp_lib"

[build-dependencies]
tauri-build = { version = "2.7", features = [] }

[dependencies]
linascp-core = { path = "../crates/core" }
tauri = { version = "2.12", features = [] }
tauri-specta = { version = "=2.0.0-rc.25", features = ["derive", "typescript"] }
specta = { version = "=2.0.0-rc.25", features = ["derive", "uuid"] }
specta-typescript = "=0.0.12"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
secrecy = "0.10"

[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "test-util"] }
```

**`src-tauri/build.rs`**
```rust
fn main() {
    tauri_build::build()
}
```

**`src-tauri/.gitignore`**
```gitignore
/target/
/gen/schemas
```

- [ ] **Step 2: Tauri config, capability, icons**

**`src-tauri/tauri.conf.json`**
```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "LinaSCP",
  "version": "0.1.0",
  "identifier": "de.mc-kg.linascp",
  "build": {
    "devUrl": "http://localhost:5173",
    "frontendDist": "../ui/dist",
    "beforeDevCommand": "bun run dev",
    "beforeBuildCommand": "bun run build"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "LinaSCP",
        "width": 1440,
        "height": 900,
        "minWidth": 1100,
        "minHeight": 700,
        "resizable": true
      }
    ],
    "security": {
      "csp": "default-src 'self' ipc: http://ipc.localhost; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:",
      "devCsp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["deb", "appimage"],
    "category": "Network",
    "shortDescription": "SSH- und SFTP-Client für Linux",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.png"]
  }
}
```

**`src-tauri/capabilities/default.json`**
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Permissions of the main window",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

**`src-tauri/app-icon.svg`**
```svg
<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <rect x="64" y="64" width="896" height="896" rx="200" fill="#4FD1C5"/>
  <path d="M300 380 L440 512 L300 644" fill="none" stroke="#062521" stroke-width="64" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M500 660 H720" fill="none" stroke="#062521" stroke-width="64" stroke-linecap="round"/>
</svg>
```

Generate the icons (the root `package.json` does not exist yet, so call the CLI directly) and drop the mobile/Windows-store variants:

```bash
bunx @tauri-apps/cli@2.12.2 icon src-tauri/app-icon.svg -o src-tauri/icons
rm -rf src-tauri/icons/android src-tauri/icons/ios src-tauri/icons/Square*Logo.png src-tauri/icons/StoreLogo.png
ls src-tauri/icons
```
Expected: `128x128.png 128x128@2x.png 32x32.png 64x64.png icon.icns icon.ico icon.png`.

- [ ] **Step 3: Write the failing DTO tests**

**`src-tauri/src/lib.rs`**
```rust
pub mod dto;
```

**`src-tauri/src/main.rs`**
```rust
fn main() {}
```

**`src-tauri/src/dto.rs`** (tests)
```rust
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
                "terminalFontSize": 14, "firstRunDone": false, "compatRendering": false
            })
        );
    }
}
```

Run: `cargo test -p linascp dto::`
Expected: FAIL to compile — `error[E0433]: failed to resolve: use of undeclared type Theme` (and similar for `SshMode`, `Settings`). (Core Tasks 1–12 must be merged first; otherwise the failure is `unresolved import linascp_core`.)

- [ ] **Step 4: Implement the DTOs**

**`src-tauri/src/dto.rs`** (implementation — insert above the `#[cfg(test)]` module)
```rust
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
    pub first_run_done: bool,
    pub compat_rendering: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            ssh_mode: SshMode::Integrate,
            terminal_renderer: TerminalRenderer::Auto,
            terminal_font_size: 14,
            first_run_done: false,
            compat_rendering: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileWithUsage {
    pub profile: LoginProfile,
    pub used_by: u32,
}
```

Run: `cargo test -p linascp dto::`
Expected: PASS (2 tests).

- [ ] **Step 5: Write the failing settings tests (plus AppState)**

**`src-tauri/src/state.rs`**
```rust
//! Application state shared by all Tauri commands.

use std::sync::Arc;

use linascp_core::error::Result;
use linascp_core::model::Host;
use linascp_core::paths::AppPaths;
use linascp_core::secrets::{KeyringSecretStore, SecretStore};
use linascp_core::session::SessionManager;
use linascp_core::sshconfig::{import_ssh_config, merge_hosts};
use linascp_core::store::Store;

pub struct AppState {
    pub paths: AppPaths,
    pub store: Arc<Store>,
    pub secrets: Arc<dyn SecretStore>,
    pub sessions: SessionManager,
}

impl AppState {
    pub fn new(paths: AppPaths, store: Store, secrets: Arc<dyn SecretStore>) -> Self {
        Self { paths, store: Arc::new(store), secrets, sessions: SessionManager::default() }
    }

    /// Production state: XDG paths, on-disk SQLite store, OS keyring.
    pub fn init() -> Result<Self> {
        let paths = AppPaths::from_env()?;
        paths.ensure_dirs()?;
        let store = Store::open(&paths.db_path())?;
        Ok(Self::new(paths, store, Arc::new(KeyringSecretStore::new())))
    }

    /// Stored hosts merged with the read-only `~/.ssh/config` import (stored wins on equal name).
    /// A broken ssh config never blocks the app: it is logged and ignored.
    pub fn merged_hosts(&self) -> Result<Vec<Host>> {
        let stored = self.store.list_hosts()?;
        let path = self.paths.ssh_config();
        let imported = if path.exists() {
            import_ssh_config(&path).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %path.display(), "ignoring unreadable ssh config");
                Vec::new()
            })
        } else {
            Vec::new()
        };
        Ok(merge_hosts(stored, imported))
    }

    #[cfg(test)]
    pub fn for_tests(root: &std::path::Path) -> Self {
        use linascp_core::secrets::MemorySecretStore;
        let paths = AppPaths::for_root(root);
        std::fs::create_dir_all(&paths.ssh_dir).expect("create test ssh dir");
        Self::new(paths, Store::open_in_memory().expect("in-memory store"), Arc::new(MemorySecretStore::new()))
    }
}
```

**`src-tauri/src/commands/mod.rs`**
```rust
pub mod settings;

use linascp_core::error::{CoreError, Result};

/// Runs blocking work (SQLite bulk work, keyring D-Bus calls, file IO) off the async runtime.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CoreError::Internal { message: format!("background task failed: {e}") })?
}
```

**`src-tauri/src/lib.rs`**
```rust
pub mod commands;
pub mod dto;
pub mod state;
```

**`src-tauri/src/commands/settings.rs`** (tests)
```rust
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
            first_run_done: true,
            compat_rendering: true,
        };
        assert_eq!(save_settings(&state, next.clone()).unwrap(), next);
        assert_eq!(load_settings(&state.store).unwrap(), next);
        assert_eq!(state.store.get_setting(FIRST_RUN_DONE).unwrap().as_deref(), Some("1"));
        assert_eq!(state.store.get_setting(COMPAT_RENDERING).unwrap().as_deref(), Some("1"));
    }

    #[test]
    fn invalid_stored_values_fall_back_to_defaults() {
        let (_dir, state) = setup();
        state.store.set_setting(THEME, "purple").unwrap();
        state.store.set_setting(TERMINAL_FONT_SIZE, "300").unwrap();
        let s = load_settings(&state.store).unwrap();
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.terminal_font_size, 14);
    }

    #[test]
    fn rejects_out_of_range_font_size() {
        let (_dir, state) = setup();
        let next = Settings { terminal_font_size: 4, ..Settings::default() };
        assert!(matches!(save_settings(&state, next), Err(CoreError::Config { .. })));
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
```

Run: `cargo test -p linascp commands::settings::`
Expected: FAIL to compile — `cannot find function load_settings`, `cannot find type AppState`, etc.

- [ ] **Step 6: Implement settings load/save and the mirror sync**

**`src-tauri/src/commands/settings.rs`** (implementation — insert above the `#[cfg(test)]` module)
```rust
use std::ops::RangeInclusive;

use linascp_core::error::{CoreError, Result};
use linascp_core::sshconfig::Mirror;
use linascp_core::store::{Store, FIRST_RUN_DONE, SSH_MODE, TERMINAL_FONT_SIZE, TERMINAL_RENDERER, THEME};
use tauri::State;

use crate::dto::{Settings, SshMode, TerminalRenderer, Theme};
use crate::state::AppState;

/// Store key of the WebKit compatibility switch (`"0"` / `"1"`); not part of core's key list.
pub const COMPAT_RENDERING: &str = "compat_rendering";

const FONT_SIZE_RANGE: RangeInclusive<u16> = 8..=32;

fn parse_flag(value: Option<String>) -> Option<bool> {
    value.map(|v| matches!(v.as_str(), "1" | "true"))
}

fn flag_str(value: bool) -> &'static str {
    if value {
        "1"
    } else {
        "0"
    }
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
            .filter(|size| FONT_SIZE_RANGE.contains(size))
            .unwrap_or(d.terminal_font_size),
        first_run_done: parse_flag(store.get_setting(FIRST_RUN_DONE)?).unwrap_or(d.first_run_done),
        compat_rendering: parse_flag(store.get_setting(COMPAT_RENDERING)?).unwrap_or(d.compat_rendering),
    })
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
    if !FONT_SIZE_RANGE.contains(&next.terminal_font_size) {
        return Err(CoreError::Config {
            message: format!("terminal font size must be between 8 and 32, got {}", next.terminal_font_size),
        });
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
```

Run: `cargo test -p linascp commands::settings::`
Expected: PASS (6 tests).

- [ ] **Step 7: Write the failing tests for secrets, hosts/groups and profiles**

**`src-tauri/src/commands/mod.rs`**
```rust
pub mod hosts;
pub mod profiles;
pub mod secrets;
pub mod settings;

use linascp_core::error::{CoreError, Result};

/// Runs blocking work (SQLite bulk work, keyring D-Bus calls, file IO) off the async runtime.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CoreError::Internal { message: format!("background task failed: {e}") })?
}
```

**`src-tauri/src/commands/secrets.rs`** (tests)
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use linascp_core::secrets::SecretOwner;
    use secrecy::ExposeSecret;

    fn read(state: &AppState, id: uuid::Uuid, kind: SecretKind) -> Option<String> {
        state
            .secrets
            .get(SecretOwner::Host(id), kind)
            .unwrap()
            .map(|s| s.expose_secret().to_string())
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
```

**`src-tauri/src/commands/hosts.rs`** (tests)
```rust
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
        let content =
            std::fs::read_to_string(state.paths.ssh_dir.join("config.d").join("linascp.conf")).unwrap();
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
```

**`src-tauri/src/commands/profiles.rs`** (tests)
```rust
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
```

Run: `cargo test -p linascp commands::`
Expected: FAIL to compile — `cannot find function set_secret`, `save_host`, `save_profile` … in `super`.

- [ ] **Step 8: Implement secrets, hosts/groups and profiles commands**

**`src-tauri/src/commands/secrets.rs`** (implementation — insert above the `#[cfg(test)]` module)
```rust
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
pub async fn secret_set(
    state: State<'_, AppState>,
    owner: SecretOwner,
    kind: SecretKind,
    value: String,
) -> Result<()> {
    set_secret(state.secrets.clone(), owner, kind, value).await
}

#[tauri::command]
#[specta::specta]
pub async fn secret_delete(state: State<'_, AppState>, owner: SecretOwner, kind: SecretKind) -> Result<()> {
    delete_secret(state.secrets.clone(), owner, kind).await
}
```

**`src-tauri/src/commands/hosts.rs`** (implementation — insert above the `#[cfg(test)]` module)
```rust
use linascp_core::error::{CoreError, Result};
use linascp_core::model::{Group, Host, HostSource, Id};
use linascp_core::secrets::SecretOwner;
use tauri::State;

use super::secrets::purge_secrets;
use super::settings::sync_mirror;
use crate::state::AppState;

/// Saves (upserts) a host; editing an ssh-config host turns it into an overriding LinaSCP host.
pub(crate) fn save_host(state: &AppState, mut host: Host) -> Result<Host> {
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
```

**`src-tauri/src/commands/profiles.rs`** (implementation — insert above the `#[cfg(test)]` module)
```rust
use linascp_core::error::{CoreError, Result};
use linascp_core::model::{Id, LoginProfile};
use tauri::State;

use linascp_core::secrets::SecretOwner;

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
```

Run: `cargo test -p linascp commands::`
Expected: PASS — settings 6, secrets 1, hosts 6, profiles 1. (A failure here points at core behaviour — fix it in core, not by weakening these tests.)

- [ ] **Step 9: Write the failing bindings-export test**

**`src-tauri/src/lib.rs`** (tests)
```rust
pub mod commands;
pub mod dto;
pub mod state;

#[cfg(test)]
mod tests {
    #[test]
    fn exports_bindings() {
        super::export_bindings(&super::specta_builder()).expect("export bindings");
        let ts = std::fs::read_to_string(super::BINDINGS_PATH).unwrap();
        assert!(ts.contains("hostsList: () =>"));
        assert!(ts.contains("export type CoreError ="));
        assert!(ts.contains("export type Settings ="));
        assert!(ts.contains("export type SecretOwner ="));
    }
}
```

Run: `cargo test -p linascp exports_bindings`
Expected: FAIL to compile — `cannot find function export_bindings in module super`.

- [ ] **Step 10: Implement the specta builder, bindings export and `run()`**

**`src-tauri/src/lib.rs`**
```rust
pub mod commands;
pub mod dto;
pub mod state;

use commands::{hosts, profiles, secrets, settings};

/// Where the generated TypeScript bindings live (committed to git).
pub const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/ipc/bindings.ts");

/// All IPC commands with their specta type information.
pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            hosts::hosts_list,
            hosts::host_save,
            hosts::host_delete,
            hosts::groups_list,
            hosts::group_save,
            hosts::group_delete,
            profiles::profiles_list,
            profiles::profile_save,
            profiles::profile_delete,
            secrets::secret_set,
            secrets::secret_delete,
            settings::settings_get,
            settings::settings_set,
        ])
        .dangerously_cast_bigints_to_number()
}

pub fn export_bindings(builder: &tauri_specta::Builder<tauri::Wry>) -> Result<(), specta_typescript::Error> {
    builder.export(specta_typescript::Typescript::default(), BINDINGS_PATH)
}

pub fn run() {
    let state = match state::AppState::init() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("LinaSCP: startup failed: {error}");
            std::process::exit(1);
        }
    };
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    if let Err(error) = export_bindings(&builder) {
        eprintln!("LinaSCP: failed to export TypeScript bindings: {error}");
    }
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running LinaSCP");
}

#[cfg(test)]
mod tests {
    #[test]
    fn exports_bindings() {
        super::export_bindings(&super::specta_builder()).expect("export bindings");
        let ts = std::fs::read_to_string(super::BINDINGS_PATH).unwrap();
        assert!(ts.contains("hostsList: () =>"));
        assert!(ts.contains("export type CoreError ="));
        assert!(ts.contains("export type Settings ="));
        assert!(ts.contains("export type SecretOwner ="));
    }
}
```

**`src-tauri/src/main.rs`**
```rust
fn main() {
    linascp_lib::run()
}
```

Run: `mkdir -p ui/src/ipc && cargo test -p linascp exports_bindings`
Expected: PASS and `ui/src/ipc/bindings.ts` exists, containing e.g.
`hostsList: () => typedError<Host[], CoreError>(__TAURI_INVOKE("hosts_list"))` and
`export type CoreError = { type: "network"; kind: NetworkKind; message: string } | …`.
(Without the `mkdir` the export fails with `No such file or directory`.)

- [ ] **Step 11: Full check**

Run: `cargo fmt -p linascp && cargo test -p linascp && cargo clippy -p linascp --all-targets -- -D warnings`
Expected: all tests PASS (dto 2, settings 6, secrets 1, hosts 6, profiles 1, lib 1); clippy clean.

- [ ] **Step 12: Commit**

```bash
git add Cargo.toml Cargo.lock src-tauri ui/src/ipc/bindings.ts
git commit -m "feat(tauri): scaffold src-tauri with app state, data commands and specta bindings

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: Output transport, sessions and connect commands

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `tokio-util`)
- Create: `src-tauri/src/util.rs`, `src-tauri/src/output.rs`, `src-tauri/src/commands/sessions.rs`, `src-tauri/src/commands/connect.rs`
- Modify (full replacement): `src-tauri/src/state.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`
- Regenerate: `ui/src/ipc/bindings.ts`
- Test: unit tests in `output.rs`, `sessions.rs`, `connect.rs`, `lib.rs`

**Interfaces:**
- Consumes (core): `connect::{connect_ssh, ConnectDeps, ConnectEvent, ConnectRequest, ConnectTarget, Prompt, PromptAnswer, PromptBroker}`, `known_hosts::KnownHosts`, `session::{LocalPtySession, OutputSink, Session, SessionEvent, SessionId, SessionManager}`; Task 13 `merged_hosts`, `AppState`.
- Produces (Rust):
  - `output.rs`: consts `MAX_CHUNK = 65536`, `FLUSH_INTERVAL = 8ms`, `HIGH_WATERMARK = 1 MiB`, `LOW_WATERMARK = 256 KiB`; `RawChunk(pub Vec<u8>)` (`IpcResponse` raw + `specta::Type` = `ArrayBuffer`); `OutputBuffer::{push, take}`; `Watermark::{new, sent, acked, is_paused, unacked}`; `FlowGate::{new, sent, ack, close, is_paused, is_closed, wait_until_open}`; `type ChunkSender = Arc<dyn Fn(Vec<u8>) + Send + Sync>`; `OutputPump::{new(ChunkSender) -> Arc<Self>, for_channel(Channel<RawChunk>) -> Arc<Self>, start_ticker, push, flush, ack, close, is_closed}`.
  - `state.rs`: `AppState` gains `broker: Arc<PromptBroker>` (global), `connects: Mutex<HashMap<Uuid, CancellationToken>>`, `outputs: Mutex<HashMap<SessionId, Arc<OutputPump>>>`.
  - `sessions.rs`: `SessionEventDto { Exit { code: Option<u32> }, Error { error: CoreError } }` (tag `kind`); helpers `make_sink`, `channel_event_sender`, `register`, `open_local`, `write`, `resize`, `ack`, `close`; commands `session_open_local(cols, rows, output, session_events) -> SessionId`, `session_write(session_id, data: Vec<u8>)`, `session_resize(session_id, cols, rows)`, `session_ack(session_id, bytes: u32)`, `session_close(session_id)`.
  - `connect.rs`: helpers `find_host`, `build_request`, `register_connect`, `unregister_connect`, `cancel_connect`; commands `connect_ssh(connect_id, host_id, cols, rows, events, output, session_events) -> SessionId` (emits `Connected { sessionId }` on success), `connect_answer(prompt_id, answer) -> bool`, `connect_cancel(connect_id)`, `connect_test(connect_id, host, events)`.
- Produces (TS, generated): `commands.sessionOpenLocal(cols, rows, output: Channel<ArrayBuffer>, sessionEvents: Channel<SessionEventDto>)`, `commands.connectSsh(connectId, hostId, cols, rows, events: Channel<ConnectEvent>, output: Channel<ArrayBuffer>, sessionEvents: Channel<SessionEventDto>)`, `commands.connectAnswer(promptId, answer)`, `commands.connectCancel(connectId)`, `commands.connectTest(connectId, host, events)`, `commands.sessionWrite(sessionId, data: number[])`, `commands.sessionResize(sessionId, cols, rows)`, `commands.sessionAck(sessionId, bytes)`, `commands.sessionClose(sessionId)`.

- [ ] **Step 1: Add the dependency and the lock helper**

In `src-tauri/Cargo.toml`, add under `[dependencies]` (after `tokio = …`):

```toml
tokio-util = "0.7"
```

**`src-tauri/src/util.rs`**
```rust
use std::sync::{Mutex, MutexGuard};

/// Locks a mutex, recovering the data if a previous holder panicked.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
```

In `src-tauri/src/lib.rs`, add these two lines directly below `pub mod dto;`:

```rust
pub mod output;
mod util;
```

- [ ] **Step 2: Write the failing output-transport tests**

**`src-tauri/src/output.rs`** (tests)
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    type Chunks = Arc<Mutex<Vec<Vec<u8>>>>;

    fn collecting_pump() -> (Arc<OutputPump>, Chunks) {
        let chunks: Chunks = Arc::default();
        let sink = chunks.clone();
        let pump = OutputPump::new(Arc::new(move |chunk: Vec<u8>| sink.lock().unwrap().push(chunk)));
        (pump, chunks)
    }

    fn wait_for(condition: impl Fn() -> bool) {
        let start = Instant::now();
        while !condition() {
            assert!(start.elapsed() < Duration::from_secs(5), "condition not met within 5 s");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn buffer_coalesces_until_max_chunk() {
        let mut buffer = OutputBuffer::default();
        assert_eq!(buffer.push(b"abc"), None);
        assert_eq!(buffer.push(&vec![0u8; MAX_CHUNK - 4]), None);
        let chunk = buffer.push(b"z").expect("a full chunk");
        assert_eq!(chunk.len(), MAX_CHUNK);
        assert_eq!(buffer.take(), None);
        buffer.push(b"tail");
        assert_eq!(buffer.take(), Some(b"tail".to_vec()));
    }

    #[test]
    fn watermark_pauses_at_high_and_resumes_at_low() {
        let mut mark = Watermark::new(100, 25);
        mark.sent(60);
        assert!(!mark.is_paused());
        mark.sent(40);
        assert!(mark.is_paused());
        mark.acked(50);
        assert!(mark.is_paused(), "50 unacked bytes are still above the low watermark");
        mark.acked(25);
        assert!(!mark.is_paused());
        assert_eq!(mark.unacked(), 25);
        mark.acked(1_000);
        assert_eq!(mark.unacked(), 0);
    }

    #[test]
    fn gate_blocks_until_enough_bytes_are_acked() {
        let gate = Arc::new(FlowGate::new(10, 2));
        gate.sent(10);
        let done = Arc::new(AtomicBool::new(false));
        let waiter = {
            let (gate, done) = (gate.clone(), done.clone());
            std::thread::spawn(move || {
                gate.wait_until_open();
                done.store(true, Ordering::SeqCst);
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(!done.load(Ordering::SeqCst));
        gate.ack(5);
        std::thread::sleep(Duration::from_millis(50));
        assert!(!done.load(Ordering::SeqCst), "5 unacked bytes are still above the low watermark");
        gate.ack(3);
        waiter.join().unwrap();
        assert!(done.load(Ordering::SeqCst));
    }

    #[test]
    fn closing_the_gate_releases_waiters() {
        let gate = Arc::new(FlowGate::new(10, 2));
        gate.sent(50);
        let waiter = {
            let gate = gate.clone();
            std::thread::spawn(move || gate.wait_until_open())
        };
        std::thread::sleep(Duration::from_millis(20));
        gate.close();
        waiter.join().unwrap();
        assert!(gate.is_closed());
    }

    #[test]
    fn pump_emits_full_chunks_immediately_and_small_ones_on_flush() {
        let (pump, chunks) = collecting_pump();
        pump.push(&vec![1u8; MAX_CHUNK]);
        assert_eq!(chunks.lock().unwrap().len(), 1);
        pump.push(b"x");
        assert_eq!(chunks.lock().unwrap().len(), 1);
        pump.flush();
        assert_eq!(chunks.lock().unwrap()[1], b"x".to_vec());
    }

    #[test]
    fn close_flushes_pending_output() {
        let (pump, chunks) = collecting_pump();
        pump.push(b"bye");
        pump.close();
        assert_eq!(*chunks.lock().unwrap(), vec![b"bye".to_vec()]);
        assert!(pump.is_closed());
    }

    #[test]
    fn pump_blocks_the_reader_above_the_high_watermark_until_acked() {
        let (pump, chunks) = collecting_pump();
        let chunks_per_high_watermark = HIGH_WATERMARK as usize / MAX_CHUNK;
        let reader = {
            let pump = pump.clone();
            std::thread::spawn(move || {
                for _ in 0..=chunks_per_high_watermark {
                    pump.push(&vec![7u8; MAX_CHUNK]);
                }
            })
        };
        wait_for(|| chunks.lock().unwrap().len() == chunks_per_high_watermark);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(chunks.lock().unwrap().len(), chunks_per_high_watermark, "reader must be paused");
        assert!(!reader.is_finished());

        pump.ack(HIGH_WATERMARK - LOW_WATERMARK);
        reader.join().unwrap();
        assert_eq!(chunks.lock().unwrap().len(), chunks_per_high_watermark + 1);
    }

    #[tokio::test(start_paused = true)]
    async fn ticker_flushes_small_output() {
        let (pump, chunks) = collecting_pump();
        pump.start_ticker();
        pump.push(b"prompt$ ");
        assert!(chunks.lock().unwrap().is_empty());
        tokio::time::sleep(FLUSH_INTERVAL * 2).await;
        assert_eq!(*chunks.lock().unwrap(), vec![b"prompt$ ".to_vec()]);
    }

    #[test]
    fn raw_chunk_is_sent_without_json_encoding() {
        let body = RawChunk(vec![1, 2, 3]).body().unwrap();
        assert!(matches!(body, InvokeResponseBody::Raw(ref bytes) if bytes == &vec![1, 2, 3]));
    }
}
```

Run: `cargo test -p linascp output::`
Expected: FAIL to compile — `cannot find type OutputPump`, `OutputBuffer`, `Watermark`, `FlowGate`, `RawChunk` …

- [ ] **Step 3: Implement the output transport**

**`src-tauri/src/output.rs`** (implementation — insert above the `#[cfg(test)]` module)
```rust
//! Terminal output transport: coalescing (8 ms / 64 KiB) and ack-based flow control.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};

use crate::util::lock;

/// A pending chunk is emitted as soon as it reaches this size.
pub const MAX_CHUNK: usize = 64 * 1024;
/// Pending output is flushed at least this often.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(8);
/// The session reader is paused when this many bytes are unacknowledged by the UI …
pub const HIGH_WATERMARK: u64 = 1024 * 1024;
/// … and resumed once the unacknowledged bytes dropped to this value.
pub const LOW_WATERMARK: u64 = 256 * 1024;

/// Raw terminal bytes, sent through a `Channel` without JSON encoding (JS receives an `ArrayBuffer`).
pub struct RawChunk(pub Vec<u8>);

impl IpcResponse for RawChunk {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(InvokeResponseBody::Raw(self.0))
    }
}

impl specta::Type for RawChunk {
    fn definition(_types: &mut specta::Types) -> specta::datatype::DataType {
        specta::datatype::DataType::Reference(specta_typescript::define("ArrayBuffer"))
    }
}

/// Coalesces small writes into larger chunks.
#[derive(Debug, Default)]
pub struct OutputBuffer {
    buf: Vec<u8>,
}

impl OutputBuffer {
    /// Appends `data`; returns the whole buffer once it reached `MAX_CHUNK`.
    pub fn push(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        self.buf.extend_from_slice(data);
        (self.buf.len() >= MAX_CHUNK).then(|| std::mem::take(&mut self.buf))
    }

    /// Takes everything pending; `None` when empty.
    pub fn take(&mut self) -> Option<Vec<u8>> {
        (!self.buf.is_empty()).then(|| std::mem::take(&mut self.buf))
    }
}

/// Hysteresis between the high and the low watermark.
#[derive(Debug, Clone, Copy)]
pub struct Watermark {
    unacked: u64,
    paused: bool,
    high: u64,
    low: u64,
}

impl Watermark {
    pub fn new(high: u64, low: u64) -> Self {
        Self { unacked: 0, paused: false, high, low }
    }

    pub fn sent(&mut self, bytes: u64) {
        self.unacked += bytes;
        if self.unacked >= self.high {
            self.paused = true;
        }
    }

    pub fn acked(&mut self, bytes: u64) {
        self.unacked = self.unacked.saturating_sub(bytes);
        if self.paused && self.unacked <= self.low {
            self.paused = false;
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn unacked(&self) -> u64 {
        self.unacked
    }
}

struct GateState {
    mark: Watermark,
    closed: bool,
}

/// Thread-blocking wrapper around [`Watermark`].
pub struct FlowGate {
    state: Mutex<GateState>,
    resumed: Condvar,
}

impl FlowGate {
    pub fn new(high: u64, low: u64) -> Self {
        Self { state: Mutex::new(GateState { mark: Watermark::new(high, low), closed: false }), resumed: Condvar::new() }
    }

    pub fn sent(&self, bytes: u64) {
        lock(&self.state).mark.sent(bytes);
    }

    pub fn ack(&self, bytes: u64) {
        lock(&self.state).mark.acked(bytes);
        self.resumed.notify_all();
    }

    pub fn close(&self) {
        lock(&self.state).closed = true;
        self.resumed.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        lock(&self.state).mark.is_paused()
    }

    pub fn is_closed(&self) -> bool {
        lock(&self.state).closed
    }

    /// Blocks the calling thread while paused and not closed.
    pub fn wait_until_open(&self) {
        let mut state = lock(&self.state);
        while state.mark.is_paused() && !state.closed {
            state = self.resumed.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

pub type ChunkSender = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

/// Per-session output pipeline: session reader → buffer → IPC channel, with back-pressure.
pub struct OutputPump {
    buffer: Mutex<OutputBuffer>,
    send: ChunkSender,
    gate: FlowGate,
}

impl OutputPump {
    pub fn new(send: ChunkSender) -> Arc<Self> {
        Arc::new(Self {
            buffer: Mutex::new(OutputBuffer::default()),
            send,
            gate: FlowGate::new(HIGH_WATERMARK, LOW_WATERMARK),
        })
    }

    /// Pump writing into a raw IPC channel, flushed by a timer. Must be called inside a Tokio runtime.
    pub fn for_channel(channel: Channel<RawChunk>) -> Arc<Self> {
        let pump = Self::new(Arc::new(move |chunk: Vec<u8>| {
            if let Err(error) = channel.send(RawChunk(chunk)) {
                tracing::debug!(%error, "terminal output dropped: channel closed");
            }
        }));
        pump.start_ticker();
        pump
    }

    /// Spawns the periodic flush task; it ends when the pump is closed or dropped.
    pub fn start_ticker(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let Some(pump) = weak.upgrade() else { break };
                pump.flush();
                if pump.is_closed() {
                    break;
                }
            }
        });
    }

    /// Called by the session reader for every output chunk. Blocks while the UI is too far behind.
    pub fn push(&self, data: &[u8]) {
        {
            let mut buffer = lock(&self.buffer);
            if let Some(chunk) = buffer.push(data) {
                self.emit(chunk);
            }
        }
        self.wait_for_capacity();
    }

    pub fn flush(&self) {
        let mut buffer = lock(&self.buffer);
        if let Some(chunk) = buffer.take() {
            self.emit(chunk);
        }
    }

    /// The UI consumed `bytes` (xterm write callback).
    pub fn ack(&self, bytes: u64) {
        self.gate.ack(bytes);
    }

    /// Flushes what is pending and releases a blocked reader for good.
    pub fn close(&self) {
        self.flush();
        self.gate.close();
    }

    pub fn is_closed(&self) -> bool {
        self.gate.is_closed()
    }

    /// Called with the buffer lock held, so chunks can never overtake each other.
    fn emit(&self, chunk: Vec<u8>) {
        self.gate.sent(chunk.len() as u64);
        (self.send)(chunk);
    }

    fn wait_for_capacity(&self) {
        if !self.gate.is_paused() {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| self.gate.wait_until_open())
            }
            // Blocking a current-thread runtime would dead-lock it; skip back-pressure there.
            Ok(_) => tracing::debug!("output flow control skipped on a current-thread runtime"),
            Err(_) => self.gate.wait_until_open(),
        }
    }
}
```

Run: `cargo test -p linascp output::`
Expected: PASS (9 tests).

- [ ] **Step 4: Extend AppState and write the failing session tests**

**`src-tauri/src/state.rs`**
```rust
//! Application state shared by all Tauri commands.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use linascp_core::connect::{ConnectEvent, PromptBroker};
use linascp_core::error::Result;
use linascp_core::model::Host;
use linascp_core::paths::AppPaths;
use linascp_core::secrets::{KeyringSecretStore, SecretStore};
use linascp_core::session::{SessionId, SessionManager};
use linascp_core::sshconfig::{import_ssh_config, merge_hosts};
use linascp_core::store::Store;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::output::OutputPump;

pub struct AppState {
    pub paths: AppPaths,
    pub store: Arc<Store>,
    pub secrets: Arc<dyn SecretStore>,
    pub sessions: SessionManager,
    /// Global prompt broker; prompts are published through each call's own emit
    /// (`ask_cancellable`), so its constructor emit is a no-op.
    pub broker: Arc<PromptBroker>,
    /// Running connection attempts keyed by the UI-generated connect id.
    pub connects: Mutex<HashMap<Uuid, CancellationToken>>,
    /// Output pumps of open sessions (needed by `session_ack` / `session_close`).
    pub outputs: Mutex<HashMap<SessionId, Arc<OutputPump>>>,
}

impl AppState {
    pub fn new(paths: AppPaths, store: Store, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            paths,
            store: Arc::new(store),
            secrets,
            sessions: SessionManager::default(),
            broker: Arc::new(PromptBroker::new(Arc::new(|_event: ConnectEvent| {}))),
            connects: Mutex::new(HashMap::new()),
            outputs: Mutex::new(HashMap::new()),
        }
    }

    /// Production state: XDG paths, on-disk SQLite store, OS keyring.
    pub fn init() -> Result<Self> {
        let paths = AppPaths::from_env()?;
        paths.ensure_dirs()?;
        let store = Store::open(&paths.db_path())?;
        Ok(Self::new(paths, store, Arc::new(KeyringSecretStore::new())))
    }

    /// Stored hosts merged with the read-only `~/.ssh/config` import (stored wins on equal name).
    /// A broken ssh config never blocks the app: it is logged and ignored.
    pub fn merged_hosts(&self) -> Result<Vec<Host>> {
        let stored = self.store.list_hosts()?;
        let path = self.paths.ssh_config();
        let imported = if path.exists() {
            import_ssh_config(&path).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %path.display(), "ignoring unreadable ssh config");
                Vec::new()
            })
        } else {
            Vec::new()
        };
        Ok(merge_hosts(stored, imported))
    }

    #[cfg(test)]
    pub fn for_tests(root: &std::path::Path) -> Self {
        use linascp_core::secrets::MemorySecretStore;
        let paths = AppPaths::for_root(root);
        std::fs::create_dir_all(&paths.ssh_dir).expect("create test ssh dir");
        Self::new(paths, Store::open_in_memory().expect("in-memory store"), Arc::new(MemorySecretStore::new()))
    }
}
```

**`src-tauri/src/commands/mod.rs`**
```rust
pub mod connect;
pub mod hosts;
pub mod profiles;
pub mod secrets;
pub mod sessions;
pub mod settings;

use linascp_core::error::{CoreError, Result};

/// Runs blocking work (SQLite bulk work, keyring D-Bus calls, file IO) off the async runtime.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CoreError::Internal { message: format!("background task failed: {e}") })?
}
```

**`src-tauri/src/commands/sessions.rs`** (tests)
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    type Bytes = Arc<Mutex<Vec<u8>>>;
    type Events = Arc<Mutex<Vec<SessionEventDto>>>;

    fn collecting_pump() -> (Arc<OutputPump>, Bytes) {
        let bytes: Bytes = Arc::default();
        let sink = bytes.clone();
        let pump = OutputPump::new(Arc::new(move |chunk: Vec<u8>| sink.lock().unwrap().extend_from_slice(&chunk)));
        (pump, bytes)
    }

    fn collecting_events() -> (EventSender, Events) {
        let events: Events = Arc::default();
        let sink = events.clone();
        (Arc::new(move |event: SessionEventDto| sink.lock().unwrap().push(event)), events)
    }

    #[test]
    fn sink_routes_output_and_flushes_before_exit() {
        let (pump, bytes) = collecting_pump();
        let (send, events) = collecting_events();
        let sink = make_sink(pump.clone(), send);

        sink(SessionEvent::Output(b"hello".to_vec()));
        assert!(bytes.lock().unwrap().is_empty(), "small output waits for the flush tick");

        sink(SessionEvent::Exit { code: Some(0) });
        assert_eq!(*bytes.lock().unwrap(), b"hello".to_vec());
        assert!(pump.is_closed());
        assert!(matches!(events.lock().unwrap().as_slice(), [SessionEventDto::Exit { code: Some(0) }]));
    }

    #[test]
    fn sink_reports_session_errors() {
        let (pump, _bytes) = collecting_pump();
        let (send, events) = collecting_events();
        make_sink(pump, send)(SessionEvent::Error(CoreError::Cancelled));
        assert!(matches!(
            events.lock().unwrap().as_slice(),
            [SessionEventDto::Error { error: CoreError::Cancelled }]
        ));
    }

    #[test]
    fn session_event_dto_uses_kind_tag() {
        let exit = serde_json::to_value(SessionEventDto::Exit { code: Some(3) }).unwrap();
        assert_eq!(exit, serde_json::json!({ "kind": "exit", "code": 3 }));
        let error = serde_json::to_value(SessionEventDto::Error { error: CoreError::Cancelled }).unwrap();
        assert_eq!(error, serde_json::json!({ "kind": "error", "error": { "type": "cancelled" } }));
    }

    #[tokio::test]
    async fn unknown_sessions_are_reported_and_closing_them_is_harmless() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let id = Uuid::new_v4();
        assert!(matches!(write(&state, id, b"x").await, Err(CoreError::Internal { .. })));
        assert!(matches!(resize(&state, id, 80, 24).await, Err(CoreError::Internal { .. })));
        ack(&state, id, 10);
        close(&state, id).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_pty_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let (pump, bytes) = collecting_pump();
        pump.start_ticker();
        let (send, _events) = collecting_events();

        let id = open_local(&state, 80, 24, pump, send).unwrap();
        write(&state, id, b"echo linascp-$((40+2))\n").await.unwrap();

        let mut found = false;
        for _ in 0..200 {
            if String::from_utf8_lossy(&bytes.lock().unwrap()).contains("linascp-42") {
                found = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(found, "shell output not received");

        resize(&state, id, 100, 30).await.unwrap();
        ack(&state, id, 64);
        close(&state, id).await.unwrap();
        assert!(state.sessions.get(id).is_none());
        assert!(state.outputs.lock().unwrap().get(&id).is_none());
    }
}
```

`commands/mod.rs` already declares `connect`; until Steps 6–7 fill it, create the module with a single placeholder line (Step 7 deletes it):

**`src-tauri/src/commands/connect.rs`**
```rust
//! Connection commands (implemented in Task 14 Step 7).
```

Run: `cargo test -p linascp commands::sessions::`
Expected: FAIL to compile — `cannot find function make_sink`, `open_local`, type `SessionEventDto`, `EventSender` …

- [ ] **Step 5: Implement the session commands**

**`src-tauri/src/commands/sessions.rs`** (implementation — insert above the `#[cfg(test)]` module)
```rust
use std::sync::Arc;

use linascp_core::error::{CoreError, Result};
use linascp_core::session::{LocalPtySession, OutputSink, Session, SessionEvent, SessionId};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;
use uuid::Uuid;

use crate::output::{OutputPump, RawChunk};
use crate::state::AppState;
use crate::util::lock;

/// Non-output session events, sent on the `session_events` channel.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionEventDto {
    Exit { code: Option<u32> },
    Error { error: CoreError },
}

pub(crate) type EventSender = Arc<dyn Fn(SessionEventDto) + Send + Sync>;

pub(crate) fn channel_event_sender(channel: Channel<SessionEventDto>) -> EventSender {
    Arc::new(move |event: SessionEventDto| {
        if let Err(error) = channel.send(event) {
            tracing::debug!(%error, "session event dropped: channel closed");
        }
    })
}

/// Core `OutputSink` that feeds the pump and reports exit/error after flushing pending output.
pub(crate) fn make_sink(pump: Arc<OutputPump>, send_event: EventSender) -> OutputSink {
    Arc::new(move |event: SessionEvent| match event {
        SessionEvent::Output(bytes) => pump.push(&bytes),
        SessionEvent::Exit { code } => {
            pump.close();
            send_event(SessionEventDto::Exit { code });
        }
        SessionEvent::Error(error) => {
            pump.close();
            send_event(SessionEventDto::Error { error });
        }
    })
}

pub(crate) fn register(state: &AppState, id: SessionId, session: Arc<dyn Session>, pump: Arc<OutputPump>) {
    state.sessions.insert(id, session);
    lock(&state.outputs).insert(id, pump);
}

fn get_session(state: &AppState, id: SessionId) -> Result<Arc<dyn Session>> {
    state.sessions.get(id).ok_or_else(|| CoreError::Internal { message: format!("unknown session {id}") })
}

pub(crate) fn open_local(
    state: &AppState,
    cols: u16,
    rows: u16,
    pump: Arc<OutputPump>,
    send_event: EventSender,
) -> Result<SessionId> {
    let session = LocalPtySession::spawn(cols, rows, make_sink(pump.clone(), send_event))?;
    let id = Uuid::new_v4();
    register(state, id, Arc::new(session), pump);
    Ok(id)
}

pub(crate) async fn write(state: &AppState, id: SessionId, data: &[u8]) -> Result<()> {
    get_session(state, id)?.write(data).await
}

pub(crate) async fn resize(state: &AppState, id: SessionId, cols: u16, rows: u16) -> Result<()> {
    get_session(state, id)?.resize(cols, rows).await
}

/// Unknown ids are ignored (the session may just have been closed).
pub(crate) fn ack(state: &AppState, id: SessionId, bytes: u32) {
    let pump = lock(&state.outputs).get(&id).cloned();
    if let Some(pump) = pump {
        pump.ack(u64::from(bytes));
    }
}

/// Closing an unknown session is not an error.
pub(crate) async fn close(state: &AppState, id: SessionId) -> Result<()> {
    let session = state.sessions.get(id);
    state.sessions.remove(id);
    let pump = lock(&state.outputs).remove(&id);
    if let Some(pump) = pump {
        pump.close();
    }
    match session {
        Some(session) => session.close().await,
        None => Ok(()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn session_open_local(
    state: State<'_, AppState>,
    cols: u16,
    rows: u16,
    output: Channel<RawChunk>,
    session_events: Channel<SessionEventDto>,
) -> Result<SessionId> {
    open_local(&state, cols, rows, OutputPump::for_channel(output), channel_event_sender(session_events))
}

#[tauri::command]
#[specta::specta]
pub async fn session_write(state: State<'_, AppState>, session_id: SessionId, data: Vec<u8>) -> Result<()> {
    write(&state, session_id, &data).await
}

#[tauri::command]
#[specta::specta]
pub async fn session_resize(state: State<'_, AppState>, session_id: SessionId, cols: u16, rows: u16) -> Result<()> {
    resize(&state, session_id, cols, rows).await
}

#[tauri::command]
#[specta::specta]
pub async fn session_ack(state: State<'_, AppState>, session_id: SessionId, bytes: u32) -> Result<()> {
    ack(&state, session_id, bytes);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn session_close(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    close(&state, session_id).await
}
```

Run: `cargo test -p linascp commands::sessions::`
Expected: PASS (5 tests; `local_pty_roundtrip` spawns `$SHELL`).

- [ ] **Step 6: Write the failing connect tests**

**`src-tauri/src/commands/connect.rs`** (tests)
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::hosts::save_host;
    use crate::commands::profiles::{save_profile, tests::profile};
    use linascp_core::connect::Prompt;
    use std::sync::Mutex;
    use std::time::Duration;

    fn setup() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        (dir, state)
    }

    #[test]
    fn build_request_resolves_profile_and_jump_host() {
        let (_dir, state) = setup();
        let p = save_profile(&state, profile("deploy")).unwrap();
        let bastion = save_host(&state, Host::new("bastion", "192.0.2.1")).unwrap();
        let mut web = Host::new("web-prod-01", "10.0.1.12");
        web.auth = HostAuth::Profile { profile_id: p.id };
        web.jump_host_id = Some(bastion.id);
        let web = save_host(&state, web).unwrap();

        let req = build_request(&state, find_host(&state, web.id).unwrap(), 140, 38).unwrap();
        assert_eq!(req.target.host.id, web.id);
        assert_eq!(req.target.profile.as_ref().map(|p| p.id), Some(p.id));
        let jump = req.jump.expect("jump target");
        assert_eq!(jump.host.id, bastion.id);
        assert!(jump.profile.is_none());
        assert_eq!((req.cols, req.rows), (140, 38));
    }

    #[test]
    fn missing_profile_or_jump_host_is_a_config_error() {
        let (_dir, state) = setup();
        let mut host = Host::new("orphan", "192.0.2.2");
        host.auth = HostAuth::Profile { profile_id: Uuid::new_v4() };
        assert!(matches!(build_request(&state, host, 80, 24), Err(CoreError::Config { .. })));

        let mut host = Host::new("lost-jump", "192.0.2.3");
        host.jump_host_id = Some(Uuid::new_v4());
        assert!(matches!(build_request(&state, host, 80, 24), Err(CoreError::Config { .. })));

        assert!(matches!(find_host(&state, Uuid::new_v4()), Err(CoreError::Config { .. })));
    }

    #[test]
    fn cancel_triggers_only_the_registered_token() {
        let (_dir, state) = setup();
        let connect_id = Uuid::new_v4();
        let token = register_connect(&state, connect_id);

        cancel_connect(&state, Uuid::new_v4());
        assert!(!token.is_cancelled(), "other ids must not cancel");
        cancel_connect(&state, connect_id);
        assert!(token.is_cancelled());

        unregister_connect(&state, connect_id);
        assert!(state.connects.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn answers_reach_the_global_broker() {
        let (_dir, state) = setup();
        let state = Arc::new(state);
        let events: Arc<Mutex<Vec<ConnectEvent>>> = Arc::default();
        let sink = events.clone();
        let emit: EmitFn = Arc::new(move |e: ConnectEvent| sink.lock().unwrap().push(e));

        let asking = {
            let state = state.clone();
            tokio::spawn(async move {
                let prompt = Prompt::Passphrase { key_path: "/tmp/key".into() };
                state.broker.ask_cancellable(&emit, prompt, &CancellationToken::new()).await
            })
        };
        let prompt_id = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let found = events.lock().unwrap().iter().find_map(|e| match e {
                    ConnectEvent::Prompt { prompt_id, .. } => Some(*prompt_id),
                    _ => None,
                });
                if let Some(id) = found {
                    break id;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("prompt event");

        assert!(!state.broker.answer(prompt_id + 1000, PromptAnswer::Cancel), "unknown prompt id");
        assert!(state.broker.answer(prompt_id, PromptAnswer::Secret { value: "pp".into(), save: false }));
        let answer = asking.await.unwrap();
        assert!(matches!(answer, PromptAnswer::Secret { ref value, save: false } if value == "pp"));
    }
}
```

Run: `cargo test -p linascp commands::connect::`
Expected: FAIL to compile — `cannot find function build_request`, `find_host`, `register_connect`, `cancel_connect`, `unregister_connect` …

- [ ] **Step 7: Implement the connect commands**

**`src-tauri/src/commands/connect.rs`** (implementation — insert above the `#[cfg(test)]` module; delete the one-line placeholder comment from Step 4)
```rust
use std::path::PathBuf;
use std::sync::Arc;

use linascp_core::connect::{
    ConnectDeps, ConnectEvent, ConnectRequest, ConnectTarget, EmitFn, PromptAnswer, connect_ssh as run_pipeline,
    test_connection,
};
use linascp_core::error::{CoreError, Result};
use linascp_core::known_hosts::KnownHosts;
use linascp_core::model::{Host, HostAuth, Id};
use linascp_core::session::SessionId;
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::sessions::{SessionEventDto, channel_event_sender, make_sink, register};
use crate::output::{OutputPump, RawChunk};
use crate::state::AppState;
use crate::util::lock;

/// Looks a host up in the merged list (stored + ssh-config).
pub(crate) fn find_host(state: &AppState, id: Id) -> Result<Host> {
    state
        .merged_hosts()?
        .into_iter()
        .find(|h| h.id == id)
        .ok_or_else(|| CoreError::Config { message: format!("host {id} not found") })
}

fn target_for(state: &AppState, host: Host) -> Result<ConnectTarget> {
    let profile = match &host.auth {
        HostAuth::Profile { profile_id } => Some(state.store.get_profile(*profile_id)?.ok_or_else(|| {
            CoreError::Config { message: format!("login profile {profile_id} of host {} not found", host.name) }
        })?),
        HostAuth::Custom { .. } => None,
    };
    Ok(ConnectTarget { host, profile })
}

/// Resolves the login profile and the (single-hop) jump host of `host`.
pub(crate) fn build_request(state: &AppState, host: Host, cols: u16, rows: u16) -> Result<ConnectRequest> {
    let jump = match host.jump_host_id {
        Some(jump_id) => {
            let jump_host = find_host(state, jump_id).map_err(|_| CoreError::Config {
                message: format!("jump host {jump_id} of host {} not found", host.name),
            })?;
            Some(target_for(state, jump_host)?)
        }
        None => None,
    };
    Ok(ConnectRequest { target: target_for(state, host)?, jump, cols, rows })
}

/// Registers a cancellable connection attempt under the UI-generated `connect_id`.
pub(crate) fn register_connect(state: &AppState, connect_id: Uuid) -> CancellationToken {
    let token = CancellationToken::new();
    lock(&state.connects).insert(connect_id, token.clone());
    token
}

pub(crate) fn unregister_connect(state: &AppState, connect_id: Uuid) {
    lock(&state.connects).remove(&connect_id);
}

/// Unknown ids are ignored (the attempt may just have finished).
pub(crate) fn cancel_connect(state: &AppState, connect_id: Uuid) {
    if let Some(token) = lock(&state.connects).get(&connect_id) {
        token.cancel();
    }
}

fn connect_deps(state: &AppState) -> ConnectDeps {
    ConnectDeps {
        known_hosts: KnownHosts::new(state.paths.known_hosts()),
        secrets: state.secrets.clone(),
        agent_sock: std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from),
    }
}

fn channel_emit(channel: Channel<ConnectEvent>) -> EmitFn {
    Arc::new(move |event: ConnectEvent| {
        if let Err(error) = channel.send(event) {
            tracing::debug!(%error, "connect event dropped: channel closed");
        }
    })
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
#[specta::specta]
pub async fn connect_ssh(
    state: State<'_, AppState>,
    connect_id: Uuid,
    host_id: Id,
    cols: u16,
    rows: u16,
    events: Channel<ConnectEvent>,
    output: Channel<RawChunk>,
    session_events: Channel<SessionEventDto>,
) -> Result<SessionId> {
    let request = build_request(&state, find_host(&state, host_id)?, cols, rows)?;
    let emit = channel_emit(events);
    let pump = OutputPump::for_channel(output);
    let sink = make_sink(pump.clone(), channel_event_sender(session_events));
    let cancel = register_connect(&state, connect_id);
    let result = run_pipeline(request, connect_deps(&state), state.broker.clone(), emit.clone(), sink, cancel).await;
    unregister_connect(&state, connect_id);
    match result {
        Ok(session) => {
            let session_id = Uuid::new_v4();
            register(&state, session_id, Arc::new(session), pump);
            // core emits `Failed` on errors; the success event is ours (exactly one terminal event).
            emit(ConnectEvent::Connected { session_id });
            Ok(session_id)
        }
        Err(error) => {
            pump.close();
            Err(error)
        }
    }
}

/// Returns `false` when the prompt id is unknown (already answered, timed out or cancelled).
#[tauri::command]
#[specta::specta]
pub async fn connect_answer(state: State<'_, AppState>, prompt_id: u64, answer: PromptAnswer) -> Result<bool> {
    Ok(state.broker.answer(prompt_id, answer))
}

#[tauri::command]
#[specta::specta]
pub async fn connect_cancel(state: State<'_, AppState>, connect_id: Uuid) -> Result<()> {
    cancel_connect(&state, connect_id);
    Ok(())
}

/// "Verbindung testen": pipeline for an (unsaved) host up to Auth, then disconnect (core `test_connection`).
#[tauri::command]
#[specta::specta]
pub async fn connect_test(
    state: State<'_, AppState>,
    connect_id: Uuid,
    host: Host,
    events: Channel<ConnectEvent>,
) -> Result<()> {
    let request = build_request(&state, host, 80, 24)?;
    let cancel = register_connect(&state, connect_id);
    let result = test_connection(request, connect_deps(&state), state.broker.clone(), channel_emit(events), cancel).await;
    unregister_connect(&state, connect_id);
    result
}
```

Run: `cargo test -p linascp commands::connect::`
Expected: PASS (4 tests).

- [ ] **Step 8: Extend the bindings test (failing)**

In `src-tauri/src/lib.rs`, add these assertions at the end of `exports_bindings` (after the `SecretOwner` assertion):

```rust
        assert!(ts.contains("sessionAck: (sessionId: string, bytes: number)"));
        assert!(ts.contains("output: Channel<ArrayBuffer>"));
        assert!(ts.contains("connectAnswer: (promptId: number, answer: PromptAnswer)"));
        assert!(ts.contains("export type SessionEventDto ="));
```

Run: `cargo test -p linascp exports_bindings`
Expected: FAIL — assertion `ts.contains("sessionAck: …")` fails (commands not registered yet).

- [ ] **Step 9: Register the new commands**

**`src-tauri/src/lib.rs`**
```rust
pub mod commands;
pub mod dto;
pub mod output;
pub mod state;
mod util;

use commands::{connect, hosts, profiles, secrets, sessions, settings};

/// Where the generated TypeScript bindings live (committed to git).
pub const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/ipc/bindings.ts");

/// All IPC commands with their specta type information.
pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            hosts::hosts_list,
            hosts::host_save,
            hosts::host_delete,
            hosts::groups_list,
            hosts::group_save,
            hosts::group_delete,
            profiles::profiles_list,
            profiles::profile_save,
            profiles::profile_delete,
            secrets::secret_set,
            secrets::secret_delete,
            settings::settings_get,
            settings::settings_set,
            connect::connect_ssh,
            connect::connect_answer,
            connect::connect_cancel,
            connect::connect_test,
            sessions::session_open_local,
            sessions::session_write,
            sessions::session_resize,
            sessions::session_ack,
            sessions::session_close,
        ])
        .dangerously_cast_bigints_to_number()
}

pub fn export_bindings(builder: &tauri_specta::Builder<tauri::Wry>) -> Result<(), specta_typescript::Error> {
    builder.export(specta_typescript::Typescript::default(), BINDINGS_PATH)
}

pub fn run() {
    let state = match state::AppState::init() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("LinaSCP: startup failed: {error}");
            std::process::exit(1);
        }
    };
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    if let Err(error) = export_bindings(&builder) {
        eprintln!("LinaSCP: failed to export TypeScript bindings: {error}");
    }
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running LinaSCP");
}

#[cfg(test)]
mod tests {
    #[test]
    fn exports_bindings() {
        super::export_bindings(&super::specta_builder()).expect("export bindings");
        let ts = std::fs::read_to_string(super::BINDINGS_PATH).unwrap();
        assert!(ts.contains("hostsList: () =>"));
        assert!(ts.contains("export type CoreError ="));
        assert!(ts.contains("export type Settings ="));
        assert!(ts.contains("export type SecretOwner ="));
        assert!(ts.contains("sessionAck: (sessionId: string, bytes: number)"));
        assert!(ts.contains("output: Channel<ArrayBuffer>"));
        assert!(ts.contains("connectAnswer: (promptId: number, answer: PromptAnswer)"));
        assert!(ts.contains("export type SessionEventDto ="));
    }
}
```

Run: `cargo fmt -p linascp && cargo test -p linascp && cargo clippy -p linascp --all-targets -- -D warnings`
Expected: all PASS (Task 13 tests + output 9, sessions 5, connect 4, lib 1); clippy clean; `ui/src/ipc/bindings.ts` regenerated with the session/connect commands.

- [ ] **Step 10: Commit**

```bash
git add src-tauri ui/src/ipc/bindings.ts Cargo.lock
git commit -m "feat(tauri): add session/connect commands with batched raw output and ack flow control

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 15: Export/import commands, start-up (logging, WebKit workaround), dialog plugin

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `tracing-subscriber`, `tracing-appender`, `tauri-plugin-dialog`)
- Modify (full replacement): `src-tauri/capabilities/default.json`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`
- Create: `src-tauri/src/commands/transfer.rs`, `src-tauri/src/startup.rs`
- Regenerate: `ui/src/ipc/bindings.ts`
- Test: unit tests in `transfer.rs`, `startup.rs`, `lib.rs`

**Interfaces:**
- Consumes (core): `export::{export_data, import_preview, import_apply, ExportOptions, ImportPreview, ImportReport, Resolution, ItemKind}`, `AppPaths::{imported_keys_dir, log_dir}`; Task 13 `blocking`, `sync_mirror`, `load_settings`, `AppState::merged_hosts`.
- Produces (Rust):
  - `transfer.rs`: `write_private(path, bytes)` (mode 0600), `export_to(&AppState, PathBuf, include_secrets, include_key_files, Option<String>)`, `preview_from(&AppState, PathBuf)`, `apply_from(&AppState, PathBuf, Option<String>, HashMap<Id, Resolution>)`; commands `export_file(path, include_secrets, include_key_files, passphrase) -> ()`, `import_preview(path) -> ImportPreview`, `import_apply(path, passphrase, resolutions) -> ImportReport`, `ssh_config_reload() -> Vec<Host>`.
  - `startup.rs`: `COMPAT_ENV`, `LOG_FILES_KEPT = 7`, `workaround_env(compat_rendering: bool, current: impl Fn(&str) -> Option<String>) -> Vec<(String, String)>`, `apply_webkit_workarounds(&Store)`, `file_appender(&Path) -> Result<RollingFileAppender, String>`, `init_logging(&Path) -> Option<WorkerGuard>`.
  - `run()` order: `AppState::init()` → `apply_webkit_workarounds` → `init_logging` → bindings export (debug) → `tauri::Builder` with `tauri_plugin_dialog`.
- Produces (TS, generated): `commands.exportFile(path, includeSecrets, includeKeyFiles, passphrase: string | null)`, `commands.importPreview(path)`, `commands.importApply(path, passphrase: string | null, resolutions: { [key in string]: Resolution })`, `commands.sshConfigReload()`.

- [ ] **Step 1: Add the dependencies and the dialog permission**

In `src-tauri/Cargo.toml`, add under `[dependencies]`:

```toml
tauri-plugin-dialog = "2.8"
tracing-appender = "0.2"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
```

**`src-tauri/capabilities/default.json`**
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Permissions of the main window",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:default"]
}
```

**`src-tauri/src/commands/mod.rs`**
```rust
pub mod connect;
pub mod hosts;
pub mod profiles;
pub mod secrets;
pub mod sessions;
pub mod settings;
pub mod transfer;

use linascp_core::error::{CoreError, Result};

/// Runs blocking work (SQLite bulk work, keyring D-Bus calls, file IO) off the async runtime.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CoreError::Internal { message: format!("background task failed: {e}") })?
}
```

- [ ] **Step 2: Write the failing export/import tests**

**`src-tauri/src/commands/transfer.rs`** (tests)
```rust
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
```

Run: `cargo test -p linascp commands::transfer::`
Expected: FAIL to compile — `cannot find function export_to`, `preview_from`, `apply_from`, `write_private` …

- [ ] **Step 3: Implement export/import and `ssh_config_reload`**

**`src-tauri/src/commands/transfer.rs`** (implementation — insert above the `#[cfg(test)]` module)
```rust
use std::collections::HashMap;
use std::fs::{OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use linascp_core::error::{CoreError, Result};
use linascp_core::export::{self as core_export, ExportOptions, ImportPreview, ImportReport, Resolution};
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
) -> Result<()> {
    let store = state.store.clone();
    let secrets = state.secrets.clone();
    blocking(move || {
        let options = ExportOptions { include_secrets, include_key_files, passphrase: passphrase.map(SecretString::from) };
        let bytes = core_export::export_data(&store, secrets.as_ref(), &options)?;
        write_private(&path, &bytes)
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
) -> Result<()> {
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
```

Run: `cargo test -p linascp commands::transfer::`
Expected: PASS (4 tests).

- [ ] **Step 4: Write the failing start-up tests**

**`src-tauri/src/startup.rs`** (tests)
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |key: &str| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn no_workaround_without_compat_rendering() {
        assert!(workaround_env(false, env_of(&[])).is_empty());
    }

    #[test]
    fn compat_rendering_disables_the_dmabuf_renderer() {
        assert_eq!(
            workaround_env(true, env_of(&[])),
            vec![("WEBKIT_DISABLE_DMABUF_RENDERER".to_string(), "1".to_string())]
        );
    }

    #[test]
    fn user_provided_values_are_never_overridden() {
        assert!(workaround_env(true, env_of(&[("WEBKIT_DISABLE_DMABUF_RENDERER", "0")])).is_empty());
    }

    #[test]
    fn file_appender_creates_the_log_directory_and_a_dated_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("state").join("logs");
        let appender = file_appender(&log_dir).unwrap();
        drop(appender);
        let names: Vec<String> = std::fs::read_dir(&log_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n.starts_with("linascp.") && n.ends_with(".log")), "{names:?}");
    }
}
```

Run: `cargo test -p linascp startup::`
Expected: FAIL to compile — `cannot find function workaround_env`, `file_appender`.

- [ ] **Step 5: Implement start-up helpers**

**`src-tauri/src/startup.rs`** (implementation — insert above the `#[cfg(test)]` module)
```rust
//! Process start-up: WebKit environment workarounds and logging.

use std::path::Path;

use linascp_core::store::Store;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder as RollingBuilder, RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::commands::settings::load_settings;

/// Applied only when the user enabled "compatibility rendering" (NVIDIA/Wayland DMABUF problems).
/// The 2026-10 spike showed WebGL runs fine without it and that it costs 10–13 % throughput.
pub const COMPAT_ENV: &[(&str, &str)] = &[("WEBKIT_DISABLE_DMABUF_RENDERER", "1")];

/// Daily log files kept in `~/.local/state/linascp/logs`.
pub const LOG_FILES_KEPT: usize = 7;

/// Which variables to set for the given switch; values the user already set are never overridden.
pub fn workaround_env(compat_rendering: bool, current: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    if !compat_rendering {
        return Vec::new();
    }
    COMPAT_ENV
        .iter()
        .filter(|&&(key, _)| current(key).is_none())
        .map(|&(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// Must run before the WebView is created and before any other thread is spawned.
pub fn apply_webkit_workarounds(store: &Store) {
    let compat = load_settings(store).map(|s| s.compat_rendering).unwrap_or(false);
    for (key, value) in workaround_env(compat, |key| std::env::var(key).ok()) {
        // SAFETY: called from `run()` before Tauri, logging or any other thread is started.
        unsafe { std::env::set_var(&key, &value) };
    }
}

/// `log_dir/linascp.YYYY-MM-DD.log`, rotated daily, `LOG_FILES_KEPT` files kept.
pub fn file_appender(log_dir: &Path) -> Result<RollingFileAppender, String> {
    std::fs::create_dir_all(log_dir).map_err(|e| e.to_string())?;
    RollingBuilder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("linascp")
        .filename_suffix("log")
        .max_log_files(LOG_FILES_KEPT)
        .build(log_dir)
        .map_err(|e| e.to_string())
}

/// Logs to stderr and to the rotating file. Filter via `LINASCP_LOG` (EnvFilter syntax).
/// The returned guard must live until the process ends (it flushes the file writer).
pub fn init_logging(log_dir: &Path) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_env("LINASCP_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,linascp_lib=debug,linascp_core=debug"));
    let (file_layer, guard) = match file_appender(log_dir) {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (Some(fmt::layer().with_ansi(false).with_writer(writer)), Some(guard))
        }
        Err(error) => {
            eprintln!("LinaSCP: file logging disabled: {error}");
            (None, None)
        }
    };
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .try_init();
    guard
}
```

Run: `cargo test -p linascp startup::`
Expected: PASS (4 tests).

- [ ] **Step 6: Extend the bindings test (failing)**

In `src-tauri/src/lib.rs`, add at the end of `exports_bindings`:

```rust
        assert!(ts.contains("exportFile: (path: string, includeSecrets: boolean, includeKeyFiles: boolean, passphrase: string | null)"));
        assert!(ts.contains("sshConfigReload: () =>"));
```

Run: `cargo test -p linascp exports_bindings`
Expected: FAIL — `exportFile` not found in the generated file.

- [ ] **Step 7: Final `lib.rs` with start-up sequence, dialog plugin and all commands**

**`src-tauri/src/lib.rs`**
```rust
pub mod commands;
pub mod dto;
pub mod output;
pub mod startup;
pub mod state;
mod util;

use commands::{connect, hosts, profiles, secrets, sessions, settings, transfer};

/// Where the generated TypeScript bindings live (committed to git).
pub const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/ipc/bindings.ts");

/// All IPC commands with their specta type information.
pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            hosts::hosts_list,
            hosts::host_save,
            hosts::host_delete,
            hosts::groups_list,
            hosts::group_save,
            hosts::group_delete,
            profiles::profiles_list,
            profiles::profile_save,
            profiles::profile_delete,
            secrets::secret_set,
            secrets::secret_delete,
            settings::settings_get,
            settings::settings_set,
            connect::connect_ssh,
            connect::connect_answer,
            connect::connect_cancel,
            connect::connect_test,
            sessions::session_open_local,
            sessions::session_write,
            sessions::session_resize,
            sessions::session_ack,
            sessions::session_close,
            transfer::export_file,
            transfer::import_preview,
            transfer::import_apply,
            transfer::ssh_config_reload,
        ])
        .dangerously_cast_bigints_to_number()
}

pub fn export_bindings(builder: &tauri_specta::Builder<tauri::Wry>) -> Result<(), specta_typescript::Error> {
    builder.export(specta_typescript::Typescript::default(), BINDINGS_PATH)
}

pub fn run() {
    // Opening the store first is deliberate: the compat-rendering flag must be known
    // before any thread exists and before WebKit is initialised.
    let state = match state::AppState::init() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("LinaSCP: startup failed: {error}");
            std::process::exit(1);
        }
    };
    startup::apply_webkit_workarounds(&state.store);
    let _log_guard = startup::init_logging(&state.paths.log_dir());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "LinaSCP starting");

    let builder = specta_builder();
    #[cfg(debug_assertions)]
    if let Err(error) = export_bindings(&builder) {
        tracing::warn!(%error, "failed to export TypeScript bindings");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running LinaSCP");
}

#[cfg(test)]
mod tests {
    #[test]
    fn exports_bindings() {
        super::export_bindings(&super::specta_builder()).expect("export bindings");
        let ts = std::fs::read_to_string(super::BINDINGS_PATH).unwrap();
        assert!(ts.contains("hostsList: () =>"));
        assert!(ts.contains("export type CoreError ="));
        assert!(ts.contains("export type Settings ="));
        assert!(ts.contains("export type SecretOwner ="));
        assert!(ts.contains("sessionAck: (sessionId: string, bytes: number)"));
        assert!(ts.contains("output: Channel<ArrayBuffer>"));
        assert!(ts.contains("connectAnswer: (promptId: number, answer: PromptAnswer)"));
        assert!(ts.contains("export type SessionEventDto ="));
        assert!(ts.contains("exportFile: (path: string, includeSecrets: boolean, includeKeyFiles: boolean, passphrase: string | null)"));
        assert!(ts.contains("sshConfigReload: () =>"));
    }
}
```

Run: `cargo fmt -p linascp && cargo test -p linascp && cargo clippy -p linascp --all-targets -- -D warnings && cargo build -p linascp`
Expected: all 43 tests PASS (Tasks 13–15), clippy clean, binary builds. `ui/src/ipc/bindings.ts` now contains all 26 commands.

- [ ] **Step 8: Commit**

```bash
git add src-tauri ui/src/ipc/bindings.ts Cargo.lock
git commit -m "feat(tauri): add export/import commands, logging, optional WebKit compat env and dialog plugin

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 16: UI scaffold (Vite + React 19 + TS strict), tokens, i18n, IPC wrappers

**Files:**
- Create: `package.json` (repository root — the only `package.json`; Vite root is `ui/`)
- Modify: `.gitignore` (append `node_modules/`, `ui/dist/`)
- Create: `ui/index.html`, `ui/vite.config.ts`, `ui/vitest.config.ts`, `ui/tsconfig.json`
- Create: `ui/src/main.tsx`, `ui/src/App.tsx`, `ui/src/styles/tokens.css` (verbatim from the design zip), `ui/src/styles/global.css`, `ui/src/i18n/de.ts`, `ui/src/test/setup.ts`
- Create: `ui/src/ipc/errors.ts`, `ui/src/ipc/api.ts`
- Test: `ui/src/App.test.tsx`, `ui/src/ipc/errors.test.ts`, `ui/src/ipc/api.test.ts`

**Interfaces:**
- Consumes: `ui/src/ipc/bindings.ts` (generated in Tasks 13–15; never edit by hand).
- Produces:
  - Scripts (run from the repo root): `bun run dev` (Vite on :5173), `bun run build` (typecheck + `ui/dist`), `bun run typecheck`, `bun run test [filter]`, `bun run tauri dev|build`.
  - `ui/src/i18n/de.ts`: `export const t = { app, common, rail, toast, steps, shell, stubs } as const` (later tasks add one top-level group per screen, e.g. `t.hosts`, `t.editor`, `t.connect`, `t.terminal`, `t.settings`).
  - `ui/src/ipc/errors.ts`: `isCoreError(value: unknown): value is CoreError`, `errorMessage(error: unknown): string`, `errorDetails(error: unknown): string`.
  - `ui/src/ipc/api.ts`: type re-exports of every binding type + `Id`, `SessionId`; `newId()`; one function per command — `hostsList, hostSave, hostDelete, groupsList, groupSave, groupDelete, profilesList, profileSave, profileDelete, secretSet, secretDelete, settingsGet, settingsSet, connectSsh, connectAnswer, connectCancel, connectTest, sessionOpenLocal, sessionWrite, sessionResize, sessionAck, sessionClose, exportFile, importPreview, importApply, sshConfigReload` (exact signatures in Step 9); channel helpers `createEventChannel<T>(onEvent)`, `createOutputChannel(onData)`, `createOutputStream(): OutputStream`.

- [ ] **Step 1: Root `package.json` and install**

**`package.json`**
```json
{
  "name": "linascp",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite --config ui/vite.config.ts",
    "build": "tsc -p ui/tsconfig.json && vite build --config ui/vite.config.ts",
    "typecheck": "tsc -p ui/tsconfig.json",
    "test": "vitest run --config ui/vitest.config.ts",
    "test:watch": "vitest --config ui/vitest.config.ts",
    "tauri": "tauri"
  },
  "dependencies": {
    "@fontsource/ibm-plex-sans": "^5.3.0",
    "@fontsource/jetbrains-mono": "^5.3.0",
    "@tauri-apps/api": "^2.12.3",
    "@tauri-apps/plugin-dialog": "^2.8.2",
    "lucide-react": "^1.55.0",
    "react": "^19.3.0",
    "react-dom": "^19.3.0",
    "zustand": "^5.0.15"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.12.2",
    "@testing-library/dom": "^10.4.2",
    "@testing-library/jest-dom": "^7.0.1",
    "@testing-library/react": "^16.3.3",
    "@testing-library/user-event": "^14.6.7",
    "@types/react": "^19.3.0",
    "@types/react-dom": "^19.3.0",
    "@vitejs/plugin-react": "^6.1.2",
    "jsdom": "^29.1.1",
    "typescript": "^7.0.2",
    "vite": "^8.3.4",
    "vitest": "^4.1.11"
  }
}
```

Append to `.gitignore` (create it if missing):

```gitignore
node_modules/
ui/dist/
```

Run: `bun install`
Expected: `bun.lock` created, no errors. Commit `bun.lock` together with this task.

- [ ] **Step 2: Vite, Vitest, TypeScript and HTML entry**

**`ui/index.html`**
```html
<!doctype html>
<html lang="de" class="app">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>LinaSCP</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

**`ui/vite.config.ts`**
```ts
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const root = fileURLToPath(new URL(".", import.meta.url));

// Tauri expects a fixed port; the window loads http://localhost:5173 in dev.
export default defineConfig({
  root,
  plugins: [react()],
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: {
    port: 5173,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
  },
});
```

**`ui/vitest.config.ts`**
```ts
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const root = fileURLToPath(new URL(".", import.meta.url));

export default defineConfig({
  root,
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    clearMocks: true,
  },
});
```

**`ui/tsconfig.json`**
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "noUncheckedIndexedAccess": true,
    "verbatimModuleSyntax": true,
    "isolatedModules": true,
    "skipLibCheck": true,
    "noEmit": true,
    "types": ["vite/client"]
  },
  "include": ["src"]
}
```

**`ui/src/test/setup.ts`**
```ts
import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(() => {
  cleanup();
  document.documentElement.className = "";
});
```

- [ ] **Step 3: Write the failing smoke test**

**`ui/src/App.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "./App";

describe("App", () => {
  it("renders the product name", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "LinaSCP" })).toBeInTheDocument();
  });
});
```

Run: `bun run test App`
Expected: FAIL — `Failed to resolve import "./App"`.

- [ ] **Step 4: Tokens, global styles, fonts, i18n, entry and App**

Copy the design tokens verbatim (the zip is in the repository root):

```bash
mkdir -p ui/src/styles
unzip -p shellport-design.zip shellport-design/tokens/tokens.css > ui/src/styles/tokens.css
head -5 ui/src/styles/tokens.css
```
Expected: the first lines are the `/* Shellport Design-Tokens` comment and `.app {`. Tokens are scoped to `.app` / `.app.light`; `index.html` puts `class="app"` on `<html>`, and the theme switch toggles `light` on `document.documentElement`.

**`ui/src/styles/global.css`**
```css
*,
*::before,
*::after {
  box-sizing: border-box;
}

html,
body,
#root {
  height: 100%;
  margin: 0;
}

body {
  background: var(--bg);
  color: var(--text);
  font-family: var(--font-ui);
  font-size: 14px;
  line-height: 1.45;
  -webkit-font-smoothing: antialiased;
  overflow: hidden;
}

h1,
h2,
h3,
p {
  margin: 0;
}

button,
input,
select,
textarea {
  font: inherit;
  color: inherit;
}

.mono {
  font-family: var(--font-mono);
}

:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

::selection {
  background: var(--accent-soft);
  color: var(--text);
}

.visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
  white-space: nowrap;
  border: 0;
}

@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
```

**`ui/src/i18n/de.ts`**
```ts
/**
 * All German UI strings, grouped by screen. Later tasks add one top-level group per screen
 * (e.g. `hosts`, `editor`, `connect`, `terminal`, `settings`). Never inline UI text in components.
 */
export const t = {
  app: {
    name: "LinaSCP",
  },
  common: {
    cancel: "Abbrechen",
    close: "Schließen",
    save: "Speichern",
    back: "Zurück",
    retry: "Erneut versuchen",
    copy: "Kopieren",
    copied: "Kopiert",
    delete: "Löschen",
    loading: "Wird geladen …",
    comingSoon: "Folgt in einer späteren Version",
  },
  rail: {
    nav: "Hauptnavigation",
    hosts: "Hosts",
    terminal: "Terminal",
    sftp: "SFTP",
    snippets: "Snippets",
    keychain: "Schlüsselbund",
    portForwarding: "Port-Forwarding",
    settings: "Einstellungen",
    toLight: "Helles Design",
    toDark: "Dunkles Design",
  },
  toast: {
    region: "Benachrichtigungen",
    copyDetails: "Details kopieren",
    dismiss: "Meldung schließen",
  },
  steps: {
    pending: "ausstehend",
    active: "läuft",
    done: "erledigt",
    failed: "fehlgeschlagen",
  },
  shell: {
    loadFailed: "Daten konnten nicht geladen werden",
    saveFailed: "Speichern fehlgeschlagen",
  },
  stubs: {
    hosts: "Server-Übersicht",
    terminal: "Terminal",
    sftp: "SFTP",
    sftpText: "Der SFTP-Dateimanager folgt in einer späteren Version.",
    editorHost: "Host bearbeiten",
    editorProfile: "Login-Profil bearbeiten",
    settings: "Einstellungen",
    firstRun: "Willkommen bei LinaSCP",
    export: "Exportieren",
    import: "Importieren",
    connect: "Verbindung",
  },
} as const;
```

**`ui/src/main.tsx`**
```tsx
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "./styles/tokens.css";
import "./styles/global.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";

const container = document.getElementById("root");
if (!container) {
  throw new Error("#root element missing in index.html");
}
createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
```

**`ui/src/App.tsx`**
```tsx
import { t } from "./i18n/de";

/** Replaced by the real shell in Task 18. */
export function App() {
  return (
    <main>
      <h1>{t.app.name}</h1>
    </main>
  );
}
```

Run: `bun run test App`
Expected: PASS (1 test).

- [ ] **Step 5: Write the failing error-helper tests**

**`ui/src/ipc/errors.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import { errorDetails, errorMessage, isCoreError } from "./errors";

describe("isCoreError", () => {
  it("accepts every backend error category", () => {
    expect(isCoreError({ type: "network", kind: "timeout", message: "timed out" })).toBe(true);
    expect(isCoreError({ type: "auth", rejectedMethods: ["password"], message: "denied" })).toBe(true);
    expect(isCoreError({ type: "cancelled" })).toBe(true);
  });

  it("rejects anything else", () => {
    expect(isCoreError({ type: "weird" })).toBe(false);
    expect(isCoreError(new Error("x"))).toBe(false);
    expect(isCoreError(null)).toBe(false);
    expect(isCoreError("network")).toBe(false);
  });
});

describe("errorMessage / errorDetails", () => {
  it("uses the backend message or the category", () => {
    expect(errorMessage({ type: "io", message: "disk full" })).toBe("disk full");
    expect(errorMessage({ type: "cancelled" })).toBe("cancelled");
    expect(errorMessage(new Error("boom"))).toBe("boom");
    expect(errorMessage(42)).toBe("42");
  });

  it("renders copyable details", () => {
    expect(errorDetails({ type: "config", message: "bad" })).toBe('{\n  "type": "config",\n  "message": "bad"\n}');
    expect(errorDetails("plain")).toBe("plain");
  });
});
```

Run: `bun run test errors`
Expected: FAIL — `Failed to resolve import "./errors"`.

- [ ] **Step 6: Implement the error helpers**

**`ui/src/ipc/errors.ts`**
```ts
import type { CoreError } from "./bindings";

const CORE_ERROR_TYPES: ReadonlySet<string> = new Set<CoreError["type"]>([
  "network",
  "hostKeyRejected",
  "auth",
  "keyring",
  "store",
  "config",
  "io",
  "cancelled",
  "internal",
]);

/** Rejections of `ipc/api.ts` calls are `CoreError`s (tagged by `type`). */
export function isCoreError(value: unknown): value is CoreError {
  if (typeof value !== "object" || value === null) return false;
  const type = (value as { type?: unknown }).type;
  return typeof type === "string" && CORE_ERROR_TYPES.has(type);
}

/** Short message for UI text (backend messages are not translated). */
export function errorMessage(error: unknown): string {
  if (isCoreError(error)) return "message" in error ? error.message : error.type;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** Technical details for the "Details kopieren" toast action. */
export function errorDetails(error: unknown): string {
  if (isCoreError(error)) return JSON.stringify(error, null, 2);
  if (error instanceof Error) return error.stack ?? error.message;
  return String(error);
}
```

Run: `bun run test errors`
Expected: PASS (4 tests).

- [ ] **Step 7: Write the failing IPC wrapper tests**

**`ui/src/ipc/api.test.ts`**
```ts
import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel<T> {
    onmessage: (response: T) => void;
    constructor(onmessage?: (response: T) => void) {
      this.onmessage = onmessage ?? (() => undefined);
    }
  }
  return { invoke, Channel };
});

import { createOutputStream, hostDelete, hostsList, importApply, sessionWrite, type Host } from "./api";

const host = { id: "h1", name: "web-01" } as unknown as Host;

describe("command wrappers", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("resolve with the command data", async () => {
    invoke.mockResolvedValue([host]);
    await expect(hostsList()).resolves.toEqual([host]);
    expect(invoke).toHaveBeenCalledWith("hosts_list");
  });

  it("reject with the CoreError", async () => {
    const error = { type: "network", kind: "timeout", message: "timed out" };
    invoke.mockRejectedValue(error);
    await expect(hostsList()).rejects.toEqual(error);
  });

  it("map unit results to undefined", async () => {
    invoke.mockResolvedValue(null);
    await expect(hostDelete("h1")).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith("host_delete", { id: "h1" });
  });

  it("encode terminal input as UTF-8 bytes", async () => {
    invoke.mockResolvedValue(null);
    await sessionWrite("s1", "hé");
    expect(invoke).toHaveBeenCalledWith("session_write", { sessionId: "s1", data: [104, 195, 169] });
  });

  it("pass import resolutions keyed by id", async () => {
    invoke.mockResolvedValue({ imported: 1, skipped: 0, secretsRestored: 0 });
    await importApply("/tmp/x.linascp", null, { h1: "replace" });
    expect(invoke).toHaveBeenCalledWith("import_apply", {
      path: "/tmp/x.linascp",
      passphrase: null,
      resolutions: { h1: "replace" },
    });
  });
});

describe("createOutputStream", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(null);
  });

  it("buffers output until a consumer attaches", () => {
    const stream = createOutputStream();
    stream.channel.onmessage(new Uint8Array([1, 2]).buffer);
    const received: number[][] = [];
    stream.attach((bytes) => received.push(Array.from(bytes)));
    stream.channel.onmessage(new Uint8Array([3]).buffer);
    expect(received).toEqual([[1, 2], [3]]);
  });

  it("buffers again after the consumer detached", () => {
    const stream = createOutputStream();
    const received: number[][] = [];
    const detach = stream.attach((bytes) => received.push(Array.from(bytes)));
    detach();
    stream.channel.onmessage(new Uint8Array([9]).buffer);
    expect(received).toEqual([]);
    stream.attach((bytes) => received.push(Array.from(bytes)));
    expect(received).toEqual([[9]]);
  });

  it("accumulates acks until the session id is known", () => {
    const stream = createOutputStream();
    stream.ack(100);
    stream.ack(28);
    expect(invoke).not.toHaveBeenCalled();
    stream.bind("s1");
    expect(invoke).toHaveBeenCalledWith("session_ack", { sessionId: "s1", bytes: 128 });
    stream.ack(5);
    expect(invoke).toHaveBeenLastCalledWith("session_ack", { sessionId: "s1", bytes: 5 });
  });
});
```

Run: `bun run test api`
Expected: FAIL — `Failed to resolve import "./api"`.

- [ ] **Step 8: (no code) — check the generated bindings are present**

Run: `grep -c "__TAURI_INVOKE(" ui/src/ipc/bindings.ts`
Expected: `26`. (If the file is missing or older, run `cargo test -p linascp exports_bindings`.)

- [ ] **Step 9: Implement the IPC wrappers**

**`ui/src/ipc/api.ts`**
```ts
/**
 * Typed wrappers around the generated tauri-specta bindings.
 * Every function resolves with the data or rejects with a `CoreError` (see `./errors`).
 * Tests mock this module: `vi.mock("<relative>/ipc/api")` and `vi.mocked(api.fn).mockResolvedValue(…)`.
 */
import { Channel } from "@tauri-apps/api/core";
import { commands } from "./bindings";
import type {
  ConnectEvent,
  CoreError,
  Group,
  Host,
  ImportPreview,
  ImportReport,
  LoginProfile,
  ProfileWithUsage,
  PromptAnswer,
  Resolution,
  SecretKind,
  SecretOwner,
  SessionEventDto,
  Settings,
} from "./bindings";

export type {
  AuthConfig,
  AuthMethod,
  Conflict,
  ConnectEvent,
  CoreError,
  EnvVar,
  ForwardKind,
  Group,
  Host,
  HostAuth,
  HostKeyDecision,
  HostKeyInfo,
  HostSource,
  ImportItem,
  ImportPreview,
  ImportReport,
  ItemKind,
  KbdPrompt,
  LoginProfile,
  NetworkKind,
  PortForward,
  ProfileWithUsage,
  Prompt,
  PromptAnswer,
  Resolution,
  SecretKind,
  SecretOwner,
  SessionEventDto,
  Settings,
  SftpSettings,
  SshMode,
  Step,
  TerminalRenderer,
  Theme,
  TransferMode,
} from "./bindings";

/** UUID string (hosts, profiles, groups, connect attempts). */
export type Id = string;
export type SessionId = string;

type CommandResult<T> = { status: "ok"; data: T } | { status: "error"; error: CoreError };

async function unwrap<T>(pending: Promise<CommandResult<T>>): Promise<T> {
  const result = await pending;
  if (result.status === "error") {
    throw result.error;
  }
  return result.data;
}

async function unwrapVoid(pending: Promise<CommandResult<null>>): Promise<void> {
  await unwrap(pending);
}

/** New random UUID for hosts, profiles, groups and connect attempts. */
export function newId(): Id {
  return crypto.randomUUID();
}

// Hosts & groups
export function hostsList(): Promise<Host[]> {
  return unwrap(commands.hostsList());
}
export function hostSave(host: Host): Promise<Host> {
  return unwrap(commands.hostSave(host));
}
export function hostDelete(id: Id): Promise<void> {
  return unwrapVoid(commands.hostDelete(id));
}
export function groupsList(): Promise<Group[]> {
  return unwrap(commands.groupsList());
}
export function groupSave(group: Group): Promise<Group> {
  return unwrap(commands.groupSave(group));
}
export function groupDelete(id: Id): Promise<void> {
  return unwrapVoid(commands.groupDelete(id));
}

// Login profiles & secrets
export function profilesList(): Promise<ProfileWithUsage[]> {
  return unwrap(commands.profilesList());
}
export function profileSave(profile: LoginProfile): Promise<LoginProfile> {
  return unwrap(commands.profileSave(profile));
}
export function profileDelete(id: Id): Promise<void> {
  return unwrapVoid(commands.profileDelete(id));
}
export function secretSet(owner: SecretOwner, kind: SecretKind, value: string): Promise<void> {
  return unwrapVoid(commands.secretSet(owner, kind, value));
}
export function secretDelete(owner: SecretOwner, kind: SecretKind): Promise<void> {
  return unwrapVoid(commands.secretDelete(owner, kind));
}

// Settings
export function settingsGet(): Promise<Settings> {
  return unwrap(commands.settingsGet());
}
export function settingsSet(settings: Settings): Promise<Settings> {
  return unwrap(commands.settingsSet(settings));
}

// Connect pipeline
export function connectSsh(
  connectId: Id,
  hostId: Id,
  cols: number,
  rows: number,
  events: Channel<ConnectEvent>,
  output: Channel<ArrayBuffer>,
  sessionEvents: Channel<SessionEventDto>,
): Promise<SessionId> {
  return unwrap(commands.connectSsh(connectId, hostId, cols, rows, events, output, sessionEvents));
}
/** Resolves `false` when the prompt was already answered, timed out or cancelled. */
export function connectAnswer(promptId: number, answer: PromptAnswer): Promise<boolean> {
  return unwrap(commands.connectAnswer(promptId, answer));
}
export function connectCancel(connectId: Id): Promise<void> {
  return unwrapVoid(commands.connectCancel(connectId));
}
/** "Verbindung testen": runs until Auth for an (unsaved) host; cancel with `connectCancel(connectId)`. */
export function connectTest(connectId: Id, host: Host, events: Channel<ConnectEvent>): Promise<void> {
  return unwrapVoid(commands.connectTest(connectId, host, events));
}

// Sessions
export function sessionOpenLocal(
  cols: number,
  rows: number,
  output: Channel<ArrayBuffer>,
  sessionEvents: Channel<SessionEventDto>,
): Promise<SessionId> {
  return unwrap(commands.sessionOpenLocal(cols, rows, output, sessionEvents));
}

const encoder = new TextEncoder();

/** Accepts xterm `onData` strings (UTF-8 encoded here) or raw bytes. */
export function sessionWrite(sessionId: SessionId, data: string | Uint8Array): Promise<void> {
  const bytes = typeof data === "string" ? encoder.encode(data) : data;
  return unwrapVoid(commands.sessionWrite(sessionId, Array.from(bytes)));
}
export function sessionResize(sessionId: SessionId, cols: number, rows: number): Promise<void> {
  return unwrapVoid(commands.sessionResize(sessionId, cols, rows));
}
/** Flow control: report bytes consumed by xterm (call from the `term.write` callback). */
export function sessionAck(sessionId: SessionId, bytes: number): Promise<void> {
  return unwrapVoid(commands.sessionAck(sessionId, bytes));
}
export function sessionClose(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sessionClose(sessionId));
}

// Export / import / ssh-config
export function exportFile(
  path: string,
  includeSecrets: boolean,
  includeKeyFiles: boolean,
  passphrase: string | null,
): Promise<void> {
  return unwrapVoid(commands.exportFile(path, includeSecrets, includeKeyFiles, passphrase));
}
export function importPreview(path: string): Promise<ImportPreview> {
  return unwrap(commands.importPreview(path));
}
export function importApply(
  path: string,
  passphrase: string | null,
  resolutions: Record<Id, Resolution>,
): Promise<ImportReport> {
  return unwrap(commands.importApply(path, passphrase, resolutions));
}
export function sshConfigReload(): Promise<Host[]> {
  return unwrap(commands.sshConfigReload());
}

// Channels
export function createEventChannel<T>(onEvent: (event: T) => void): Channel<T> {
  return new Channel<T>(onEvent);
}

/** Raw terminal output: the backend sends `InvokeResponseBody::Raw`, JS receives an `ArrayBuffer`. */
export function createOutputChannel(onData: (bytes: Uint8Array) => void): Channel<ArrayBuffer> {
  return new Channel<ArrayBuffer>((buffer) => onData(new Uint8Array(buffer)));
}

/**
 * Output channel that can be created before the terminal exists (connect screen) and before the
 * session id is known: chunks are buffered until `attach`, acks are summed until `bind`.
 */
export interface OutputStream {
  readonly channel: Channel<ArrayBuffer>;
  /** Delivers buffered chunks synchronously, then live ones. Returns a detach function. */
  attach(onData: (bytes: Uint8Array) => void): () => void;
  /** Sets the session id once `connectSsh`/`sessionOpenLocal` resolved; flushes pending acks. */
  bind(sessionId: SessionId): void;
  /** Report consumed bytes (xterm write callback). */
  ack(bytes: number): void;
}

export function createOutputStream(): OutputStream {
  let consumer: ((bytes: Uint8Array) => void) | null = null;
  let pending: Uint8Array[] = [];
  let sessionId: SessionId | null = null;
  let unsentAck = 0;

  const flushAck = () => {
    if (sessionId === null || unsentAck === 0) return;
    const bytes = unsentAck;
    unsentAck = 0;
    sessionAck(sessionId, bytes).catch(() => undefined);
  };

  const channel = createOutputChannel((bytes) => {
    if (consumer) consumer(bytes);
    else pending.push(bytes);
  });

  return {
    channel,
    attach(onData) {
      consumer = onData;
      const buffered = pending;
      pending = [];
      for (const chunk of buffered) onData(chunk);
      return () => {
        if (consumer === onData) consumer = null;
      };
    },
    bind(id) {
      sessionId = id;
      flushAck();
    },
    ack(bytes) {
      unsentAck += bytes;
      flushAck();
    },
  };
}
```

Run: `bun run test api`
Expected: PASS (8 tests).

- [ ] **Step 10: Typecheck, build and full test run**

Run: `bun run typecheck && bun run build && bun run test`
Expected: no type errors; `ui/dist/index.html` produced; all tests PASS (App 1, errors 4, api 8).

Run (manual smoke, needs a desktop session): `bun run tauri dev`
Expected: a 1440×900 window titled "LinaSCP" showing the heading "LinaSCP" on the dark background.

- [ ] **Step 11: Commit**

```bash
git add package.json bun.lock .gitignore ui
git commit -m "feat(ui): scaffold Vite/React UI with design tokens, i18n and typed IPC wrappers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 17: Component kit

**Files:**
- Create: `ui/src/components/cx.ts`, `ui/src/components/tone.ts`, `ui/src/components/roving.ts`, `ui/src/components/field.ts`, `ui/src/components/field.module.css`, `ui/src/components/index.ts`
- Create (one folder per component, `X.tsx` + `X.module.css`): `Button`, `IconButton`, `TextField` (uses `field.module.css`), `Select`, `Checkbox`, `Segmented`, `Tabs`, `Chip`, `Card`, `ProgressBar`, `StepList`, `Dialog`, `Toast`
- Test: `ui/src/components/roving.test.ts`, `Button/Button.test.tsx`, `IconButton/IconButton.test.tsx`, `TextField/TextField.test.tsx`, `Select/Select.test.tsx`, `Checkbox/Checkbox.test.tsx`, `Segmented/Segmented.test.tsx`, `Tabs/Tabs.test.tsx`, `StepList/StepList.test.tsx`, `ProgressBar/ProgressBar.test.tsx`, `Chip/Chip.test.tsx`, `Dialog/Dialog.test.tsx`, `Toast/Toast.test.tsx`

**Interfaces (all exported from `ui/src/components/index.ts`):**
- `cx(...names: Array<string | false | null | undefined>): string`; `type Tone = "accent" | "amber" | "red" | "green" | "neutral"`; `nextIndex(key, current, count, orientation: "horizontal" | "vertical" | "both", isDisabled?) => number | null`.
- `Button { variant?: "primary" | "secondary" | "ghost" | "danger" | "inverted" (default "secondary"); size?: "sm" | "md" | "lg" (34/40/44 px); icon?: LucideIcon; ref?; …button attributes }` — `type` defaults to `"button"`, renders `data-variant`.
- `IconButton { label: string /* aria-label + title */; icon: LucideIcon; size?: "sm" | "md" | "lg" (28/34/44 px); ref?; …button attributes }` (`title` may be overridden).
- `TextField { label; value: string; onChange: (value: string) => void; error?: string | null; hint?: string; mono?: boolean; type?; placeholder?; ref?; …input attributes }` → `aria-invalid` + `aria-describedby` (hint and error ids).
- `Select<T extends string> { label; value: T; options: readonly { value: T; label: string; disabled?: boolean }[]; onChange: (value: T) => void; hint?; error?; …select attributes }`.
- `Checkbox { label: ReactNode; checked: boolean; onChange: (checked: boolean) => void; hint?: string; …input attributes }`.
- `Segmented<T extends string> { label /* aria-label */; value: T; options: readonly { value: T; label: string; disabled?: boolean }[]; onChange: (value: T) => void; size?: "sm" | "md" }` — `role="radiogroup"`, buttons `role="radio"`, arrow keys move + select.
- `Tabs { label; tabs: readonly { id: string; label: string; disabled?: boolean }[]; value: string; onChange: (id: string) => void; orientation?: "horizontal" | "vertical"; idBase?: string }` + `tabId(idBase, id)`, `tabPanelId(idBase, id)` for the consumer's `role="tabpanel"` (`id={tabPanelId(base, id)} aria-labelledby={tabId(base, id)}`).
- `Chip { tone?: Tone; icon?: LucideIcon; mono?: boolean; children }` · `Card { selected?: boolean; padded?: boolean; ref?; …div attributes }` · `ProgressBar { value: number /* 0..1 */; tone?: Tone; label?: string }`.
- `StepList { steps: readonly { id: string; label: string; state: "pending" | "active" | "done" | "failed"; meta?: string; detail?: ReactNode }[] }` — `detail` renders below the step (inline 2FA prompts); active step has `aria-current="step"`.
- `Dialog { title; open; onClose; size?: "sm" | "md" | "lg" (440/640/860 px); children; footer?; subtitle?; icon?; role?: "dialog" | "alertdialog"; initialFocusRef?; hideClose?; className? }` — focus trap, Esc closes, focus restored to the previously focused element, initial focus = `initialFocusRef` or first focusable in the body.
- `ToastProvider` + `useToast(): { show(options: { tone: Tone; message: string; details?: string; durationMs?: number }): number; dismiss(id: number): void }` — default 5 s, `tone: "red"` 10 s; `details` adds "Details kopieren".

- [ ] **Step 1: Write the failing helper tests**

**`ui/src/components/roving.test.ts`**
```ts
import { describe, expect, it } from "vitest";
import { nextIndex } from "./roving";

describe("nextIndex", () => {
  it("moves forward and backward horizontally with wrap-around", () => {
    expect(nextIndex("ArrowRight", 0, 3, "horizontal")).toBe(1);
    expect(nextIndex("ArrowRight", 2, 3, "horizontal")).toBe(0);
    expect(nextIndex("ArrowLeft", 0, 3, "horizontal")).toBe(2);
  });

  it("ignores keys of the other orientation", () => {
    expect(nextIndex("ArrowDown", 0, 3, "horizontal")).toBeNull();
    expect(nextIndex("ArrowRight", 0, 3, "vertical")).toBeNull();
    expect(nextIndex("ArrowDown", 0, 3, "vertical")).toBe(1);
    expect(nextIndex("Enter", 0, 3, "both")).toBeNull();
  });

  it("accepts both axes for 'both'", () => {
    expect(nextIndex("ArrowDown", 0, 3, "both")).toBe(1);
    expect(nextIndex("ArrowLeft", 1, 3, "both")).toBe(0);
  });

  it("skips disabled items and supports Home/End", () => {
    const disabled = (i: number) => i === 1;
    expect(nextIndex("ArrowRight", 0, 3, "horizontal", disabled)).toBe(2);
    expect(nextIndex("Home", 2, 3, "horizontal", (i) => i === 0)).toBe(1);
    expect(nextIndex("End", 0, 3, "horizontal", (i) => i === 2)).toBe(1);
  });
});
```

Run: `bun run test roving`
Expected: FAIL — `Failed to resolve import "./roving"`.

- [ ] **Step 2: Implement the shared helpers**

**`ui/src/components/roving.ts`**
```ts
export type Orientation = "horizontal" | "vertical" | "both";

function firstEnabled(start: number, step: 1 | -1, count: number, isDisabled: (index: number) => boolean) {
  for (let i = start; i >= 0 && i < count; i += step) {
    if (!isDisabled(i)) return i;
  }
  return null;
}

/**
 * Roving-tabindex navigation: index to move to for `key`, skipping disabled items and wrapping around.
 * Returns `null` when `key` is not a navigation key for `orientation`.
 */
export function nextIndex(
  key: string,
  current: number,
  count: number,
  orientation: Orientation,
  isDisabled: (index: number) => boolean = () => false,
): number | null {
  if (key === "Home") return firstEnabled(0, 1, count, isDisabled);
  if (key === "End") return firstEnabled(count - 1, -1, count, isDisabled);
  const horizontal = orientation !== "vertical";
  const vertical = orientation !== "horizontal";
  const forward = (horizontal && key === "ArrowRight") || (vertical && key === "ArrowDown");
  const backward = (horizontal && key === "ArrowLeft") || (vertical && key === "ArrowUp");
  if (!forward && !backward) return null;
  const step = forward ? 1 : -1;
  for (let i = 1; i <= count; i += 1) {
    const candidate = (((current + step * i) % count) + count) % count;
    if (!isDisabled(candidate)) return candidate;
  }
  return null;
}
```

**`ui/src/components/cx.ts`**
```ts
/** Joins truthy class names. */
export function cx(...names: Array<string | false | null | undefined>): string {
  return names.filter(Boolean).join(" ");
}
```

**`ui/src/components/tone.ts`**
```ts
/** Semantic colours: accent = SSH/primary, amber = SFTP/warning, red = error, green = success. */
export type Tone = "accent" | "amber" | "red" | "green" | "neutral";
```

**`ui/src/components/field.ts`**
```ts
/** Ids and `aria-describedby` for a form control with optional hint and error text. */
export function fieldDescription(inputId: string, hint?: string | null, error?: string | null) {
  const hintId = `${inputId}-hint`;
  const errorId = `${inputId}-error`;
  const ids = [hint ? hintId : null, error ? errorId : null].filter((id): id is string => id !== null);
  return { hintId, errorId, describedBy: ids.length > 0 ? ids.join(" ") : undefined };
}
```

**`ui/src/components/field.module.css`**
```css
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.label {
  font-size: 12px;
  color: var(--text3);
}

.control {
  width: 100%;
  height: 40px;
  padding: 0 12px;
  border: 1px solid var(--bd-input);
  border-radius: 8px;
  background: var(--s1);
  color: var(--text);
  font-size: 14px;
  transition: border-color 0.15s;
}

.control::placeholder {
  color: var(--muted2);
}

.control:hover:not(:disabled) {
  border-color: var(--bd-hover);
}

.control:focus-visible {
  border-color: var(--accent);
  outline: 2px solid var(--accent-ring);
  outline-offset: 0;
}

.control:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.invalid,
.invalid:hover:not(:disabled) {
  border-color: var(--red);
}

.mono {
  font-family: var(--font-mono);
  font-size: 13px;
}

.hint {
  font-size: 12px;
  color: var(--muted);
}

.error {
  font-size: 12px;
  color: var(--red);
}
```

Run: `bun run test roving`
Expected: PASS (4 tests).

- [ ] **Step 3: Write the failing Button / IconButton tests**

**`ui/src/components/Button/Button.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Plus } from "lucide-react";
import { describe, expect, it, vi } from "vitest";
import { Button } from "./Button";

describe("Button", () => {
  it.each(["primary", "secondary", "ghost", "danger", "inverted"] as const)("renders the %s variant", (variant) => {
    render(<Button variant={variant}>Speichern</Button>);
    const button = screen.getByRole("button", { name: "Speichern" });
    expect(button).toHaveAttribute("data-variant", variant);
    expect(button).toHaveAttribute("type", "button");
  });

  it("defaults to the secondary variant", () => {
    render(<Button>Abbrechen</Button>);
    expect(screen.getByRole("button", { name: "Abbrechen" })).toHaveAttribute("data-variant", "secondary");
  });

  it("renders a decorative icon and forwards clicks", async () => {
    const onClick = vi.fn();
    render(
      <Button variant="primary" icon={Plus} onClick={onClick}>
        Neuer Host
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Neuer Host" });
    expect(button.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    await userEvent.click(button);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("does not fire when disabled", async () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick}>
        Speichern
      </Button>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Speichern" }));
    expect(onClick).not.toHaveBeenCalled();
  });
});
```

**`ui/src/components/IconButton/IconButton.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Pencil } from "lucide-react";
import { describe, expect, it, vi } from "vitest";
import { IconButton } from "./IconButton";

describe("IconButton", () => {
  it("uses the label as accessible name and tooltip", async () => {
    const onClick = vi.fn();
    render(<IconButton label="Host bearbeiten" icon={Pencil} onClick={onClick} />);
    const button = screen.getByRole("button", { name: "Host bearbeiten" });
    expect(button).toHaveAttribute("title", "Host bearbeiten");
    await userEvent.click(button);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("allows overriding the tooltip", () => {
    render(<IconButton label="Snippets" icon={Pencil} title="Snippets – später" />);
    expect(screen.getByRole("button", { name: "Snippets" })).toHaveAttribute("title", "Snippets – später");
  });
});
```

Run: `bun run test Button`
Expected: FAIL — cannot resolve `./Button` and `./IconButton`.

- [ ] **Step 4: Implement Button and IconButton**

**`ui/src/components/Button/Button.tsx`**
```tsx
import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
import { cx } from "../cx";
import styles from "./Button.module.css";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "inverted";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: LucideIcon;
  children?: ReactNode;
  ref?: Ref<HTMLButtonElement>;
}

const ICON_SIZE: Record<ButtonSize, number> = { sm: 14, md: 16, lg: 18 };

export function Button({
  variant = "secondary",
  size = "md",
  icon: Icon,
  className,
  children,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      {...rest}
      type={type}
      data-variant={variant}
      className={cx(styles.button, styles[variant], styles[size], className)}
    >
      {Icon ? <Icon size={ICON_SIZE[size]} strokeWidth={2} aria-hidden="true" /> : null}
      {children}
    </button>
  );
}
```

**`ui/src/components/Button/Button.module.css`**
```css
.button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border: 1px solid transparent;
  border-radius: 10px;
  font-weight: 500;
  white-space: nowrap;
  cursor: pointer;
  transition:
    background 0.15s,
    border-color 0.15s,
    color 0.15s,
    filter 0.15s;
}

.button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.sm {
  height: 34px;
  padding: 0 12px;
  font-size: 13px;
}

.md {
  height: 40px;
  padding: 0 16px;
  font-size: 14px;
}

.lg {
  height: 44px;
  padding: 0 20px;
  font-size: 14px;
}

.primary {
  background: var(--accent);
  color: var(--on-accent);
  font-weight: 600;
}

.primary:hover:not(:disabled) {
  filter: brightness(1.08);
}

.secondary {
  background: transparent;
  border-color: var(--bd-strong);
  color: var(--text);
}

.secondary:hover:not(:disabled) {
  background: var(--raised);
  border-color: var(--bd-hover);
}

.ghost {
  background: transparent;
  color: var(--text3);
}

.ghost:hover:not(:disabled) {
  background: var(--raised);
  color: var(--text);
}

.danger {
  background: var(--red);
  color: var(--bg);
  font-weight: 600;
}

.danger:hover:not(:disabled) {
  filter: brightness(1.08);
}

.inverted {
  background: var(--text);
  color: var(--bg);
  font-weight: 600;
}

.inverted:hover:not(:disabled) {
  filter: brightness(1.1);
}
```

**`ui/src/components/IconButton/IconButton.tsx`**
```tsx
import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, Ref } from "react";
import { cx } from "../cx";
import styles from "./IconButton.module.css";

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  /** Accessible name and default tooltip. */
  label: string;
  icon: LucideIcon;
  size?: "sm" | "md" | "lg";
  ref?: Ref<HTMLButtonElement>;
}

const ICON_SIZE = { sm: 14, md: 16, lg: 20 } as const;

export function IconButton({ label, icon: Icon, size = "md", className, type = "button", ...rest }: IconButtonProps) {
  return (
    <button
      title={label}
      {...rest}
      type={type}
      aria-label={label}
      className={cx(styles.iconButton, styles[size], className)}
    >
      <Icon size={ICON_SIZE[size]} strokeWidth={2} aria-hidden="true" />
    </button>
  );
}
```

**`ui/src/components/IconButton/IconButton.module.css`**
```css
.iconButton {
  display: inline-grid;
  place-items: center;
  flex: none;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}

.iconButton:hover:not(:disabled) {
  background: var(--raised);
  color: var(--text);
}

.iconButton:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.sm {
  width: 28px;
  height: 28px;
}

.md {
  width: 34px;
  height: 34px;
}

.lg {
  width: 44px;
  height: 44px;
  border-radius: 10px;
}
```

Run: `bun run test Button`
Expected: PASS (Button 8, IconButton 2).

- [ ] **Step 5: Write the failing form-control tests**

**`ui/src/components/TextField/TextField.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TextField } from "./TextField";

describe("TextField", () => {
  it("associates the label with the input", () => {
    render(<TextField label="Adresse" value="" onChange={() => undefined} placeholder="10.0.1.12" />);
    const input = screen.getByLabelText("Adresse");
    expect(input).toHaveAttribute("type", "text");
    expect(input).toHaveAttribute("placeholder", "10.0.1.12");
    expect(input).not.toHaveAttribute("aria-invalid");
  });

  it("reports changes as plain strings", async () => {
    const onChange = vi.fn();
    render(<TextField label="Name" value="" onChange={onChange} />);
    await userEvent.type(screen.getByLabelText("Name"), "w");
    expect(onChange).toHaveBeenCalledWith("w");
  });

  it("renders the error with aria-invalid and aria-describedby", () => {
    render(
      <TextField label="Port" value="0" onChange={() => undefined} hint="Standard: 22" error="Port muss zwischen 1 und 65535 liegen" />,
    );
    const input = screen.getByLabelText("Port");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input.getAttribute("aria-describedby")).toMatch(/-hint .*-error$/);
    expect(input).toHaveAccessibleDescription("Standard: 22 Port muss zwischen 1 und 65535 liegen");
  });
});
```

**`ui/src/components/Select/Select.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Select } from "./Select";

const options = [
  { value: "auto", label: "Automatisch" },
  { value: "webgl", label: "WebGL" },
  { value: "dom", label: "DOM" },
] as const;

describe("Select", () => {
  it("renders a labelled native select and reports the chosen value", async () => {
    const onChange = vi.fn();
    render(<Select label="Terminal-Renderer" value="auto" options={options} onChange={onChange} />);
    const select = screen.getByLabelText("Terminal-Renderer");
    expect(select).toHaveValue("auto");
    await userEvent.selectOptions(select, "dom");
    expect(onChange).toHaveBeenCalledWith("dom");
  });

  it("marks errors", () => {
    render(<Select label="Gruppe" value="auto" options={options} onChange={() => undefined} error="Pflichtfeld" />);
    expect(screen.getByLabelText("Gruppe")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Gruppe")).toHaveAccessibleDescription("Pflichtfeld");
  });
});
```

**`ui/src/components/Checkbox/Checkbox.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Checkbox } from "./Checkbox";

describe("Checkbox", () => {
  it("toggles via its label", async () => {
    const onChange = vi.fn();
    render(<Checkbox label="Geheimnisse einschließen" checked={false} onChange={onChange} />);
    await userEvent.click(screen.getByText("Geheimnisse einschließen"));
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("describes itself with the hint and respects disabled", async () => {
    const onChange = vi.fn();
    render(<Checkbox label="Im Profil speichern" checked onChange={onChange} hint="Im Schlüsselbund" disabled />);
    const box = screen.getByRole("checkbox", { name: "Im Profil speichern" });
    expect(box).toBeChecked();
    expect(box).toHaveAccessibleDescription("Im Schlüsselbund");
    await userEvent.click(box);
    expect(onChange).not.toHaveBeenCalled();
  });
});
```

Run: `bun run test TextField Select Checkbox`
Expected: FAIL — cannot resolve `./TextField`, `./Select`, `./Checkbox`.

- [ ] **Step 6: Implement TextField, Select, Checkbox**

**`ui/src/components/TextField/TextField.tsx`**
```tsx
import { type InputHTMLAttributes, type Ref, useId } from "react";
import { cx } from "../cx";
import { fieldDescription } from "../field";
import field from "../field.module.css";

export interface TextFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "size"> {
  label: string;
  value: string;
  onChange: (value: string) => void;
  error?: string | null;
  hint?: string;
  mono?: boolean;
  ref?: Ref<HTMLInputElement>;
}

export function TextField({
  label,
  value,
  onChange,
  error,
  hint,
  mono = false,
  id,
  className,
  type = "text",
  ...rest
}: TextFieldProps) {
  const generatedId = useId();
  const inputId = id ?? generatedId;
  const { hintId, errorId, describedBy } = fieldDescription(inputId, hint, error);
  return (
    <div className={cx(field.field, className)}>
      <label className={field.label} htmlFor={inputId}>
        {label}
      </label>
      <input
        {...rest}
        id={inputId}
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy}
        className={cx(field.control, mono && field.mono, error && field.invalid)}
      />
      {hint ? (
        <p id={hintId} className={field.hint}>
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className={field.error}>
          {error}
        </p>
      ) : null}
    </div>
  );
}
```

**`ui/src/components/Select/Select.tsx`**
```tsx
import { ChevronDown } from "lucide-react";
import { type SelectHTMLAttributes, useId } from "react";
import { cx } from "../cx";
import { fieldDescription } from "../field";
import field from "../field.module.css";
import styles from "./Select.module.css";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
}

export interface SelectProps<T extends string>
  extends Omit<SelectHTMLAttributes<HTMLSelectElement>, "value" | "onChange"> {
  label: string;
  value: T;
  options: readonly SelectOption<T>[];
  onChange: (value: T) => void;
  hint?: string;
  error?: string | null;
}

export function Select<T extends string>({
  label,
  value,
  options,
  onChange,
  hint,
  error,
  id,
  className,
  ...rest
}: SelectProps<T>) {
  const generatedId = useId();
  const selectId = id ?? generatedId;
  const { hintId, errorId, describedBy } = fieldDescription(selectId, hint, error);
  return (
    <div className={cx(field.field, className)}>
      <label className={field.label} htmlFor={selectId}>
        {label}
      </label>
      <div className={styles.wrap}>
        <select
          {...rest}
          id={selectId}
          value={value}
          onChange={(event) => onChange(event.target.value as T)}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          className={cx(field.control, styles.select, error && field.invalid)}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value} disabled={option.disabled}>
              {option.label}
            </option>
          ))}
        </select>
        <ChevronDown className={styles.chevron} size={16} aria-hidden="true" />
      </div>
      {hint ? (
        <p id={hintId} className={field.hint}>
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className={field.error}>
          {error}
        </p>
      ) : null}
    </div>
  );
}
```

**`ui/src/components/Select/Select.module.css`**
```css
.wrap {
  position: relative;
}

.select {
  appearance: none;
  padding-right: 34px;
  cursor: pointer;
}

.chevron {
  position: absolute;
  top: 50%;
  right: 12px;
  transform: translateY(-50%);
  color: var(--muted);
  pointer-events: none;
}
```

**`ui/src/components/Checkbox/Checkbox.tsx`**
```tsx
import { type InputHTMLAttributes, type ReactNode, useId } from "react";
import { cx } from "../cx";
import styles from "./Checkbox.module.css";

export interface CheckboxProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "checked" | "onChange"> {
  label: ReactNode;
  checked: boolean;
  onChange: (checked: boolean) => void;
  hint?: string;
}

export function Checkbox({ label, checked, onChange, hint, id, className, ...rest }: CheckboxProps) {
  const generatedId = useId();
  const inputId = id ?? generatedId;
  const hintId = `${inputId}-hint`;
  return (
    <div className={cx(styles.wrap, className)}>
      <label className={styles.row} htmlFor={inputId}>
        <input
          {...rest}
          id={inputId}
          type="checkbox"
          checked={checked}
          onChange={(event) => onChange(event.target.checked)}
          aria-describedby={hint ? hintId : undefined}
          className={styles.box}
        />
        <span className={styles.label}>{label}</span>
      </label>
      {hint ? (
        <p id={hintId} className={styles.hint}>
          {hint}
        </p>
      ) : null}
    </div>
  );
}
```

**`ui/src/components/Checkbox/Checkbox.module.css`**
```css
.wrap {
  display: flex;
  flex-direction: column;
}

.row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 32px;
  color: var(--text2);
  font-size: 14px;
  cursor: pointer;
}

.row:has(.box:disabled) {
  opacity: 0.55;
  cursor: not-allowed;
}

.box {
  flex: none;
  width: 16px;
  height: 16px;
  margin: 0;
  accent-color: var(--accent);
  cursor: inherit;
}

.label {
  min-width: 0;
}

.hint {
  margin-left: 26px;
  font-size: 12px;
  color: var(--muted);
}
```

Run: `bun run test TextField Select Checkbox`
Expected: PASS (TextField 3, Select 2, Checkbox 2).

- [ ] **Step 7: Write the failing Segmented / Tabs tests**

**`ui/src/components/Segmented/Segmented.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Segmented } from "./Segmented";

type Proto = "all" | "ssh" | "sftp";
const options = [
  { value: "all", label: "Alle" },
  { value: "ssh", label: "SSH" },
  { value: "sftp", label: "SFTP", disabled: true },
] as const;

function Harness({ onChange }: { onChange: (v: Proto) => void }) {
  const [value, setValue] = useState<Proto>("all");
  return (
    <Segmented
      label="Protokoll filtern"
      value={value}
      options={options}
      onChange={(v) => {
        setValue(v);
        onChange(v);
      }}
    />
  );
}

describe("Segmented", () => {
  it("is a radiogroup with one checked radio in the tab order", () => {
    render(<Harness onChange={() => undefined} />);
    expect(screen.getByRole("radiogroup", { name: "Protokoll filtern" })).toBeInTheDocument();
    const all = screen.getByRole("radio", { name: "Alle" });
    expect(all).toHaveAttribute("aria-checked", "true");
    expect(all).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("radio", { name: "SSH" })).toHaveAttribute("tabindex", "-1");
  });

  it("selects on click", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    await userEvent.click(screen.getByRole("radio", { name: "SSH" }));
    expect(onChange).toHaveBeenCalledWith("ssh");
    expect(screen.getByRole("radio", { name: "SSH" })).toHaveAttribute("aria-checked", "true");
  });

  it("moves selection and focus with arrow keys, skipping disabled options", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    screen.getByRole("radio", { name: "Alle" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenLastCalledWith("ssh");
    expect(screen.getByRole("radio", { name: "SSH" })).toHaveFocus();
    await userEvent.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenLastCalledWith("all");
    expect(screen.getByRole("radio", { name: "Alle" })).toHaveFocus();
  });
});
```

**`ui/src/components/Tabs/Tabs.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { Tabs, tabPanelId } from "./Tabs";

const tabs = [
  { id: "general", label: "Allgemein" },
  { id: "auth", label: "Authentifizierung" },
  { id: "connection", label: "Verbindung" },
  { id: "sftp", label: "SFTP" },
];

function Harness({ orientation }: { orientation?: "horizontal" | "vertical" }) {
  const [value, setValue] = useState("general");
  return (
    <>
      <Tabs label="Host-Einstellungen" tabs={tabs} value={value} onChange={setValue} orientation={orientation} idBase="ed" />
      <div role="tabpanel" id={tabPanelId("ed", value)}>
        {value}
      </div>
    </>
  );
}

describe("Tabs", () => {
  it("renders a tablist with the selected tab wired to its panel", () => {
    render(<Harness />);
    expect(screen.getByRole("tablist", { name: "Host-Einstellungen" })).toHaveAttribute("aria-orientation", "horizontal");
    const general = screen.getByRole("tab", { name: "Allgemein" });
    expect(general).toHaveAttribute("aria-selected", "true");
    expect(general).toHaveAttribute("aria-controls", "ed-panel-general");
    expect(general).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("tab", { name: "SFTP" })).toHaveAttribute("tabindex", "-1");
  });

  it("supports ArrowRight/ArrowLeft with wrap-around and Home/End", async () => {
    render(<Harness />);
    screen.getByRole("tab", { name: "Allgemein" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Authentifizierung" })).toHaveFocus();
    expect(screen.getByRole("tab", { name: "Authentifizierung" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(screen.getByRole("tab", { name: "SFTP" })).toHaveFocus();
    await userEvent.keyboard("{Home}");
    expect(screen.getByRole("tab", { name: "Allgemein" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{End}");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("sftp");
  });

  it("uses ArrowDown/ArrowUp when vertical", async () => {
    render(<Harness orientation="vertical" />);
    screen.getByRole("tab", { name: "Allgemein" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Allgemein" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{ArrowDown}");
    expect(screen.getByRole("tab", { name: "Authentifizierung" })).toHaveFocus();
  });
});
```

Run: `bun run test Segmented Tabs`
Expected: FAIL — cannot resolve `./Segmented`, `./Tabs`.

- [ ] **Step 8: Implement Segmented and Tabs**

**`ui/src/components/Segmented/Segmented.tsx`**
```tsx
import { type KeyboardEvent, useRef } from "react";
import { cx } from "../cx";
import { nextIndex } from "../roving";
import styles from "./Segmented.module.css";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
}

export interface SegmentedProps<T extends string> {
  /** Accessible name of the radiogroup. */
  label: string;
  value: T;
  options: readonly SegmentedOption<T>[];
  onChange: (value: T) => void;
  size?: "sm" | "md";
  className?: string;
}

export function Segmented<T extends string>({ label, value, options, onChange, size = "md", className }: SegmentedProps<T>) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = options.findIndex((option) => option.value === value);
  const tabStop = selectedIndex >= 0 ? selectedIndex : options.findIndex((option) => !option.disabled);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, options.length, "both", (i) => Boolean(options[i]?.disabled));
    const option = next === null ? undefined : options[next];
    if (next === null || !option) return;
    event.preventDefault();
    onChange(option.value);
    refs.current[next]?.focus();
  };

  return (
    <div role="radiogroup" aria-label={label} className={cx(styles.group, styles[size], className)}>
      {options.map((option, index) => {
        const checked = option.value === value;
        return (
          <button
            key={option.value}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            tabIndex={index === tabStop ? 0 : -1}
            disabled={option.disabled}
            className={cx(styles.option, checked && styles.checked)}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => onKeyDown(event, index)}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
```

**`ui/src/components/Segmented/Segmented.module.css`**
```css
.group {
  display: inline-flex;
  gap: 2px;
  padding: 3px;
  border: 1px solid var(--bd-strong);
  border-radius: 10px;
  background: var(--card);
}

.option {
  padding: 0 12px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text3);
  font-weight: 500;
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}

.md .option {
  height: 32px;
  font-size: 13px;
}

.sm .option {
  height: 26px;
  padding: 0 10px;
  font-size: 12px;
}

.option:hover:not(:disabled) {
  color: var(--text);
}

.option:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.checked,
.checked:hover:not(:disabled) {
  background: var(--bd-strong);
  color: var(--text);
}
```

**`ui/src/components/Tabs/Tabs.tsx`**
```tsx
import { type KeyboardEvent, useId, useRef } from "react";
import { cx } from "../cx";
import { nextIndex } from "../roving";
import styles from "./Tabs.module.css";

export interface TabItem {
  id: string;
  label: string;
  disabled?: boolean;
}

export interface TabsProps {
  /** Accessible name of the tablist. */
  label: string;
  tabs: readonly TabItem[];
  value: string;
  onChange: (id: string) => void;
  orientation?: "horizontal" | "vertical";
  /** Prefix for tab/panel ids; pass it when rendering the matching `role="tabpanel"`. */
  idBase?: string;
  className?: string;
}

export function tabId(idBase: string, id: string): string {
  return `${idBase}-tab-${id}`;
}

export function tabPanelId(idBase: string, id: string): string {
  return `${idBase}-panel-${id}`;
}

export function Tabs({ label, tabs, value, onChange, orientation = "horizontal", idBase, className }: TabsProps) {
  const generatedId = useId();
  const base = idBase ?? generatedId;
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = tabs.findIndex((tab) => tab.id === value);
  const tabStop = selectedIndex >= 0 ? selectedIndex : tabs.findIndex((tab) => !tab.disabled);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, tabs.length, orientation, (i) => Boolean(tabs[i]?.disabled));
    const tab = next === null ? undefined : tabs[next];
    if (next === null || !tab) return;
    event.preventDefault();
    onChange(tab.id);
    refs.current[next]?.focus();
  };

  return (
    <div
      role="tablist"
      aria-label={label}
      aria-orientation={orientation}
      className={cx(styles.list, styles[orientation], className)}
    >
      {tabs.map((tab, index) => {
        const selected = tab.id === value;
        return (
          <button
            key={tab.id}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="tab"
            id={tabId(base, tab.id)}
            aria-selected={selected}
            aria-controls={tabPanelId(base, tab.id)}
            tabIndex={index === tabStop ? 0 : -1}
            disabled={tab.disabled}
            className={cx(styles.tab, selected && styles.selected)}
            onClick={() => onChange(tab.id)}
            onKeyDown={(event) => onKeyDown(event, index)}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}
```

**`ui/src/components/Tabs/Tabs.module.css`**
```css
.list {
  display: flex;
}

.horizontal {
  gap: 4px;
  border-bottom: 1px solid var(--bd);
}

.vertical {
  flex-direction: column;
  gap: 2px;
}

.tab {
  border: 0;
  background: transparent;
  color: var(--text3);
  font-weight: 500;
  font-size: 14px;
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}

.horizontal .tab {
  height: 40px;
  padding: 0 14px;
  box-shadow: inset 0 -2px 0 transparent;
}

.vertical .tab {
  height: 38px;
  padding: 0 14px;
  border-radius: 8px;
  text-align: left;
}

.tab:hover:not(:disabled) {
  color: var(--text);
}

.tab:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.horizontal .selected {
  color: var(--text);
  box-shadow: inset 0 -2px 0 var(--accent);
}

.vertical .selected {
  background: var(--raised);
  color: var(--text);
  box-shadow: inset 2px 0 0 var(--accent);
}
```

Run: `bun run test Segmented Tabs`
Expected: PASS (Segmented 3, Tabs 3).

- [ ] **Step 9: Write the failing display-component tests**

**`ui/src/components/Chip/Chip.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Card } from "../Card/Card";
import { Chip } from "./Chip";

describe("Chip and Card", () => {
  it("renders chips with their tone", () => {
    render(
      <Chip tone="amber" mono>
        SFTP
      </Chip>,
    );
    expect(screen.getByText("SFTP")).toHaveAttribute("data-tone", "amber");
  });

  it("defaults chips to neutral and marks selected cards", () => {
    render(
      <>
        <Chip>prod</Chip>
        <Card selected data-testid="card">
          web-prod-01
        </Card>
      </>,
    );
    expect(screen.getByText("prod")).toHaveAttribute("data-tone", "neutral");
    expect(screen.getByTestId("card")).toHaveAttribute("data-selected", "true");
  });
});
```

**`ui/src/components/ProgressBar/ProgressBar.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProgressBar } from "./ProgressBar";

describe("ProgressBar", () => {
  it("exposes the value in percent", () => {
    render(<ProgressBar value={0.4} label="Verbindungsaufbau" />);
    const bar = screen.getByRole("progressbar", { name: "Verbindungsaufbau" });
    expect(bar).toHaveAttribute("aria-valuenow", "40");
    expect(bar).toHaveAttribute("aria-valuemin", "0");
    expect(bar).toHaveAttribute("aria-valuemax", "100");
  });

  it("clamps out-of-range values", () => {
    render(<ProgressBar value={3} label="A" />);
    expect(screen.getByRole("progressbar", { name: "A" })).toHaveAttribute("aria-valuenow", "100");
  });
});
```

**`ui/src/components/StepList/StepList.test.tsx`**
```tsx
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { StepList } from "./StepList";

describe("StepList", () => {
  it("renders each state with an accessible status", () => {
    render(
      <StepList
        steps={[
          { id: "tcp", label: "Verbinden", state: "done", meta: "10.0.1.12:22" },
          { id: "hostKey", label: "Host-Key prüfen", state: "failed" },
          { id: "auth", label: "Anmelden", state: "active", detail: <input aria-label="Code" /> },
          { id: "shell", label: "Shell starten", state: "pending" },
        ]}
      />,
    );
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(4);
    expect(within(items[0]!).getByText(/erledigt/)).toBeInTheDocument();
    expect(within(items[0]!).getByText("10.0.1.12:22")).toBeInTheDocument();
    expect(within(items[1]!).getByText(/fehlgeschlagen/)).toBeInTheDocument();
    expect(items[2]).toHaveAttribute("aria-current", "step");
    expect(within(items[2]!).getByRole("textbox", { name: "Code" })).toBeInTheDocument();
    expect(within(items[3]!).getByText(/ausstehend/)).toBeInTheDocument();
    expect(items[3]).not.toHaveAttribute("aria-current");
  });
});
```

Run: `bun run test Chip ProgressBar StepList`
Expected: FAIL — cannot resolve `./Chip`, `../Card/Card`, `./ProgressBar`, `./StepList`.

- [ ] **Step 10: Implement Chip, Card, ProgressBar, StepList**

**`ui/src/components/Chip/Chip.tsx`**
```tsx
import type { LucideIcon } from "lucide-react";
import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "../cx";
import type { Tone } from "../tone";
import styles from "./Chip.module.css";

export interface ChipProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: Tone;
  icon?: LucideIcon;
  mono?: boolean;
  children: ReactNode;
}

export function Chip({ tone = "neutral", icon: Icon, mono = false, className, children, ...rest }: ChipProps) {
  return (
    <span {...rest} data-tone={tone} className={cx(styles.chip, styles[tone], mono && styles.mono, className)}>
      {Icon ? <Icon size={12} strokeWidth={2.2} aria-hidden="true" /> : null}
      {children}
    </span>
  );
}
```

**`ui/src/components/Chip/Chip.module.css`**
```css
.chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 22px;
  padding: 0 8px;
  border: 1px solid transparent;
  border-radius: 6px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
  white-space: nowrap;
}

.mono {
  font-family: var(--font-mono);
}

.accent {
  background: var(--accent-soft);
  border-color: var(--accent-bd);
  color: var(--accent);
}

.amber {
  background: var(--amber-soft);
  border-color: var(--amber-bd);
  color: var(--amber);
}

.red {
  background: var(--red-soft);
  border-color: var(--red-bd);
  color: var(--red);
}

.green {
  background: color-mix(in srgb, var(--green) 14%, transparent);
  border-color: color-mix(in srgb, var(--green) 35%, transparent);
  color: var(--green);
}

.neutral {
  background: var(--raised);
  color: var(--text3);
}
```

**`ui/src/components/Card/Card.tsx`**
```tsx
import type { HTMLAttributes, Ref } from "react";
import { cx } from "../cx";
import styles from "./Card.module.css";

export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  selected?: boolean;
  padded?: boolean;
  ref?: Ref<HTMLDivElement>;
}

export function Card({ selected = false, padded = false, className, ...rest }: CardProps) {
  return (
    <div
      {...rest}
      data-selected={selected ? "true" : undefined}
      className={cx(styles.card, selected && styles.selected, padded && styles.padded, className)}
    />
  );
}
```

**`ui/src/components/Card/Card.module.css`**
```css
.card {
  border: 1px solid var(--bd-card);
  border-radius: 12px;
  background: var(--card);
  transition: border-color 0.15s;
}

.selected {
  border-color: var(--accent);
}

.padded {
  padding: 16px;
}
```

**`ui/src/components/ProgressBar/ProgressBar.tsx`**
```tsx
import { cx } from "../cx";
import type { Tone } from "../tone";
import styles from "./ProgressBar.module.css";

export interface ProgressBarProps {
  /** 0..1 */
  value: number;
  tone?: Tone;
  label?: string;
  className?: string;
}

export function ProgressBar({ value, tone = "accent", label, className }: ProgressBarProps) {
  const ratio = Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
  const percent = Math.round(ratio * 100);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      className={cx(styles.track, className)}
    >
      <div className={cx(styles.fill, styles[tone])} style={{ width: `${percent}%` }} />
    </div>
  );
}
```

**`ui/src/components/ProgressBar/ProgressBar.module.css`**
```css
.track {
  height: 4px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--raised);
}

.fill {
  height: 100%;
  border-radius: inherit;
  transition: width 0.4s ease;
}

.accent {
  background: var(--accent);
}

.amber {
  background: var(--amber);
}

.red {
  background: var(--red);
}

.green {
  background: var(--green);
}

.neutral {
  background: var(--muted);
}
```

**`ui/src/components/StepList/StepList.tsx`**
```tsx
import { Check, LoaderCircle, X } from "lucide-react";
import type { ReactNode } from "react";
import { t } from "../../i18n/de";
import { cx } from "../cx";
import styles from "./StepList.module.css";

export type StepState = "pending" | "active" | "done" | "failed";

export interface StepItem {
  id: string;
  label: string;
  state: StepState;
  /** Mono text on the right, e.g. "admin@bastion:2222". */
  meta?: string;
  /** Rendered below the step, e.g. an inline keyboard-interactive prompt. */
  detail?: ReactNode;
}

export interface StepListProps {
  steps: readonly StepItem[];
  className?: string;
}

function StepIcon({ state }: { state: StepState }) {
  switch (state) {
    case "done":
      return <Check size={16} strokeWidth={2.6} />;
    case "active":
      return <LoaderCircle size={16} strokeWidth={2.4} className={styles.spin} />;
    case "failed":
      return <X size={16} strokeWidth={2.6} />;
    case "pending":
      return <span className={styles.dot} />;
  }
}

export function StepList({ steps, className }: StepListProps) {
  return (
    <ol className={cx(styles.list, className)}>
      {steps.map((step) => (
        <li
          key={step.id}
          data-state={step.state}
          aria-current={step.state === "active" ? "step" : undefined}
          className={cx(styles.step, styles[step.state])}
        >
          <span className={styles.icon} aria-hidden="true">
            <StepIcon state={step.state} />
          </span>
          <span className={styles.label}>
            {step.label}
            <span className="visually-hidden">{` – ${t.steps[step.state]}`}</span>
          </span>
          {step.meta ? <span className={styles.meta}>{step.meta}</span> : null}
          {step.detail ? <div className={styles.detail}>{step.detail}</div> : null}
        </li>
      ))}
    </ol>
  );
}
```

**`ui/src/components/StepList/StepList.module.css`**
```css
.list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.step {
  display: grid;
  grid-template-columns: 24px 1fr auto;
  align-items: center;
  column-gap: 12px;
  padding: 10px 12px;
  border-radius: 10px;
}

.icon {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
}

.label {
  color: var(--text);
  font-size: 14px;
}

.meta {
  color: var(--muted);
  font-family: var(--font-mono);
  font-size: 12px;
}

.detail {
  grid-column: 2 / -1;
  margin-top: 10px;
}

.done .icon {
  color: var(--green);
}

.active {
  background: var(--accent-soft);
}

.active .icon {
  color: var(--accent);
}

.failed .icon,
.failed .label {
  color: var(--red);
}

.pending .label {
  color: var(--muted2);
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--dim);
}

.spin {
  animation: spin 0.9s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
```

Run: `bun run test Chip ProgressBar StepList`
Expected: PASS (Chip 2, ProgressBar 2, StepList 1).

- [ ] **Step 11: Write the failing Dialog tests**

**`ui/src/components/Dialog/Dialog.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { Button } from "../Button/Button";
import { TextField } from "../TextField/TextField";
import { Dialog } from "./Dialog";

function Harness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Öffnen
      </button>
      <Dialog
        title="Einstellungen"
        open={open}
        onClose={() => setOpen(false)}
        footer={<Button onClick={() => setOpen(false)}>OK</Button>}
      >
        <TextField label="Name" value="" onChange={() => undefined} />
      </Dialog>
    </>
  );
}

async function openDialog() {
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(screen.getByRole("button", { name: "Öffnen" }));
  return user;
}

describe("Dialog", () => {
  it("is a labelled modal dialog that focuses the first field", async () => {
    await openDialog();
    const dialog = screen.getByRole("dialog", { name: "Einstellungen" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByLabelText("Name")).toHaveFocus();
  });

  it("traps Tab and Shift+Tab inside the dialog", async () => {
    const user = await openDialog();
    const ok = screen.getByRole("button", { name: "OK" });
    const close = screen.getByRole("button", { name: "Schließen" });
    await user.tab();
    expect(ok).toHaveFocus();
    await user.tab();
    expect(close).toHaveFocus();
    await user.tab({ shift: true });
    expect(ok).toHaveFocus();
  });

  it("closes on Escape and restores focus to the trigger", async () => {
    const user = await openDialog();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Öffnen" })).toHaveFocus();
  });

  it("closes via the close button", async () => {
    const user = await openDialog();
    await user.click(screen.getByRole("button", { name: "Schließen" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("renders nothing when closed", () => {
    render(
      <Dialog title="X" open={false} onClose={() => undefined}>
        body
      </Dialog>,
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
```

Run: `bun run test Dialog`
Expected: FAIL — cannot resolve `./Dialog`.

- [ ] **Step 12: Implement Dialog**

**`ui/src/components/Dialog/Dialog.tsx`**
```tsx
import { X } from "lucide-react";
import { type KeyboardEvent, type ReactNode, type RefObject, useEffect, useId, useRef } from "react";
import { t } from "../../i18n/de";
import { cx } from "../cx";
import { IconButton } from "../IconButton/IconButton";
import styles from "./Dialog.module.css";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusables(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE));
}

export interface DialogProps {
  title: string;
  open: boolean;
  onClose: () => void;
  size?: "sm" | "md" | "lg";
  children: ReactNode;
  footer?: ReactNode;
  subtitle?: ReactNode;
  /** Leading visual in the header, e.g. an initials chip. */
  icon?: ReactNode;
  /** Use "alertdialog" for destructive/blocking confirmations. */
  role?: "dialog" | "alertdialog";
  initialFocusRef?: RefObject<HTMLElement | null>;
  hideClose?: boolean;
  className?: string;
}

export function Dialog({
  title,
  open,
  onClose,
  size = "md",
  children,
  footer,
  subtitle,
  icon,
  role = "dialog",
  initialFocusRef,
  hideClose = false,
  className,
}: DialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    if (!open) return undefined;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const panel = panelRef.current;
    const body = bodyRef.current;
    const target =
      initialFocusRef?.current ?? (body ? focusables(body)[0] : undefined) ?? (panel ? focusables(panel)[0] : undefined) ?? panel;
    target?.focus();
    return () => previous?.focus();
  }, [open, initialFocusRef]);

  if (!open) return null;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const panel = panelRef.current;
    if (!panel) return;
    const items = focusables(panel);
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) {
      event.preventDefault();
      panel.focus();
      return;
    }
    const active = document.activeElement;
    if (event.shiftKey && (active === first || active === panel)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div className={styles.backdrop}>
      <div
        ref={panelRef}
        role={role}
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={cx(styles.panel, styles[size], className)}
        onKeyDown={onKeyDown}
      >
        <header className={styles.header}>
          {icon}
          <div className={styles.titles}>
            <h2 id={titleId} className={styles.title}>
              {title}
            </h2>
            {subtitle ? <p className={styles.subtitle}>{subtitle}</p> : null}
          </div>
          {hideClose ? null : <IconButton label={t.common.close} icon={X} size="sm" onClick={onClose} />}
        </header>
        <div ref={bodyRef} className={styles.body}>
          {children}
        </div>
        {footer ? <footer className={styles.footer}>{footer}</footer> : null}
      </div>
    </div>
  );
}
```

**`ui/src/components/Dialog/Dialog.module.css`**
```css
.backdrop {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: grid;
  place-items: center;
  padding: 24px;
  background: color-mix(in srgb, var(--bg-deep) 72%, transparent);
  backdrop-filter: blur(2px);
}

.panel {
  display: flex;
  flex-direction: column;
  width: 100%;
  max-height: calc(100vh - 48px);
  overflow: hidden;
  border: 1px solid var(--bd-strong);
  border-radius: 16px;
  background: var(--dlg);
  box-shadow: 0 24px 64px rgb(0 0 0 / 0.45);
  outline: none;
}

.sm {
  max-width: 440px;
}

.md {
  max-width: 640px;
}

.lg {
  max-width: 860px;
}

.header {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 18px 22px;
  border-bottom: 1px solid var(--bd);
}

.titles {
  flex: 1;
  min-width: 0;
}

.title {
  color: var(--text);
  font-size: 17px;
  font-weight: 600;
}

.subtitle {
  margin-top: 2px;
  color: var(--muted);
  font-size: 13px;
}

.body {
  flex: 1;
  overflow: auto;
  padding: 22px;
}

.footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding: 14px 22px;
  border-top: 1px solid var(--bd);
  background: var(--s1b);
}
```

Run: `bun run test Dialog`
Expected: PASS (5 tests).

- [ ] **Step 13: Write the failing Toast tests**

**`ui/src/components/Toast/Toast.test.tsx`**
```tsx
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type ToastOptions, ToastProvider, useToast } from "./Toast";

function Trigger({ options }: { options: ToastOptions }) {
  const toast = useToast();
  return (
    <button type="button" onClick={() => toast.show(options)}>
      Zeigen
    </button>
  );
}

function renderWith(options: ToastOptions) {
  return render(
    <ToastProvider>
      <Trigger options={options} />
    </ToastProvider>,
  );
}

describe("Toast", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows a status message and dismisses it", async () => {
    const user = userEvent.setup();
    renderWith({ tone: "green", message: "Gespeichert" });
    await user.click(screen.getByRole("button", { name: "Zeigen" }));
    expect(screen.getByRole("status")).toHaveTextContent("Gespeichert");
    await user.click(screen.getByRole("button", { name: "Meldung schließen" }));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("dismisses itself after the duration", () => {
    vi.useFakeTimers();
    renderWith({ tone: "accent", message: "Kopiert", durationMs: 1000 });
    fireEvent.click(screen.getByRole("button", { name: "Zeigen" }));
    expect(screen.getByRole("status")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("shows errors as alerts with copyable details", async () => {
    const user = userEvent.setup();
    renderWith({ tone: "red", message: "Speichern fehlgeschlagen", details: '{"type":"io"}' });
    await user.click(screen.getByRole("button", { name: "Zeigen" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
    await user.click(screen.getByRole("button", { name: "Details kopieren" }));
    await expect(navigator.clipboard.readText()).resolves.toBe('{"type":"io"}');
    expect(await screen.findByRole("button", { name: "Kopiert" })).toBeInTheDocument();
  });

  it("requires the provider", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    expect(() => render(<Trigger options={{ tone: "accent", message: "x" }} />)).toThrow(/ToastProvider/);
  });
});
```

Run: `bun run test Toast`
Expected: FAIL — cannot resolve `./Toast`.

- [ ] **Step 14: Implement Toast and the barrel export**

**`ui/src/components/Toast/Toast.tsx`**
```tsx
import { X } from "lucide-react";
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { t } from "../../i18n/de";
import { Button } from "../Button/Button";
import { cx } from "../cx";
import { IconButton } from "../IconButton/IconButton";
import type { Tone } from "../tone";
import styles from "./Toast.module.css";

export interface ToastOptions {
  tone: Tone;
  message: string;
  /** Technical details; adds a "Details kopieren" action. */
  details?: string;
  durationMs?: number;
}

export interface ToastApi {
  show(options: ToastOptions): number;
  dismiss(id: number): void;
}

interface ToastEntry extends ToastOptions {
  id: number;
}

const DEFAULT_DURATION_MS = 5000;
const ERROR_DURATION_MS = 10000;

const ToastContext = createContext<ToastApi | null>(null);

function ToastItem({ entry, onDismiss }: { entry: ToastEntry; onDismiss: () => void }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    if (!entry.details) return;
    navigator.clipboard?.writeText(entry.details).then(
      () => setCopied(true),
      () => setCopied(false),
    );
  };
  return (
    <div role={entry.tone === "red" ? "alert" : "status"} className={cx(styles.toast, styles[entry.tone])}>
      <p className={styles.message}>{entry.message}</p>
      {entry.details ? (
        <Button size="sm" variant="ghost" onClick={copy}>
          {copied ? t.common.copied : t.toast.copyDetails}
        </Button>
      ) : null}
      <IconButton label={t.toast.dismiss} icon={X} size="sm" onClick={onDismiss} />
    </div>
  );
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [entries, setEntries] = useState<ToastEntry[]>([]);
  const nextId = useRef(1);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  const dismiss = useCallback((id: number) => {
    setEntries((current) => current.filter((entry) => entry.id !== id));
    const timer = timers.current.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      timers.current.delete(id);
    }
  }, []);

  const show = useCallback(
    (options: ToastOptions) => {
      const id = nextId.current;
      nextId.current += 1;
      setEntries((current) => [...current, { ...options, id }]);
      const duration = options.durationMs ?? (options.tone === "red" ? ERROR_DURATION_MS : DEFAULT_DURATION_MS);
      timers.current.set(
        id,
        setTimeout(() => dismiss(id), duration),
      );
      return id;
    },
    [dismiss],
  );

  useEffect(() => {
    const pending = timers.current;
    return () => {
      pending.forEach((timer) => clearTimeout(timer));
      pending.clear();
    };
  }, []);

  const api = useMemo<ToastApi>(() => ({ show, dismiss }), [show, dismiss]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className={styles.region} role="region" aria-label={t.toast.region}>
        {entries.map((entry) => (
          <ToastItem key={entry.id} entry={entry} onDismiss={() => dismiss(entry.id)} />
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastApi {
  const api = useContext(ToastContext);
  if (!api) {
    throw new Error("useToast must be used inside <ToastProvider>");
  }
  return api;
}
```

**`ui/src/components/Toast/Toast.module.css`**
```css
.region {
  position: fixed;
  right: 20px;
  bottom: 20px;
  z-index: 200;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 8px;
  pointer-events: none;
}

.toast {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 280px;
  max-width: 440px;
  padding: 10px 10px 10px 14px;
  border: 1px solid var(--bd-strong);
  border-left: 3px solid var(--muted);
  border-radius: 12px;
  background: var(--dlg);
  box-shadow: 0 12px 32px rgb(0 0 0 / 0.35);
  pointer-events: auto;
}

.message {
  flex: 1;
  min-width: 0;
  color: var(--text);
  font-size: 13px;
}

.accent {
  border-left-color: var(--accent);
}

.amber {
  border-left-color: var(--amber);
}

.red {
  border-left-color: var(--red);
}

.green {
  border-left-color: var(--green);
}

.neutral {
  border-left-color: var(--muted);
}
```

**`ui/src/components/index.ts`**
```ts
export { Button, type ButtonProps, type ButtonSize, type ButtonVariant } from "./Button/Button";
export { Card, type CardProps } from "./Card/Card";
export { Checkbox, type CheckboxProps } from "./Checkbox/Checkbox";
export { Chip, type ChipProps } from "./Chip/Chip";
export { cx } from "./cx";
export { Dialog, type DialogProps } from "./Dialog/Dialog";
export { IconButton, type IconButtonProps } from "./IconButton/IconButton";
export { ProgressBar, type ProgressBarProps } from "./ProgressBar/ProgressBar";
export { nextIndex, type Orientation } from "./roving";
export { Segmented, type SegmentedOption, type SegmentedProps } from "./Segmented/Segmented";
export { Select, type SelectOption, type SelectProps } from "./Select/Select";
export { StepList, type StepItem, type StepListProps, type StepState } from "./StepList/StepList";
export { type TabItem, Tabs, type TabsProps, tabId, tabPanelId } from "./Tabs/Tabs";
export { TextField, type TextFieldProps } from "./TextField/TextField";
export { type ToastApi, type ToastOptions, ToastProvider, useToast } from "./Toast/Toast";
export type { Tone } from "./tone";
```

Run: `bun run test Toast && bun run typecheck && bun run test`
Expected: Toast 4 PASS; no type errors; full suite PASS (Task 16 + Task 17: 13 + 41 tests).

- [ ] **Step 15: Commit**

```bash
git add ui/src/components
git commit -m "feat(ui): add accessible component kit (buttons, fields, tabs, dialog, toast, steps)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 18: Stores, app shell, rail, modal layer, global shortcuts

**Files:**
- Create: `ui/src/stores/ui.ts`, `ui/src/stores/settings.ts`, `ui/src/stores/hosts.ts`, `ui/src/stores/sessions.ts`, `ui/src/stores/connect.ts`
- Create: `ui/src/shell/useGlobalShortcuts.ts`, `ui/src/shell/Rail.tsx`, `ui/src/shell/Rail.module.css`, `ui/src/shell/AppShell.tsx`, `ui/src/shell/AppShell.module.css`, `ui/src/shell/ModalLayer.tsx`, `ui/src/shell/StubDialog.tsx`, `ui/src/shell/ScreenPlaceholder.tsx`, `ui/src/shell/ScreenPlaceholder.module.css`
- Create (stubs that later tasks replace): `ui/src/screens/hosts/HostsScreen.tsx` (Task 19), `ui/src/screens/terminal/TerminalScreen.tsx` (Task 22), `ui/src/screens/editor/EditorModal.tsx` (Task 20), `ui/src/screens/connect/ConnectScreen.tsx` (Task 21), `ui/src/screens/settings/SettingsDialog.tsx`, `FirstRunDialog.tsx`, `ExportDialog.tsx`, `ImportDialog.tsx` (Task 23)
- Create (final for TP1): `ui/src/screens/sftp/SftpPlaceholder.tsx`
- Modify (full replacement): `ui/src/App.tsx`, `ui/src/App.test.tsx`
- Test: `ui/src/stores/stores.test.ts`, `ui/src/shell/useGlobalShortcuts.test.tsx`, `ui/src/shell/Rail.test.tsx`, `ui/src/shell/AppShell.test.tsx`, `ui/src/App.test.tsx`

**Interfaces:**
- Consumes: `ipc/api.ts` (Task 16), `ipc/errors.ts`, component kit (Task 17), `t` (Task 16).
- Produces (store shapes — screens use exactly these):
  ```ts
  // stores/ui.ts
  type View = "hosts" | "terminal" | "sftp";
  type EditorTarget = { kind: "host"; id?: Id } | { kind: "profile"; id?: Id };   // id undefined = create new
  type ModalState =
    | { type: "editor"; target: EditorTarget }
    | { type: "settings" } | { type: "firstRun" } | { type: "export" } | { type: "import" } | { type: "connect" };
  useUiStore: { view; setView(v); modal: ModalState | null; openModal(m); closeModal();
                theme: Theme; setTheme(t) /* toggles "light" on <html> */; searchFocusToken: number; focusSearch() }
  applyTheme(theme: Theme): void
  // stores/settings.ts
  DEFAULT_SETTINGS: Settings
  useSettingsStore: { settings: Settings; loaded: boolean; load(): Promise<void>;
                      save(patch: Partial<Settings>): Promise<Settings> /* optimistic, reverts + rethrows on error */ }
  // stores/hosts.ts
  type ProtocolFilter = "all" | "ssh" | "sftp";
  useHostsStore: { hosts: Host[]; groups: Group[]; profiles: ProfileWithUsage[]; loaded: boolean;
                   filter: ProtocolFilter; query: string; groupId: Id | null; selectedId: Id | null;
                   load(); reloadSshConfig(); saveHost(h): Promise<Host>; deleteHost(id); saveProfile(p): Promise<LoginProfile>;
                   deleteProfile(id); saveGroup(g): Promise<Group>; deleteGroup(id);
                   setFilter(f); setQuery(q); setGroupId(id | null); select(id | null) }   // mutations reload all lists
  // stores/sessions.ts
  type Tab = { id: string; title: string; kind: "ssh" | "local"; hostId?: Id; sessionId?: SessionId;
               status: "connecting" | "connected" | "closed" | "error" };
  useSessionsStore: { tabs: Tab[]; activeId: string | null; addTab(tab) /* activates */; closeTab(id) /* activates right, else left neighbour */;
                      setActive(id); updateTab(id, patch); openLocal(): Promise<void> /* no-op until Task 22 */ }
  // stores/connect.ts (shape only; flow in Task 21)
  type ConnectFlow = { connectId: string; hostId: Id; protocol: "ssh" | "sftp"; steps: Step[]; current?: Step; done: Step[];
                       failed?: { step: Step; error: CoreError }; log: string[]; prompt?: { promptId: number; prompt: Prompt };
                       attempt: number; autoRetry: boolean };
  useConnectStore: { active: ConnectFlow | null; start(hostId, protocol): Promise<void>; answer(promptId, answer): Promise<void>;
                     cancel(): Promise<void>; retry(): Promise<void> }
  ```
- Produces (components): `AppShell` (keeps all three views mounted, inactive ones `hidden`), `Rail`, `ModalLayer`, `StubDialog({ title })`, `ScreenPlaceholder({ title, text? })`, `useGlobalShortcuts()`, `shortcutFor(event): Shortcut | null`.
- Shortcuts (window `keydown`, capture phase, ignored while a modal is open): `Ctrl+K` → `setView("hosts")` + `focusSearch()`; `Ctrl+N` → `openModal({ type: "editor", target: { kind: "host" } })`; `Ctrl+Shift+T` → `useSessionsStore.getState().openLocal()`; `Ctrl+,` → `openModal({ type: "settings" })`. Inside an element with `data-terminal` only `Ctrl+Shift+T` and `Ctrl+,` are handled (Ctrl+K/Ctrl+N stay with the shell).

- [ ] **Step 1: Write the failing store tests**

**`ui/src/stores/stores.test.ts`**
```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { useConnectStore } from "./connect";
import { useHostsStore } from "./hosts";
import { useSessionsStore } from "./sessions";
import { DEFAULT_SETTINGS, useSettingsStore } from "./settings";
import { useUiStore } from "./ui";

vi.mock("../ipc/api");

const host = (id: string, name: string) => ({ id, name }) as unknown as api.Host;

function mockLists() {
  vi.mocked(api.hostsList).mockResolvedValue([]);
  vi.mocked(api.groupsList).mockResolvedValue([]);
  vi.mocked(api.profilesList).mockResolvedValue([]);
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSessionsStore.setState(useSessionsStore.getInitialState(), true);
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
});

describe("ui store", () => {
  it("applies the theme class to <html>", () => {
    useUiStore.getState().setTheme("light");
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(useUiStore.getState().theme).toBe("light");
    useUiStore.getState().setTheme("dark");
    expect(document.documentElement.classList.contains("light")).toBe(false);
  });

  it("opens/closes modals and bumps the search focus token", () => {
    useUiStore.getState().openModal({ type: "editor", target: { kind: "host" } });
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host" } });
    useUiStore.getState().closeModal();
    expect(useUiStore.getState().modal).toBeNull();
    useUiStore.getState().focusSearch();
    expect(useUiStore.getState().searchFocusToken).toBe(1);
  });
});

describe("settings store", () => {
  it("loads the persisted settings", async () => {
    vi.mocked(api.settingsGet).mockResolvedValue({ ...DEFAULT_SETTINGS, theme: "light" });
    await useSettingsStore.getState().load();
    expect(useSettingsStore.getState()).toMatchObject({ loaded: true, settings: { theme: "light" } });
  });

  it("saves optimistically and keeps the persisted result", async () => {
    vi.mocked(api.settingsSet).mockImplementation(async (s) => ({ ...s, terminalFontSize: 15 }));
    const pending = useSettingsStore.getState().save({ sshMode: "mirror" });
    expect(useSettingsStore.getState().settings.sshMode).toBe("mirror");
    await pending;
    expect(api.settingsSet).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, sshMode: "mirror" });
    expect(useSettingsStore.getState().settings.terminalFontSize).toBe(15);
  });

  it("reverts and rethrows when saving fails", async () => {
    const error = { type: "io", message: "denied" };
    vi.mocked(api.settingsSet).mockRejectedValue(error);
    await expect(useSettingsStore.getState().save({ theme: "light" })).rejects.toEqual(error);
    expect(useSettingsStore.getState().settings.theme).toBe("dark");
  });
});

describe("hosts store", () => {
  it("loads hosts, groups and profiles", async () => {
    mockLists();
    vi.mocked(api.hostsList).mockResolvedValue([host("h1", "web-prod-01")]);
    await useHostsStore.getState().load();
    expect(useHostsStore.getState()).toMatchObject({ loaded: true, hosts: [{ id: "h1" }], groups: [], profiles: [] });
  });

  it("reloads after mutations and clears the selection of a deleted host", async () => {
    mockLists();
    vi.mocked(api.hostSave).mockImplementation(async (h) => h);
    vi.mocked(api.hostDelete).mockResolvedValue(undefined);
    await expect(useHostsStore.getState().saveHost(host("h2", "db"))).resolves.toMatchObject({ id: "h2" });
    expect(api.hostsList).toHaveBeenCalledTimes(1);
    useHostsStore.getState().select("h2");
    await useHostsStore.getState().deleteHost("h2");
    expect(useHostsStore.getState().selectedId).toBeNull();
    expect(api.hostsList).toHaveBeenCalledTimes(2);
  });

  it("keeps filter, query and group selection", () => {
    const s = useHostsStore.getState();
    s.setFilter("sftp");
    s.setQuery("prod");
    s.setGroupId("g1");
    expect(useHostsStore.getState()).toMatchObject({ filter: "sftp", query: "prod", groupId: "g1" });
  });
});

describe("sessions store", () => {
  it("activates new tabs and a neighbour when the active tab closes", () => {
    const s = useSessionsStore.getState();
    s.addTab({ id: "a", title: "web", kind: "ssh", status: "connecting" });
    s.addTab({ id: "b", title: "Lokal", kind: "local", status: "connected" });
    s.addTab({ id: "c", title: "db", kind: "ssh", status: "connected" });
    expect(useSessionsStore.getState().activeId).toBe("c");
    useSessionsStore.getState().setActive("b");
    useSessionsStore.getState().closeTab("b");
    expect(useSessionsStore.getState().activeId).toBe("c");
    useSessionsStore.getState().updateTab("c", { status: "closed" });
    expect(useSessionsStore.getState().tabs.find((tab) => tab.id === "c")?.status).toBe("closed");
    useSessionsStore.getState().closeTab("c");
    expect(useSessionsStore.getState().activeId).toBe("a");
    useSessionsStore.getState().closeTab("a");
    expect(useSessionsStore.getState()).toMatchObject({ tabs: [], activeId: null });
  });
});

describe("connect store", () => {
  it("starts without an active flow", () => {
    expect(useConnectStore.getState().active).toBeNull();
  });
});
```

Run: `bun run test stores`
Expected: FAIL — cannot resolve `./connect`, `./hosts`, `./sessions`, `./settings`, `./ui`.

- [ ] **Step 2: Implement the stores**

**`ui/src/stores/ui.ts`**
```ts
import { create } from "zustand";
import type { Id, Theme } from "../ipc/api";

export type View = "hosts" | "terminal" | "sftp";

/** `id` undefined = create a new host/profile. */
export type EditorTarget = { kind: "host"; id?: Id } | { kind: "profile"; id?: Id };

export type ModalState =
  | { type: "editor"; target: EditorTarget }
  | { type: "settings" }
  | { type: "firstRun" }
  | { type: "export" }
  | { type: "import" }
  | { type: "connect" };

export interface UiState {
  view: View;
  setView(view: View): void;
  modal: ModalState | null;
  openModal(modal: ModalState): void;
  closeModal(): void;
  theme: Theme;
  /** Applies the theme immediately; persisting is the settings store's job. */
  setTheme(theme: Theme): void;
  /** Incremented by Ctrl+K; the host search input focuses itself when it changes. */
  searchFocusToken: number;
  focusSearch(): void;
}

/** Tokens live on `.app` (= <html>); `.app.light` switches to the light palette. */
export function applyTheme(theme: Theme): void {
  document.documentElement.classList.toggle("light", theme === "light");
}

export const useUiStore = create<UiState>()((set) => ({
  view: "hosts",
  setView: (view) => set({ view }),
  modal: null,
  openModal: (modal) => set({ modal }),
  closeModal: () => set({ modal: null }),
  theme: "dark",
  setTheme: (theme) => {
    applyTheme(theme);
    set({ theme });
  },
  searchFocusToken: 0,
  focusSearch: () => set((state) => ({ searchFocusToken: state.searchFocusToken + 1 })),
}));
```

**`ui/src/stores/settings.ts`**
```ts
import { create } from "zustand";
import { type Settings, settingsGet, settingsSet } from "../ipc/api";

export const DEFAULT_SETTINGS: Settings = {
  theme: "dark",
  sshMode: "integrate",
  terminalRenderer: "auto",
  terminalFontSize: 14,
  firstRunDone: false,
  compatRendering: false,
};

export interface SettingsState {
  settings: Settings;
  loaded: boolean;
  load(): Promise<void>;
  /** Optimistic: applies the patch immediately, keeps the persisted result, reverts and rethrows on error. */
  save(patch: Partial<Settings>): Promise<Settings>;
}

export const useSettingsStore = create<SettingsState>()((set, get) => ({
  settings: DEFAULT_SETTINGS,
  loaded: false,
  load: async () => {
    const settings = await settingsGet();
    set({ settings, loaded: true });
  },
  save: async (patch) => {
    const previous = get().settings;
    const next = { ...previous, ...patch };
    set({ settings: next });
    try {
      const saved = await settingsSet(next);
      set({ settings: saved });
      return saved;
    } catch (error) {
      set({ settings: previous });
      throw error;
    }
  },
}));
```

**`ui/src/stores/hosts.ts`**
```ts
import { create } from "zustand";
import {
  type Group,
  groupDelete,
  groupSave,
  groupsList,
  type Host,
  hostDelete,
  hostSave,
  hostsList,
  type Id,
  type LoginProfile,
  type ProfileWithUsage,
  profileDelete,
  profileSave,
  profilesList,
  sshConfigReload,
} from "../ipc/api";

export type ProtocolFilter = "all" | "ssh" | "sftp";

export interface HostsState {
  hosts: Host[];
  groups: Group[];
  profiles: ProfileWithUsage[];
  loaded: boolean;
  filter: ProtocolFilter;
  query: string;
  /** Sidebar group selection; `null` = "Alle Hosts". */
  groupId: Id | null;
  selectedId: Id | null;
  load(): Promise<void>;
  reloadSshConfig(): Promise<void>;
  saveHost(host: Host): Promise<Host>;
  deleteHost(id: Id): Promise<void>;
  saveProfile(profile: LoginProfile): Promise<LoginProfile>;
  deleteProfile(id: Id): Promise<void>;
  saveGroup(group: Group): Promise<Group>;
  deleteGroup(id: Id): Promise<void>;
  setFilter(filter: ProtocolFilter): void;
  setQuery(query: string): void;
  setGroupId(groupId: Id | null): void;
  select(id: Id | null): void;
}

export const useHostsStore = create<HostsState>()((set, get) => ({
  hosts: [],
  groups: [],
  profiles: [],
  loaded: false,
  filter: "all",
  query: "",
  groupId: null,
  selectedId: null,
  load: async () => {
    const [hosts, groups, profiles] = await Promise.all([hostsList(), groupsList(), profilesList()]);
    set({ hosts, groups, profiles, loaded: true });
  },
  reloadSshConfig: async () => {
    set({ hosts: await sshConfigReload() });
  },
  saveHost: async (host) => {
    const saved = await hostSave(host);
    await get().load();
    return saved;
  },
  deleteHost: async (id) => {
    await hostDelete(id);
    set((state) => ({ selectedId: state.selectedId === id ? null : state.selectedId }));
    await get().load();
  },
  saveProfile: async (profile) => {
    const saved = await profileSave(profile);
    await get().load();
    return saved;
  },
  deleteProfile: async (id) => {
    await profileDelete(id);
    await get().load();
  },
  saveGroup: async (group) => {
    const saved = await groupSave(group);
    await get().load();
    return saved;
  },
  deleteGroup: async (id) => {
    await groupDelete(id);
    set((state) => ({ groupId: state.groupId === id ? null : state.groupId }));
    await get().load();
  },
  setFilter: (filter) => set({ filter }),
  setQuery: (query) => set({ query }),
  setGroupId: (groupId) => set({ groupId }),
  select: (selectedId) => set({ selectedId }),
}));
```

**`ui/src/stores/sessions.ts`**
```ts
import { create } from "zustand";
import type { Id, SessionId } from "../ipc/api";

export type TabKind = "ssh" | "local";
export type TabStatus = "connecting" | "connected" | "closed" | "error";

export interface Tab {
  id: string;
  title: string;
  kind: TabKind;
  hostId?: Id;
  sessionId?: SessionId;
  status: TabStatus;
}

export interface SessionsState {
  tabs: Tab[];
  activeId: string | null;
  /** Appends and activates the tab. */
  addTab(tab: Tab): void;
  /** Removes the tab; if it was active, activates the right neighbour, else the left one. */
  closeTab(id: string): void;
  setActive(id: string): void;
  updateTab(id: string, patch: Partial<Omit<Tab, "id">>): void;
  /** Opens a local terminal tab (Ctrl+Shift+T, "+" in the tab bar). Implemented in Task 22. */
  openLocal(): Promise<void>;
}

export const useSessionsStore = create<SessionsState>()((set) => ({
  tabs: [],
  activeId: null,
  addTab: (tab) => set((state) => ({ tabs: [...state.tabs, tab], activeId: tab.id })),
  closeTab: (id) =>
    set((state) => {
      const index = state.tabs.findIndex((tab) => tab.id === id);
      if (index === -1) return state;
      const tabs = state.tabs.filter((tab) => tab.id !== id);
      if (state.activeId !== id) return { tabs };
      const neighbour = tabs[index] ?? tabs[index - 1];
      return { tabs, activeId: neighbour ? neighbour.id : null };
    }),
  setActive: (activeId) => set({ activeId }),
  updateTab: (id, patch) =>
    set((state) => ({ tabs: state.tabs.map((tab) => (tab.id === id ? { ...tab, ...patch } : tab)) })),
  openLocal: async () => undefined,
}));
```

**`ui/src/stores/connect.ts`**
```ts
import { create } from "zustand";
import type { CoreError, Id, Prompt, PromptAnswer, Step } from "../ipc/api";

export type ConnectProtocol = "ssh" | "sftp";

export interface ConnectFlow {
  connectId: string;
  hostId: Id;
  protocol: ConnectProtocol;
  steps: Step[];
  current?: Step;
  done: Step[];
  failed?: { step: Step; error: CoreError };
  log: string[];
  prompt?: { promptId: number; prompt: Prompt };
  attempt: number;
  autoRetry: boolean;
}

export interface ConnectState {
  active: ConnectFlow | null;
  start(hostId: Id, protocol: ConnectProtocol): Promise<void>;
  answer(promptId: number, answer: PromptAnswer): Promise<void>;
  cancel(): Promise<void>;
  retry(): Promise<void>;
}

/** State shape only — Task 21 implements the connection flow. */
export const useConnectStore = create<ConnectState>()(() => ({
  active: null,
  start: async () => undefined,
  answer: async () => undefined,
  cancel: async () => undefined,
  retry: async () => undefined,
}));
```

Run: `bun run test stores`
Expected: PASS (10 tests).

- [ ] **Step 3: Write the failing shortcut tests**

**`ui/src/shell/useGlobalShortcuts.test.tsx`**
```tsx
import { createEvent, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { useGlobalShortcuts } from "./useGlobalShortcuts";

vi.mock("../ipc/api");

function Harness() {
  useGlobalShortcuts();
  return (
    <div>
      <input aria-label="Suche" />
      <div data-terminal="">
        <textarea aria-label="Terminal-Eingabe" />
      </div>
    </div>
  );
}

const openLocal = vi.fn(async () => undefined);

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useSessionsStore.setState({ ...useSessionsStore.getInitialState(), openLocal }, true);
});

describe("useGlobalShortcuts", () => {
  it("Ctrl+K shows the host overview and focuses the search", () => {
    useUiStore.getState().setView("terminal");
    render(<Harness />);
    const event = createEvent.keyDown(document.body, { key: "k", ctrlKey: true });
    fireEvent(document.body, event);
    expect(event.defaultPrevented).toBe(true);
    expect(useUiStore.getState()).toMatchObject({ view: "hosts", searchFocusToken: 1 });
  });

  it("Ctrl+N opens the editor for a new host", () => {
    render(<Harness />);
    fireEvent.keyDown(document.body, { key: "n", ctrlKey: true });
    expect(useUiStore.getState().modal).toEqual({ type: "editor", target: { kind: "host" } });
  });

  it("Ctrl+Shift+T opens a local terminal", () => {
    render(<Harness />);
    fireEvent.keyDown(document.body, { key: "T", ctrlKey: true, shiftKey: true });
    expect(openLocal).toHaveBeenCalledOnce();
  });

  it("Ctrl+, opens the settings", () => {
    render(<Harness />);
    fireEvent.keyDown(screen.getByLabelText("Suche"), { key: ",", ctrlKey: true });
    expect(useUiStore.getState().modal).toEqual({ type: "settings" });
  });

  it("leaves Ctrl+K and Ctrl+N to the terminal but handles Ctrl+Shift+T there", () => {
    render(<Harness />);
    const terminal = screen.getByLabelText("Terminal-Eingabe");
    const event = createEvent.keyDown(terminal, { key: "k", ctrlKey: true });
    fireEvent(terminal, event);
    expect(event.defaultPrevented).toBe(false);
    fireEvent.keyDown(terminal, { key: "n", ctrlKey: true });
    expect(useUiStore.getState()).toMatchObject({ searchFocusToken: 0, modal: null });
    fireEvent.keyDown(terminal, { key: "T", ctrlKey: true, shiftKey: true });
    expect(openLocal).toHaveBeenCalledOnce();
  });

  it("is inactive while a modal is open and ignores other combinations", () => {
    render(<Harness />);
    fireEvent.keyDown(document.body, { key: "k" });
    fireEvent.keyDown(document.body, { key: "k", ctrlKey: true, altKey: true });
    expect(useUiStore.getState().searchFocusToken).toBe(0);
    useUiStore.getState().openModal({ type: "import" });
    fireEvent.keyDown(document.body, { key: ",", ctrlKey: true });
    expect(useUiStore.getState().modal).toEqual({ type: "import" });
  });
});
```

Run: `bun run test useGlobalShortcuts`
Expected: FAIL — cannot resolve `./useGlobalShortcuts`.

- [ ] **Step 4: Implement the shortcuts hook**

**`ui/src/shell/useGlobalShortcuts.ts`**
```ts
import { useEffect } from "react";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";

export type Shortcut = "search" | "newHost" | "newLocalTerminal" | "settings";

type KeyInfo = Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

/** Maps a key event to an app shortcut (Ctrl+K, Ctrl+N, Ctrl+Shift+T, Ctrl+,). */
export function shortcutFor(event: KeyInfo): Shortcut | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;
  const key = event.key.toLowerCase();
  if (event.shiftKey) return key === "t" ? "newLocalTerminal" : null;
  switch (key) {
    case "k":
      return "search";
    case "n":
      return "newHost";
    case ",":
      return "settings";
    default:
      return null;
  }
}

/** Shortcuts that also apply while focus is inside a terminal (`[data-terminal]`). */
const TERMINAL_SAFE: ReadonlySet<Shortcut> = new Set<Shortcut>(["newLocalTerminal", "settings"]);

function insideTerminal(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest("[data-terminal]") !== null;
}

/**
 * Registers the global shortcuts on `window` in the capture phase, so they win over xterm.js
 * (which stops propagation of keys it handles). Inactive while a modal is open.
 */
export function useGlobalShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const shortcut = shortcutFor(event);
      if (!shortcut) return;
      const ui = useUiStore.getState();
      if (ui.modal) return;
      if (!TERMINAL_SAFE.has(shortcut) && insideTerminal(event.target)) return;
      event.preventDefault();
      event.stopPropagation();
      switch (shortcut) {
        case "search":
          ui.setView("hosts");
          ui.focusSearch();
          break;
        case "newHost":
          ui.openModal({ type: "editor", target: { kind: "host" } });
          break;
        case "newLocalTerminal":
          void useSessionsStore.getState().openLocal();
          break;
        case "settings":
          ui.openModal({ type: "settings" });
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}
```

Run: `bun run test useGlobalShortcuts`
Expected: PASS (6 tests).

- [ ] **Step 5: Write the failing Rail tests**

**`ui/src/shell/Rail.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../components";
import * as api from "../ipc/api";
import { useSettingsStore } from "../stores/settings";
import { useUiStore } from "../stores/ui";
import { Rail } from "./Rail";

vi.mock("../ipc/api");

function renderRail() {
  return render(
    <ToastProvider>
      <Rail />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
});

describe("Rail", () => {
  it("navigates between the views", async () => {
    renderRail();
    expect(screen.getByRole("navigation", { name: "Hauptnavigation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hosts" })).toHaveAttribute("aria-current", "page");
    await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
    expect(useUiStore.getState().view).toBe("terminal");
    expect(screen.getByRole("button", { name: "Terminal" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Hosts" })).not.toHaveAttribute("aria-current");
    await userEvent.click(screen.getByRole("button", { name: "SFTP" }));
    expect(useUiStore.getState().view).toBe("sftp");
  });

  it("shows later features as disabled with a tooltip", async () => {
    renderRail();
    for (const name of ["Snippets", "Schlüsselbund", "Port-Forwarding"]) {
      const item = screen.getByRole("button", { name });
      expect(item).toHaveAttribute("aria-disabled", "true");
      expect(item).toHaveAttribute("title", `${name} – Folgt in einer späteren Version`);
      await userEvent.click(item);
    }
    expect(useUiStore.getState()).toMatchObject({ view: "hosts", modal: null });
  });

  it("opens the settings dialog", async () => {
    renderRail();
    await userEvent.click(screen.getByRole("button", { name: "Einstellungen" }));
    expect(useUiStore.getState().modal).toEqual({ type: "settings" });
  });

  it("toggles and persists the theme", async () => {
    renderRail();
    await userEvent.click(screen.getByRole("button", { name: "Helles Design" }));
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ theme: "light" }));
    await userEvent.click(screen.getByRole("button", { name: "Dunkles Design" }));
    expect(document.documentElement.classList.contains("light")).toBe(false);
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ theme: "dark" }));
  });

  it("reports a failed theme save", async () => {
    vi.mocked(api.settingsSet).mockRejectedValue({ type: "io", message: "read-only" });
    renderRail();
    await userEvent.click(screen.getByRole("button", { name: "Helles Design" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Speichern fehlgeschlagen");
  });
});
```

Run: `bun run test Rail`
Expected: FAIL — cannot resolve `./Rail`.

- [ ] **Step 6: Implement the Rail**

**`ui/src/shell/Rail.tsx`**
```tsx
import {
  ArrowRightLeft,
  CodeXml,
  FolderSync,
  KeyRound,
  type LucideIcon,
  Moon,
  Server,
  Settings,
  SquareTerminal,
  Sun,
} from "lucide-react";
import { cx, useToast } from "../components";
import { t } from "../i18n/de";
import { errorDetails } from "../ipc/errors";
import { useSettingsStore } from "../stores/settings";
import { useUiStore, type View } from "../stores/ui";
import styles from "./Rail.module.css";

const VIEW_ITEMS: ReadonlyArray<{ view: View; label: string; icon: LucideIcon }> = [
  { view: "hosts", label: t.rail.hosts, icon: Server },
  { view: "terminal", label: t.rail.terminal, icon: SquareTerminal },
  { view: "sftp", label: t.rail.sftp, icon: FolderSync },
];

const LATER_ITEMS: ReadonlyArray<{ label: string; icon: LucideIcon }> = [
  { label: t.rail.snippets, icon: CodeXml },
  { label: t.rail.keychain, icon: KeyRound },
  { label: t.rail.portForwarding, icon: ArrowRightLeft },
];

const ICON_PROPS = { size: 20, strokeWidth: 1.8, "aria-hidden": true } as const;

export function Rail() {
  const view = useUiStore((state) => state.view);
  const setView = useUiStore((state) => state.setView);
  const openModal = useUiStore((state) => state.openModal);
  const theme = useUiStore((state) => state.theme);
  const setTheme = useUiStore((state) => state.setTheme);
  const saveSettings = useSettingsStore((state) => state.save);
  const toast = useToast();

  const toggleTheme = () => {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    saveSettings({ theme: next }).catch((error: unknown) =>
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) }),
    );
  };
  // The toggle names (and shows the icon of) the mode it switches to.
  const themeLabel = theme === "dark" ? t.rail.toLight : t.rail.toDark;
  const ThemeIcon = theme === "dark" ? Sun : Moon;

  return (
    <nav className={styles.rail} aria-label={t.rail.nav}>
      <div className={styles.logo} aria-hidden="true">
        &gt;_
      </div>
      {VIEW_ITEMS.map(({ view: target, label, icon: Icon }) => (
        <button
          key={target}
          type="button"
          className={cx(styles.item, view === target && styles.active)}
          aria-label={label}
          title={label}
          aria-current={view === target ? "page" : undefined}
          onClick={() => setView(target)}
        >
          <Icon {...ICON_PROPS} />
        </button>
      ))}
      {LATER_ITEMS.map(({ label, icon: Icon }) => (
        <button
          key={label}
          type="button"
          className={cx(styles.item, styles.disabled)}
          aria-label={label}
          aria-disabled="true"
          title={`${label} – ${t.common.comingSoon}`}
        >
          <Icon {...ICON_PROPS} />
        </button>
      ))}
      <div className={styles.spacer} />
      <button
        type="button"
        className={styles.item}
        aria-label={t.rail.settings}
        title={t.rail.settings}
        onClick={() => openModal({ type: "settings" })}
      >
        <Settings {...ICON_PROPS} />
      </button>
      <button type="button" className={styles.item} aria-label={themeLabel} title={themeLabel} onClick={toggleTheme}>
        <ThemeIcon {...ICON_PROPS} />
      </button>
    </nav>
  );
}
```

**`ui/src/shell/Rail.module.css`**
```css
.rail {
  display: flex;
  flex: 0 0 64px;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  width: 64px;
  padding: 14px 0;
  border-right: 1px solid var(--bd);
  background: var(--s1);
}

.logo {
  display: grid;
  place-items: center;
  width: 36px;
  height: 36px;
  margin-bottom: 14px;
  border-radius: 10px;
  background: var(--accent);
  color: var(--on-accent);
  font-family: var(--font-mono);
  font-size: 15px;
  font-weight: 600;
  user-select: none;
}

.item {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  padding: 0;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}

.item:hover {
  background: var(--hover);
  color: var(--text);
}

.active,
.active:hover {
  background: var(--raised);
  color: var(--accent);
}

.disabled,
.disabled:hover {
  background: transparent;
  color: var(--dim);
  cursor: not-allowed;
}

.spacer {
  flex: 1;
}
```

Run: `bun run test Rail`
Expected: PASS (5 tests).

- [ ] **Step 7: Write the failing AppShell / App tests**

**`ui/src/shell/AppShell.test.tsx`**
```tsx
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../components";
import * as api from "../ipc/api";
import { useHostsStore } from "../stores/hosts";
import { DEFAULT_SETTINGS, useSettingsStore } from "../stores/settings";
import { type ModalState, useUiStore } from "../stores/ui";
import { AppShell } from "./AppShell";

vi.mock("../ipc/api");

function mockBackend(settings: Partial<api.Settings> = {}) {
  vi.mocked(api.settingsGet).mockResolvedValue({ ...DEFAULT_SETTINGS, firstRunDone: true, ...settings });
  vi.mocked(api.hostsList).mockResolvedValue([]);
  vi.mocked(api.groupsList).mockResolvedValue([]);
  vi.mocked(api.profilesList).mockResolvedValue([]);
}

function renderShell() {
  return render(
    <ToastProvider>
      <AppShell />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
});

describe("AppShell", () => {
  it("renders the rail and the host overview and loads data", async () => {
    mockBackend();
    renderShell();
    expect(screen.getByRole("navigation", { name: "Hauptnavigation" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Server-Übersicht" })).toBeInTheDocument();
    await waitFor(() => expect(useSettingsStore.getState().loaded).toBe(true));
    await waitFor(() => expect(useHostsStore.getState().loaded).toBe(true));
  });

  it("switches views via the rail and keeps inactive views mounted", async () => {
    mockBackend();
    renderShell();
    await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
    expect(screen.getByRole("heading", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Server-Übersicht" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Server-Übersicht", hidden: true })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "SFTP" }));
    expect(screen.getByRole("heading", { name: "SFTP" })).toBeInTheDocument();
    expect(screen.getByText("Der SFTP-Dateimanager folgt in einer späteren Version.")).toBeVisible();
  });

  it("applies the persisted theme", async () => {
    mockBackend({ theme: "light" });
    renderShell();
    await waitFor(() => expect(document.documentElement.classList.contains("light")).toBe(true));
  });

  it("opens the first-run dialog while the first run is not done", async () => {
    mockBackend({ firstRunDone: false });
    renderShell();
    expect(await screen.findByRole("dialog", { name: "Willkommen bei LinaSCP" })).toBeInTheDocument();
  });

  it("renders every modal type", async () => {
    mockBackend();
    renderShell();
    const cases: Array<[ModalState, string]> = [
      [{ type: "editor", target: { kind: "host" } }, "Host bearbeiten"],
      [{ type: "editor", target: { kind: "profile", id: "p1" } }, "Login-Profil bearbeiten"],
      [{ type: "settings" }, "Einstellungen"],
      [{ type: "firstRun" }, "Willkommen bei LinaSCP"],
      [{ type: "export" }, "Exportieren"],
      [{ type: "import" }, "Importieren"],
      [{ type: "connect" }, "Verbindung"],
    ];
    for (const [modal, title] of cases) {
      act(() => useUiStore.getState().openModal(modal));
      expect(screen.getByRole("dialog", { name: title })).toBeInTheDocument();
    }
    await userEvent.click(screen.getByRole("button", { name: "Schließen" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("reports load failures as an error toast", async () => {
    mockBackend();
    vi.mocked(api.settingsGet).mockRejectedValue({ type: "store", message: "database is locked" });
    renderShell();
    expect(await screen.findByRole("alert")).toHaveTextContent("Daten konnten nicht geladen werden");
  });
});
```

**`ui/src/App.test.tsx`**
```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { App } from "./App";
import * as api from "./ipc/api";
import { DEFAULT_SETTINGS } from "./stores/settings";

vi.mock("./ipc/api");

describe("App", () => {
  it("renders the application shell", async () => {
    vi.mocked(api.settingsGet).mockResolvedValue({ ...DEFAULT_SETTINGS, firstRunDone: true });
    vi.mocked(api.hostsList).mockResolvedValue([]);
    vi.mocked(api.groupsList).mockResolvedValue([]);
    vi.mocked(api.profilesList).mockResolvedValue([]);
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Hauptnavigation" })).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Server-Übersicht" })).toBeInTheDocument();
  });
});
```

Run: `bun run test AppShell App`
Expected: FAIL — cannot resolve `./AppShell`; App test fails because the shell is not rendered yet.

- [ ] **Step 8: Implement placeholders, stubs, ModalLayer, AppShell and App**

**`ui/src/shell/ScreenPlaceholder.tsx`**
```tsx
import styles from "./ScreenPlaceholder.module.css";

export function ScreenPlaceholder({ title, text }: { title: string; text?: string }) {
  return (
    <div className={styles.placeholder}>
      <h1 className={styles.title}>{title}</h1>
      {text ? <p className={styles.text}>{text}</p> : null}
    </div>
  );
}
```

**`ui/src/shell/ScreenPlaceholder.module.css`**
```css
.placeholder {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 40px;
  color: var(--muted);
}

.title {
  color: var(--text);
  font-size: 20px;
  font-weight: 600;
}

.text {
  font-size: 14px;
}
```

**`ui/src/shell/StubDialog.tsx`**
```tsx
import { Dialog } from "../components";
import { t } from "../i18n/de";
import { useUiStore } from "../stores/ui";

/** Temporary modal body until the real dialog of a later task replaces the stub component. */
export function StubDialog({ title }: { title: string }) {
  const closeModal = useUiStore((state) => state.closeModal);
  return (
    <Dialog open title={title} onClose={closeModal} size="sm">
      <p>{t.common.comingSoon}</p>
    </Dialog>
  );
}
```

**`ui/src/screens/hosts/HostsScreen.tsx`**
```tsx
import { t } from "../../i18n/de";
import { ScreenPlaceholder } from "../../shell/ScreenPlaceholder";

/** Stub — replaced by the host overview in Task 19 (must keep rendering an h1 "Server-Übersicht" or equivalent heading). */
export function HostsScreen() {
  return <ScreenPlaceholder title={t.stubs.hosts} />;
}
```

**`ui/src/screens/terminal/TerminalScreen.tsx`**
```tsx
import { t } from "../../i18n/de";
import { ScreenPlaceholder } from "../../shell/ScreenPlaceholder";

/** Stub — replaced by the terminal screen in Task 22. */
export function TerminalScreen() {
  return <ScreenPlaceholder title={t.stubs.terminal} />;
}
```

**`ui/src/screens/sftp/SftpPlaceholder.tsx`**
```tsx
import { t } from "../../i18n/de";
import { ScreenPlaceholder } from "../../shell/ScreenPlaceholder";

/** SFTP view in TP1: the file manager arrives in TP3. */
export function SftpPlaceholder() {
  return <ScreenPlaceholder title={t.stubs.sftp} text={t.stubs.sftpText} />;
}
```

**`ui/src/screens/editor/EditorModal.tsx`**
```tsx
import { t } from "../../i18n/de";
import { StubDialog } from "../../shell/StubDialog";
import type { EditorTarget } from "../../stores/ui";

/** Stub — replaced by the host/profile editor in Task 20 (same props). */
export function EditorModal({ target }: { target: EditorTarget }) {
  return <StubDialog title={target.kind === "host" ? t.stubs.editorHost : t.stubs.editorProfile} />;
}
```

**`ui/src/screens/connect/ConnectScreen.tsx`**
```tsx
import { t } from "../../i18n/de";
import { StubDialog } from "../../shell/StubDialog";

/** Stub — replaced by the connect flow screens in Task 21. */
export function ConnectScreen() {
  return <StubDialog title={t.stubs.connect} />;
}
```

**`ui/src/screens/settings/SettingsDialog.tsx`**
```tsx
import { t } from "../../i18n/de";
import { StubDialog } from "../../shell/StubDialog";

/** Stub — replaced in Task 23. */
export function SettingsDialog() {
  return <StubDialog title={t.stubs.settings} />;
}
```

**`ui/src/screens/settings/FirstRunDialog.tsx`**
```tsx
import { t } from "../../i18n/de";
import { StubDialog } from "../../shell/StubDialog";

/** Stub — replaced in Task 23 (must save `firstRunDone: true`). */
export function FirstRunDialog() {
  return <StubDialog title={t.stubs.firstRun} />;
}
```

**`ui/src/screens/settings/ExportDialog.tsx`**
```tsx
import { t } from "../../i18n/de";
import { StubDialog } from "../../shell/StubDialog";

/** Stub — replaced in Task 23. */
export function ExportDialog() {
  return <StubDialog title={t.stubs.export} />;
}
```

**`ui/src/screens/settings/ImportDialog.tsx`**
```tsx
import { t } from "../../i18n/de";
import { StubDialog } from "../../shell/StubDialog";

/** Stub — replaced in Task 23. */
export function ImportDialog() {
  return <StubDialog title={t.stubs.import} />;
}
```

**`ui/src/shell/ModalLayer.tsx`**
```tsx
import { ConnectScreen } from "../screens/connect/ConnectScreen";
import { EditorModal } from "../screens/editor/EditorModal";
import { ExportDialog } from "../screens/settings/ExportDialog";
import { FirstRunDialog } from "../screens/settings/FirstRunDialog";
import { ImportDialog } from "../screens/settings/ImportDialog";
import { SettingsDialog } from "../screens/settings/SettingsDialog";
import { useUiStore } from "../stores/ui";

/** Renders the single open modal (`useUiStore().modal`). */
export function ModalLayer() {
  const modal = useUiStore((state) => state.modal);
  if (!modal) return null;
  switch (modal.type) {
    case "editor":
      return <EditorModal target={modal.target} />;
    case "settings":
      return <SettingsDialog />;
    case "firstRun":
      return <FirstRunDialog />;
    case "export":
      return <ExportDialog />;
    case "import":
      return <ImportDialog />;
    case "connect":
      return <ConnectScreen />;
  }
}
```

**`ui/src/shell/AppShell.tsx`**
```tsx
import { useEffect } from "react";
import { useToast } from "../components";
import { t } from "../i18n/de";
import { errorDetails } from "../ipc/errors";
import { HostsScreen } from "../screens/hosts/HostsScreen";
import { SftpPlaceholder } from "../screens/sftp/SftpPlaceholder";
import { TerminalScreen } from "../screens/terminal/TerminalScreen";
import { useHostsStore } from "../stores/hosts";
import { useSettingsStore } from "../stores/settings";
import { useUiStore } from "../stores/ui";
import styles from "./AppShell.module.css";
import { ModalLayer } from "./ModalLayer";
import { Rail } from "./Rail";
import { useGlobalShortcuts } from "./useGlobalShortcuts";

/**
 * Rail + the three views + modal layer. All views stay mounted (inactive ones are `hidden`)
 * so terminal sessions survive view switches.
 */
export function AppShell() {
  const view = useUiStore((state) => state.view);
  const setTheme = useUiStore((state) => state.setTheme);
  const openModal = useUiStore((state) => state.openModal);
  const theme = useSettingsStore((state) => state.settings.theme);
  const firstRunDone = useSettingsStore((state) => state.settings.firstRunDone);
  const settingsLoaded = useSettingsStore((state) => state.loaded);
  const loadSettings = useSettingsStore((state) => state.load);
  const loadHosts = useHostsStore((state) => state.load);
  const toast = useToast();

  useGlobalShortcuts();

  useEffect(() => {
    const report = (error: unknown) =>
      toast.show({ tone: "red", message: t.shell.loadFailed, details: errorDetails(error) });
    loadSettings().catch(report);
    loadHosts().catch(report);
  }, [loadSettings, loadHosts, toast]);

  useEffect(() => {
    setTheme(theme);
  }, [theme, setTheme]);

  useEffect(() => {
    if (settingsLoaded && !firstRunDone) openModal({ type: "firstRun" });
  }, [settingsLoaded, firstRunDone, openModal]);

  return (
    <div className={styles.shell}>
      <Rail />
      <main className={styles.main}>
        <section className={styles.view} hidden={view !== "hosts"}>
          <HostsScreen />
        </section>
        <section className={styles.view} hidden={view !== "terminal"}>
          <TerminalScreen />
        </section>
        <section className={styles.view} hidden={view !== "sftp"}>
          <SftpPlaceholder />
        </section>
      </main>
      <ModalLayer />
    </div>
  );
}
```

**`ui/src/shell/AppShell.module.css`**
```css
.shell {
  display: flex;
  height: 100vh;
  min-height: 0;
  background: var(--bg);
  color: var(--text);
}

.main {
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.view {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.view[hidden] {
  display: none;
}
```

**`ui/src/App.tsx`**
```tsx
import { ToastProvider } from "./components";
import { AppShell } from "./shell/AppShell";

export function App() {
  return (
    <ToastProvider>
      <AppShell />
    </ToastProvider>
  );
}
```

Run: `bun run test AppShell App`
Expected: PASS (AppShell 6, App 1).

- [ ] **Step 9: Full verification**

Run: `bun run typecheck && bun run test && bun run build`
Expected: no type errors; all test files PASS (Task 16–18: 13 − 1 replaced App test + 1 new + 41 + 27 = 81 tests); build succeeds.

Run (manual, desktop session): `bun run tauri dev`
Expected: dark app with the 64 px rail; Hosts/Terminal/SFTP switch the view; Snippets/Schlüsselbund/Port-Forwarding greyed out with tooltip; theme toggle switches to light and survives a restart; first start shows the "Willkommen bei LinaSCP" stub dialog; Ctrl+, opens the settings stub.

- [ ] **Step 10: Commit**

```bash
git add ui/src
git commit -m "feat(ui): add zustand stores, app shell with rail, modal layer and global shortcuts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
