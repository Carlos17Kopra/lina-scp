# LinaSCP TP2a (Terminal Sidebar, Color Schemes, Session Info) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Selectable terminal color schemes and font sizes (global, with per-host overrides) and a collapsible terminal sidebar showing session info and design settings.

**Architecture:**
- **Core:**
  - `Host` gains two nullable override fields, stored through migration V2 and validated in `validate.rs`.
  - The connect pipeline collects a `SessionInfo` and attaches it to `SshSession`:
    - from `kex_done`: kex, cipher and MAC;
    - from `check_server_key`: the host key;
    - from `auth::authenticate`: the methods used.
- **Tauri:**
  - `Settings` gains `terminalScheme` and `terminalSidebarOpen`.
  - `connect_ssh` returns `ConnectedSession { sessionId, info }`.
- **UI:**
  - The effective prefs (host override over global) drive `TerminalView`.
  - A 280 px sidebar is added next to the panels. It has the tabs Sitzung / Design / Tunnel (disabled) and is toggled
    by a tab-bar button and `Ctrl+Shift+B`.

**Tech Stack:** Rust 2024 (rustc ≥ 1.88 via rustup: `export PATH=$HOME/.cargo/bin:$PATH`), russh 0.64, rusqlite, tauri-specta; React 19, zustand, xterm.js, Vitest + Testing Library, bun.

**Spec:** `docs/superpowers/specs/2026-10-15-linascp-tp2a-sidebar-design.md`

## Global Constraints

- Branch `feat/tp2a-sidebar` (already created, the spec is committed there). Commits end with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Language and naming:
  - Code, comments and identifiers are in English.
  - UI strings are in German and live only in `ui/src/i18n/de.ts`.
  - The app name is "LinaSCP", never "Shellport".
- Secrets never go into the DB, logs, events, errors, UI notices or `SessionInfo`.
- Font sizes: 10–20 px (`schemes.json` `fontSize.min/max`) for both host and settings. The default global size stays
  14.
- Scheme ids:
  - Valid values are `"auto"` or `[A-Za-z]{1,32}`.
  - The core validates only this format.
  - The UI maps unknown ids to `"auto"`.
- The terminal background stays opaque. Bright colors are the same as the normal colors, `black` = `bd` and
  `brightBlack` = `dim`.
- `ui/src/ipc/bindings.ts` is generated, never edit it by hand. Regenerate with
  `cargo test -p linascp exports_bindings`.
- TDD: write the failing test first. Test output stays free of warnings, including act() and jsdom noise.
- src-tauri tests need `SHELL=/bin/sh` on this machine (an interactive `.bashrc` makes the PTY tests hang).
- Full check before the final commit:
  ```bash
  export PATH=$HOME/.cargo/bin:$PATH
  cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
  SHELL=/bin/sh cargo test --workspace
  cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1
  bun run typecheck && bun run test && bun run build
  ```

## Review Focus

Inputs and conditions the spec implies but would otherwise go untested. Each one has its pinning test in the owning
task:

1. **Editing a host in the editor keeps its terminal overrides.** `draftToHost` builds a fresh object, so the fields
   would silently drop on every save → Task 1, Step 7 (drafts round-trip test).
2. **Settings saved by an older build with a font size outside 10–20 (8, 9, 21–32)** fall back to 14 on load
   instead of failing → Task 2, Step 1 (`invalid_stored_values_fall_back_to_defaults`).
3. **A stored or imported unknown scheme id** (`"solarizedNeon"` from a newer build) renders like `"auto"` and
   shows "Wie App-Design" as selected → Task 5, Step 1 and Task 7, Step 1.
4. **The host is deleted while its tab is open.** The tab falls back to the global values, the Sitzung tab shows "—",
   and "Nur diesen Host" is disabled → Task 5, Step 1 (`useTerminalPrefs`), Task 6, Step 1 and Task 7, Step 1.
5. **2FA logins (key + keyboard-interactive)** report both methods in order → Task 3, Step 1 (auth unit test) and
   Step 7 (Docker test).

---

### Task 1: Host terminal overrides (model, migration V2, store, validation, UI drafts)

**Files:**
- Modify: `crates/core/src/model.rs` (struct `Host`, `Host::new`, tests)
- Modify: `crates/core/src/store/migrations.rs` (V2 + test module)
- Modify: `crates/core/src/store/mod.rs` (`HOST_COLUMNS`, `upsert_host`, `host_from_row`)
- Modify: `crates/core/src/store/tests.rs` (`full_host`)
- Modify: `crates/core/src/validate.rs`
- Modify: `crates/core/src/sshconfig/mirror.rs` (test)
- Regenerate: `ui/src/ipc/bindings.ts`
- Modify: `ui/src/model/defaults.ts`, `ui/src/test/fixtures.ts`, `ui/src/screens/editor/drafts.ts`, `ui/src/screens/editor/drafts.test.ts`

**Interfaces:**
- Produces:
  - Rust: `Host.terminal_scheme: Option<String>`, `Host.terminal_font_size: Option<u16>`,
    `validate::TERMINAL_FONT_SIZES: RangeInclusive<u16>` (= `10..=20`) and
    `validate::is_valid_terminal_scheme(&str) -> bool`.
  - TS: `Host.terminalScheme: string | null` and `Host.terminalFontSize: number | null`. specta may emit them as
    optional (`?:`). Always write them explicitly as `null` in literals.

- [ ] **Step 1: Write the failing core tests**

In `crates/core/src/validate.rs`, add to `mod tests`:

```rust
    #[test]
    fn terminal_overrides_are_range_and_format_checked() {
        let mut host = Host::new("h", "h.example");
        for size in [10, 20] {
            host.terminal_font_size = Some(size);
            assert!(validate_host(&host).is_ok(), "size {size}");
        }
        for size in [9, 21] {
            host.terminal_font_size = Some(size);
            assert!(validate_host(&host).is_err(), "size {size}");
        }
        host.terminal_font_size = None;
        let longest = "x".repeat(32);
        for scheme in ["auto", "nord", "solDark", longest.as_str()] {
            host.terminal_scheme = Some(scheme.into());
            assert!(validate_host(&host).is_ok(), "scheme {scheme:?}");
        }
        let too_long = "x".repeat(33);
        for scheme in ["", "a b", "../x", "nörd", too_long.as_str()] {
            host.terminal_scheme = Some(scheme.into());
            assert!(validate_host(&host).is_err(), "scheme {scheme:?}");
        }
    }
```

At the end of `crates/core/src/store/migrations.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_adds_nullable_terminal_columns_to_existing_hosts() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V1).unwrap();
        conn.execute_batch("PRAGMA user_version = 1").unwrap();
        conn.execute(
            "INSERT INTO hosts (id, name, address, auth_method, created_at, updated_at) \
             VALUES ('a', 'n', 'h', 'password', 0, 0)",
            [],
        )
        .unwrap();
        run(&mut conn).unwrap();
        let row: (Option<String>, Option<i64>) = conn
            .query_row("SELECT terminal_scheme, terminal_font_size FROM hosts WHERE id = 'a'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(row, (None, None));
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 2);
    }
}
```

In `crates/core/src/store/tests.rs`, make `full_host()` set the overrides, so `host_roundtrip_with_all_fields`
covers them. Add these lines before the function's final expression:

```rust
    h.terminal_scheme = Some("nord".into());
    h.terminal_font_size = Some(16);
```

In `crates/core/src/model.rs` `mod tests`, add a test proving that old export files without the fields still load:

```rust
    #[test]
    fn host_without_terminal_fields_deserializes_to_none() {
        let mut json = serde_json::to_value(Host::new("a", "a.example")).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("terminalScheme");
        object.remove("terminalFontSize");
        let host: Host = serde_json::from_value(json).unwrap();
        assert_eq!((host.terminal_scheme, host.terminal_font_size), (None, None));
    }
```

In `crates/core/src/sshconfig/mirror.rs` `mod tests`, add:

```rust
    #[test]
    fn render_ignores_terminal_overrides() {
        let (mut hosts, profiles) = sample();
        let plain = Mirror::render(&hosts, &profiles);
        for host in &mut hosts {
            host.terminal_scheme = Some("nord".into());
            host.terminal_font_size = Some(16);
        }
        assert_eq!(Mirror::render(&hosts, &profiles), plain);
    }
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `export PATH=$HOME/.cargo/bin:$PATH; cargo test -p linascp-core 2>&1 | tail -20`
Expected: compile errors `no field terminal_scheme on type Host`.

- [ ] **Step 3: Implement**

`crates/core/src/model.rs`: in `struct Host`, after `forwards`, add:

```rust
    /// Terminal color scheme override (`"auto"` or a `schemes.json` id); `None` = global setting.
    #[serde(default)]
    pub terminal_scheme: Option<String>,
    /// Terminal font size override in px; `None` = global setting.
    #[serde(default)]
    pub terminal_font_size: Option<u16>,
```

In `Host::new`, add `terminal_scheme: None, terminal_font_size: None,` after `forwards: Vec::new(),`.

`crates/core/src/store/migrations.rs`: below `V1`, add the following and change the list:

```rust
/// TP2a: per-host terminal scheme / font size overrides (NULL = global setting).
const V2: &str = r#"
ALTER TABLE hosts ADD COLUMN terminal_scheme TEXT;
ALTER TABLE hosts ADD COLUMN terminal_font_size INTEGER;
"#;

const MIGRATIONS: &[&str] = &[V1, V2];
```

`crates/core/src/store/mod.rs`:
- `HOST_COLUMNS`: append `, terminal_scheme, terminal_font_size` after `created_at, updated_at`.
- `upsert_host`:
  - Extend the `VALUES` list with `, ?24, ?25`.
  - Append `, terminal_scheme = excluded.terminal_scheme, terminal_font_size = excluded.terminal_font_size` to the
    `ON CONFLICT … DO UPDATE SET` list.
  - Append `host.terminal_scheme, host.terminal_font_size,` to `params![…]` after `now_unix()`.
- `host_from_row`: add `terminal_scheme: row.get(23)?, terminal_font_size: row.get(24)?,` after `forwards: Vec::new(),`.

`crates/core/src/validate.rs`: add `use std::ops::RangeInclusive;` at the top, then add:

```rust
/// Allowed terminal font sizes in px (`schemes.json` `fontSize.min/max`).
pub const TERMINAL_FONT_SIZES: RangeInclusive<u16> = 10..=20;

/// `"auto"` or a scheme id from `schemes.json`. The core only checks the format; the UI maps
/// ids it does not know (e.g. from a newer build) to `"auto"`.
pub fn is_valid_terminal_scheme(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphabetic())
}
```

In `validate_host`, add before the final `match &host.auth`:

```rust
    if let Some(scheme) = &host.terminal_scheme
        && !is_valid_terminal_scheme(scheme)
    {
        return Err(CoreError::config("Ungültiges Farbschema"));
    }
    if let Some(size) = host.terminal_font_size
        && !TERMINAL_FONT_SIZES.contains(&size)
    {
        return Err(CoreError::config("Schriftgröße muss zwischen 10 und 20 px liegen"));
    }
```

- [ ] **Step 4: Run the core tests**

Run: `cargo test -p linascp-core 2>&1 | grep -E "test result|FAILED|panicked"`
Expected: all `ok`.

- [ ] **Step 5: Regenerate the bindings and run the UI typecheck**

Run: `SHELL=/bin/sh cargo test -p linascp exports_bindings && bun run typecheck`
Expected: the typecheck errors list every `Host` literal that misses the new fields:
`ui/src/model/defaults.ts`, `ui/src/test/fixtures.ts` and `ui/src/screens/editor/drafts.ts`. If specta emitted the
fields as optional, there are no errors, but do Step 6 anyway.

- [ ] **Step 6: Write the failing UI test (Review Focus 1)**

In `ui/src/screens/editor/drafts.test.ts`, inside the `makeHost({...})` call that builds `host` for
"round-trips every field", add the overrides:

```ts
    terminalScheme: "nord",
    terminalFontSize: 16,
```

Run: `bun run test ui/src/screens/editor/drafts.test.ts`
Expected: FAIL in "round-trips every field" (the overrides are missing after `draftToHost`).

- [ ] **Step 7: Carry the fields through the drafts and literals**

`ui/src/model/defaults.ts` (`newHost`) and `ui/src/test/fixtures.ts` (`makeHost`): add
`terminalScheme: null, terminalFontSize: null,` after `forwards`.

`ui/src/screens/editor/drafts.ts`:
- `interface HostDraft`: after `createdAt: number;`, add:
  ```ts
  /** Terminal overrides are edited in the terminal sidebar, not here; carried through unchanged. */
  terminalScheme: string | null;
  terminalFontSize: number | null;
  ```
- `hostToDraft`: add `terminalScheme: host.terminalScheme ?? null, terminalFontSize: host.terminalFontSize ?? null,`
  after `createdAt: host.createdAt,`.
- `draftToHost`: add `terminalScheme: draft.terminalScheme, terminalFontSize: draft.terminalFontSize,` after
  `forwards: …,`.

Run: `bun run typecheck && bun run test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/core src-tauri ui/src
git commit -m "feat(core): per-host terminal scheme and font size overrides

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Settings `terminalScheme` and `terminalSidebarOpen`

**Files:**
- Modify: `crates/core/src/store/mod.rs` (setting keys)
- Modify: `src-tauri/src/dto.rs` (`Settings`, its default and serialization test)
- Modify: `src-tauri/src/commands/settings.rs`
- Regenerate: `ui/src/ipc/bindings.ts`
- Modify: `ui/src/stores/settings.ts` (`DEFAULT_SETTINGS`)

**Interfaces:**
- Consumes: `validate::TERMINAL_FONT_SIZES` and `validate::is_valid_terminal_scheme` (Task 1).
- Produces: TS `Settings.terminalScheme: string` (default `"auto"`) and `Settings.terminalSidebarOpen: boolean`
  (default `false`).

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/commands/settings.rs` `mod tests`:
- In `save_roundtrips_all_fields`, add `terminal_scheme: "nord".into(), terminal_sidebar_open: true,` to the
  `Settings { … }` literal.
- In `invalid_stored_values_fall_back_to_defaults`, add before `let s = …`:
  ```rust
  state.store.set_setting(TERMINAL_SCHEME, "../evil").unwrap();
  ```
  Add after the existing asserts:
  ```rust
  assert_eq!(s.terminal_scheme, "auto");
  state.store.set_setting(TERMINAL_FONT_SIZE, "9").unwrap();
  assert_eq!(load_settings(&state.store).unwrap().terminal_font_size, 14);
  ```
- Add:
  ```rust
  #[test]
  fn rejects_invalid_scheme_and_out_of_range_font_size() {
      let (_dir, state) = setup();
      let bad_scheme = Settings { terminal_scheme: "a b".into(), ..Settings::default() };
      assert!(matches!(save_settings(&state, bad_scheme), Err(CoreError::Config { .. })));
      let too_big = Settings { terminal_font_size: 21, ..Settings::default() };
      assert!(matches!(save_settings(&state, too_big), Err(CoreError::Config { .. })));
  }
  ```

In `src-tauri/src/dto.rs` `settings_serialize_camel_case`, extend the expected JSON with
`"terminalScheme": "auto", "terminalSidebarOpen": false`.

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `SHELL=/bin/sh cargo test -p linascp settings 2>&1 | tail -15`
Expected: compile errors (unknown fields and constant).

- [ ] **Step 3: Implement**

`crates/core/src/store/mod.rs`, next to the other keys:

```rust
/// Setting key: terminal color scheme id (`"auto"` or a `schemes.json` id).
pub const TERMINAL_SCHEME: &str = "terminal_scheme";
/// Setting key: `"1"` while the terminal sidebar is open.
pub const TERMINAL_SIDEBAR_OPEN: &str = "terminal_sidebar_open";
```

`src-tauri/src/dto.rs`:
- Add `pub terminal_scheme: String,` and `pub terminal_sidebar_open: bool,` to `Settings`, after
  `terminal_font_size`.
- In `Default`, add `terminal_scheme: "auto".to_string(), terminal_sidebar_open: false,`.
- `Settings` derives `Eq`, and `String` keeps that valid.

`src-tauri/src/commands/settings.rs`:
- Delete `const FONT_SIZE_RANGE` and the `RangeInclusive` import.
- Import `linascp_core::validate::{TERMINAL_FONT_SIZES, is_valid_terminal_scheme}`, and add `TERMINAL_SCHEME` and
  `TERMINAL_SIDEBAR_OPEN` to the store import.
- `load_settings`:
  - Replace `FONT_SIZE_RANGE` with `TERMINAL_FONT_SIZES`.
  - Add:
    ```rust
    terminal_scheme: store
        .get_setting(TERMINAL_SCHEME)?
        .filter(|id| is_valid_terminal_scheme(id))
        .unwrap_or(d.terminal_scheme),
    terminal_sidebar_open: parse_flag(store.get_setting(TERMINAL_SIDEBAR_OPEN)?).unwrap_or(d.terminal_sidebar_open),
    ```
- `save_settings`: replace the range check with:
  ```rust
  if !TERMINAL_FONT_SIZES.contains(&next.terminal_font_size) {
      return Err(CoreError::Config {
          message: format!("terminal font size must be between 10 and 20, got {}", next.terminal_font_size),
      });
  }
  if !is_valid_terminal_scheme(&next.terminal_scheme) {
      return Err(CoreError::Config { message: "invalid terminal color scheme id".to_string() });
  }
  ```
  Then persist both new values:
  ```rust
  store.set_setting(TERMINAL_SCHEME, &next.terminal_scheme)?;
  store.set_setting(TERMINAL_SIDEBAR_OPEN, flag_str(next.terminal_sidebar_open))?;
  ```

- [ ] **Step 4: Run the tests, regenerate the bindings and update the UI defaults**

Run: `SHELL=/bin/sh cargo test -p linascp 2>&1 | grep -E "test result|FAILED"`
Expected: all ok, and `bindings.ts` is regenerated by `exports_bindings`.

In `ui/src/stores/settings.ts` `DEFAULT_SETTINGS`, add `terminalScheme: "auto", terminalSidebarOpen: false,` after
`terminalFontSize`.

Run: `bun run typecheck && bun run test`
Expected: PASS. If a test builds a full `Settings` literal without `...DEFAULT_SETTINGS`, add the two fields there.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/store/mod.rs src-tauri ui/src
git commit -m "feat(settings): terminal color scheme and sidebar state

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `SessionInfo` from the connect pipeline (core)

**Files:**
- Modify: `crates/core/src/connect/auth.rs` (`authenticate` returns the methods used)
- Modify: `crates/core/src/connect/handler.rs` (record the host key and the negotiated names)
- Modify: `crates/core/src/connect/mod.rs` (`SessionInfo`, `handshake`, `establish`, `Established`)
- Modify: `crates/core/src/connect/ssh_session.rs` (`info` field and accessor)
- Test: `crates/core/tests/ssh_integration.rs`

**Interfaces:**
- Produces:
  - `linascp_core::connect::SessionInfo { host_key_algorithm, host_key_sha256, kex, cipher, mac: String, auth_methods: Vec<AuthMethod> }`.
    It derives `Debug, Clone, Default, PartialEq, Eq, Serialize, specta::Type`, uses camelCase and is re-exported
    from `connect`.
  - `SshSession::info(&self) -> &SessionInfo`.
  - `auth::authenticate(...) -> Result<Vec<AuthMethod>>`.

- [ ] **Step 1: Write the failing auth unit test**

In `crates/core/src/connect/auth.rs`, test `key_with_partial_success_continues_with_keyboard_interactive`, replace
`authenticate(&mut fake, &h.ctx(AuthMethod::Key, Some(key))).await.unwrap();` with:

```rust
        let used = authenticate(&mut fake, &h.ctx(AuthMethod::Key, Some(key))).await.unwrap();
        assert_eq!(used, vec![AuthMethod::Key, AuthMethod::KeyboardInteractive]);
```

In `password_from_store_needs_no_prompt`, replace the `authenticate(...)` line with:

```rust
        let used = authenticate(&mut fake, &h.ctx(AuthMethod::Password, None)).await.unwrap();
        assert_eq!(used, vec![AuthMethod::Password]);
```

- [ ] **Step 2: Run the test and verify that it fails**

Run: `cargo test -p linascp-core connect::auth 2>&1 | tail -5`
Expected: compile error `expected (), found Vec`, or a mismatched-types assert.

- [ ] **Step 3: Return the methods used**

In `auth.rs`, change the signature to `pub async fn authenticate(auth: &mut dyn Authenticator, ctx: &AuthContext<'_>) -> Result<Vec<AuthMethod>>`
and the `MethodOutcome::Done` arm to:

```rust
            MethodOutcome::Done => {
                ctx.log(format!("debug1: Authenticated as '{}'", ctx.user));
                return Ok(used);
            }
```

`used` already holds every method that succeeded, in order: each round pushes its method before running it, and only
`Done`/`Partial` continue.

In `connect/mod.rs`, change `Pipeline::authenticate` to return `Result<Vec<AuthMethod>>`. Its last line already
returns the value. In `connect_jump`, keep `self.authenticate(&mut handle, jump).await?;`, because the jump host's
methods are not shown.

Run: `cargo test -p linascp-core connect::auth`
Expected: PASS.

- [ ] **Step 4: Add `SessionInfo` and record into it in the handler**

In `crates/core/src/connect/mod.rs`, near `ConnectEvent`, add:

```rust
/// What was negotiated for an established connection; shown in the terminal sidebar.
/// Never add secrets (passwords, passphrases, key material, prompt answers) to this struct.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub host_key_algorithm: String,
    pub host_key_sha256: String,
    pub kex: String,
    pub cipher: String,
    /// Client→server MAC exactly as russh names it (a placeholder for AEAD ciphers).
    pub mac: String,
    /// Methods of the successful login, in order (two for 2FA).
    pub auth_methods: Vec<AuthMethod>,
}
```

Import `crate::model::AuthMethod` if it is not imported yet, and `std::sync::Mutex`. Then define a type alias:

```rust
/// Filled by the handler during the handshake (host key, negotiated algorithms).
pub(crate) type InfoSlot = Arc<Mutex<SessionInfo>>;
```

In `crates/core/src/connect/handler.rs`:
- Add `use std::sync::PoisonError;` and `use super::InfoSlot;`.
- Add the field `negotiated: InfoSlot` to `ClientHandler`, and add `negotiated: InfoSlot` as the last parameter of
  `ClientHandler::new`, stored in `Self { …, negotiated }`.
- In `verify`, directly after `let info = fingerprints(key);`, add:

```rust
        {
            let mut slot = self.negotiated.lock().unwrap_or_else(PoisonError::into_inner);
            slot.host_key_algorithm = info.algorithm.clone();
            slot.host_key_sha256 = info.sha256.clone();
        }
```

In `impl client::Handler for ClientHandler`, add:

```rust
    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        _session: &mut client::Session,
    ) -> std::result::Result<(), Self::Error> {
        let mut slot = self.negotiated.lock().unwrap_or_else(PoisonError::into_inner);
        slot.kex = names.kex.as_ref().to_string();
        slot.cipher = names.cipher.as_ref().to_string();
        slot.mac = names.client_mac.as_ref().to_string();
        Ok(())
    }
```

If the handler unit tests construct a `ClientHandler`, pass `Default::default()` (an `InfoSlot`).

- [ ] **Step 5: Thread the slot through the pipeline and into `SshSession`**

In `connect/mod.rs`:
- `handshake(&self, host: &Host, stream: Box<dyn Transport>, negotiated: InfoSlot)`: pass `negotiated` as the last
  argument of `ClientHandler::new`.
- `connect_jump`: `self.handshake(&jump.host, Box::new(stream), InfoSlot::default()).await?` (the jump info is not
  shown).
- `struct Established` gets the field `info: SessionInfo`.
- At the end of `establish`:

```rust
        let negotiated = InfoSlot::default();
        let mut handle = self.step(Step::HostKey, self.handshake(host, stream, negotiated.clone())).await?;
        let auth_methods = self.step(Step::Auth, self.authenticate(&mut handle, &req.target)).await?;
        let mut info = negotiated.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        info.auth_methods = auth_methods;
        Ok(Established { handle, jump, info })
```

- `open_shell`: `Ok(SshSession::start(channel, est.handle, est.jump, est.info, sink))`.

In `connect/ssh_session.rs`:
- Add the field `info: SessionInfo` to `SshSession` and import `super::SessionInfo`.
- `start(channel, handle, jump, info: SessionInfo, sink)` stores it.
- Add:

```rust
    /// Negotiated algorithms, host key and auth methods of this connection.
    pub fn info(&self) -> &SessionInfo {
        &self.info
    }
```

Re-export with `pub use` so that `linascp_core::connect::SessionInfo` resolves. It is defined in `connect/mod.rs`,
so it is already public there.

Run: `cargo test -p linascp-core 2>&1 | grep -E "test result|FAILED|error"`
Expected: all ok.

- [ ] **Step 6: Write the Docker integration assertions**

In `crates/core/tests/ssh_integration.rs`, test `unknown_host_key_trust_writes_known_hosts`, replace the first
`assert_shell(session.expect("connected"), &output).await;` with:

```rust
    let session = session.expect("connected");
    let info = session.info().clone();
    let Prompt::HostKeyUnknown { info: shown, .. } = &p.prompts()[0] else { panic!("expected host key prompt") };
    assert_eq!(info.host_key_sha256, shown.sha256);
    assert_eq!(info.host_key_algorithm, shown.algorithm);
    assert!(!info.kex.is_empty() && !info.cipher.is_empty() && !info.mac.is_empty(), "{info:?}");
    assert_eq!(info.auth_methods, vec![AuthMethod::Password]);
    assert_shell(session, &output).await;
```

In `key_then_keyboard_interactive_two_factor`, replace `assert_shell(session.expect("connected with key + keyboard-interactive"), &output).await;` with:

```rust
    let session = session.expect("connected with key + keyboard-interactive");
    assert_eq!(session.info().auth_methods, vec![AuthMethod::Key, AuthMethod::KeyboardInteractive]);
    assert_shell(session, &output).await;
```

- [ ] **Step 7: Run the Docker tests**

Run: `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1 2>&1 | grep -E "test result|FAILED|panicked"`
Expected: `16 passed`. Docker must be running. If `mac` turns out empty for AEAD ciphers, relax the assertion to
`kex`/`cipher` only and note it in the commit.

- [ ] **Step 8: Commit**

```bash
git add crates/core
git commit -m "feat(connect): collect negotiated algorithms, host key and auth methods per session

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `connect_ssh` returns `ConnectedSession`; tabs carry info, size and start time

**Files:**
- Modify: `src-tauri/src/dto.rs` (`ConnectedSession`)
- Modify: `src-tauri/src/commands/connect.rs`
- Regenerate: `ui/src/ipc/bindings.ts`
- Modify: `ui/src/ipc/api.ts`, `ui/src/stores/sessions.ts`, `ui/src/stores/connect.ts`, `ui/src/test/fixtures.ts`
- Modify: `ui/src/screens/terminal/TerminalView.tsx` (report the size to the tab)
- Test: `ui/src/stores/connect.test.ts`, `ui/src/stores/sessions.test.ts`, `ui/src/screens/terminal/TerminalView.test.tsx`

**Interfaces:**
- Consumes: `SessionInfo` and `SshSession::info()` (Task 3).
- Produces:
  - TS `ConnectedSession { sessionId: string; info: SessionInfo }`, with `connectSsh(...)` returning
    `Promise<ConnectedSession>`.
  - `Tab` gains `info?: SessionInfo; connectedAt?: number; cols?: number; rows?: number`.
  - `makeSessionInfo(overrides?)` is added to `ui/src/test/fixtures.ts`.

- [ ] **Step 1: Rust: return the info**

In `src-tauri/src/dto.rs`:

```rust
/// Result of `connect_ssh`: the registered session and what was negotiated for it.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedSession {
    pub session_id: SessionId,
    pub info: linascp_core::connect::SessionInfo,
}
```

Use the same `SessionId` type that `connect_ssh_registered` returns today.

In `src-tauri/src/commands/connect.rs`, change `connect_ssh_inner`, `connect_ssh_registered` and the
`#[tauri::command] connect_ssh` to return `Result<ConnectedSession>`. In the success arm:

```rust
        Ok(session) => {
            let info = session.info().clone();
            register(state, session_id, Arc::new(session), pump.clone());
            heal_if_already_exited(state, session_id, &pump);
            // core emits `Failed` on errors; the success event is ours (exactly one terminal event).
            emit(ConnectEvent::Connected { session_id });
            Ok(ConnectedSession { session_id, info })
        }
```

Update the src-tauri tests that use the result as a session id (around the `connect_ssh_inner`/`connect_ssh_registered`
calls in `mod tests`). Map them with `.map(|connected| connected.session_id)`, or match on `Err` as they do today.

Run: `SHELL=/bin/sh cargo test -p linascp 2>&1 | grep -E "test result|FAILED"`
Expected: all ok, and the bindings are regenerated.

- [ ] **Step 2: Write the failing UI tests**

`ui/src/test/fixtures.ts`:

```ts
export function makeSessionInfo(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    hostKeyAlgorithm: "ssh-ed25519",
    hostKeySha256: "SHA256:abc",
    kex: "curve25519-sha256",
    cipher: "chacha20-poly1305@openssh.com",
    mac: "none",
    authMethods: ["key"],
    ...overrides,
  };
}
```

Import `SessionInfo` from `../ipc/api`, and export it from `api.ts` (Step 3).

In `ui/src/stores/connect.test.ts`, replace every `connectSsh` mock that resolves a plain session id string `X` with
`{ sessionId: X, info: makeSessionInfo() }`. Find them with
`grep -n "connectSsh" ui/src/stores/connect.test.ts`. Then add a test next to the existing success test:

```ts
  it("stores the session info and the connect time on the new tab", async () => {
    vi.setSystemTime(new Date(2026, 9, 15, 14, 2));
    const info = makeSessionInfo({ authMethods: ["key", "keyboardInteractive"] });
    vi.mocked(api.connectSsh).mockResolvedValue({ sessionId: "s1", info });
    await useConnectStore.getState().start("h-web");
    expect(useSessionsStore.getState().tabs[0]).toMatchObject({ sessionId: "s1", info, connectedAt: new Date(2026, 9, 15, 14, 2).getTime() });
  });
```

Use the store's existing start/flush helpers from that file if `start` needs host seeding (copy the seeding from the
existing success test). Use `vi.useFakeTimers({ toFake: ["Date"] })` / `vi.useRealTimers()` around the test if the
file does not already fake timers.

In `ui/src/stores/sessions.test.ts`, extend the existing `openLocal` success test with:

```ts
    expect(useSessionsStore.getState().tabs[0]?.connectedAt).toEqual(expect.any(Number));
```

In `ui/src/screens/terminal/TerminalView.test.tsx`, add:

```ts
  it("reports the terminal size to the tab", () => {
    useSessionsStore.setState({ tabs: [tab], activeId: tab.id });
    render(<TerminalView tab={tab} visible />);
    expect(useSessionsStore.getState().tabs[0]).toMatchObject({ cols: 80, rows: 24 });
    act(() => term().resize.fire({ cols: 120, rows: 40 }));
    expect(useSessionsStore.getState().tabs[0]).toMatchObject({ cols: 120, rows: 40 });
  });
```

Import `useSessionsStore` from `../../stores/sessions`, and reset it in `beforeEach` with
`useSessionsStore.setState(useSessionsStore.getInitialState(), true);`.

Run: `bun run typecheck; bun run test ui/src/stores ui/src/screens/terminal`
Expected: FAIL (type errors and missing fields).

- [ ] **Step 3: Implement**

`ui/src/ipc/api.ts`:
- Add `ConnectedSession` and `SessionInfo` to both the `import type {…}` and the `export type {…}` lists.
- Change `connectSsh` to return `Promise<ConnectedSession>`. The body stays `unwrap(commands.connectSsh(...))`.

`ui/src/stores/sessions.ts`:
- `Tab` adds the following fields and imports `SessionInfo`:
  ```ts
  /** Negotiated algorithms etc. of an SSH session (sidebar "Sitzung"). */
  info?: SessionInfo;
  /** Client time (ms) when the session connected / the local terminal opened. */
  connectedAt?: number;
  /** Current terminal size, reported by `TerminalView`. */
  cols?: number;
  rows?: number;
  ```
- `openLocal`: `addTab({ …, status: "connected", connectedAt: Date.now() })`.

`ui/src/stores/connect.ts`:
- In `run`: `const { sessionId, info } = await connectSsh(…);`, then pass `info` into `onConnected`.
- `onConnected(flow, sessionId, info, tabId, stream)` adds `info, connectedAt: Date.now()` to the `addTab` call.
- The `!isCurrent` branch still closes `sessionId`.

`ui/src/screens/terminal/TerminalView.tsx`:
- Import `useSessionsStore`.
- Add `const reportSize = (cols: number, rows: number) => useSessionsStore.getState().updateTab(tab.id, { cols, rows });`.
  Call it inside the `onResize` subscription (next to `sessionResize`) and in the fit effect right after the initial
  `sessionResize(sessionId, term.cols, term.rows)`.
- Reading `tab.id` inside these closures is fine, because the terminal effect already depends on `tab.id`.

Run: `bun run typecheck && bun run test`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri ui/src
git commit -m "feat(connect): hand the session info to the UI; tabs track size and start time

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Scheme resolution and effective prefs in `TerminalView`

**Files:**
- Modify: `ui/src/screens/terminal/theme.ts`
- Create: `ui/src/screens/terminal/prefs.ts`
- Modify: `ui/src/screens/terminal/TerminalView.tsx`
- Modify: `ui/src/i18n/de.ts` (`t.terminal.design.auto`)
- Test: `ui/src/screens/terminal/terminalConfig.test.ts`, `ui/src/screens/terminal/prefs.test.ts` (new), `ui/src/screens/terminal/TerminalView.test.tsx`

**Interfaces:**
- Produces:
  - `theme.ts`:
    - `type Scheme` (exported)
    - `AUTO_SCHEME = "auto"`
    - `SCHEME_CHOICES: ReadonlyArray<{ id: string; label: string }>` ("Wie App-Design" first, then the 9 schemes in
      JSON order)
    - `isKnownScheme(id: string): boolean`
    - `resolveScheme(schemeId: string, appTheme: Theme): Scheme`
    - `terminalTheme(schemeId: string, appTheme: Theme): ITheme`
  - `prefs.ts`:
    - `effectivePrefs(host, settings): { scheme: string; fontSize: number }`
    - `useHost(hostId?: Id): Host | undefined`
    - `useTerminalPrefs(hostId?: Id)`

- [ ] **Step 1: Write the failing tests**

Replace the `terminalTheme` assertions in `ui/src/screens/terminal/terminalConfig.test.ts` with:

```ts
  it("maps every scheme to an opaque xterm theme; auto and unknown ids follow the app theme", () => {
    expect(terminalTheme("auto", "dark")).toMatchObject({ background: "#0B0E13", foreground: "#D4DAE2", cursor: "#4FD1C5", green: "#3DDC97" });
    expect(terminalTheme("auto", "light")).toMatchObject({ background: "#FFFFFF", foreground: "#2A313B", cursor: "#0E7C72" });
    expect(terminalTheme("solarizedNeon", "light")).toEqual(terminalTheme("auto", "light"));
    expect(terminalTheme("nord", "light")).toMatchObject({ background: "#2E3440", brightBlack: "#7B88A1", black: "#3B4252" });
    for (const { id } of SCHEME_CHOICES) {
      expect(terminalTheme(id, "dark").background).toMatch(/^#[0-9A-F]{6}$/);
    }
    expect(SCHEME_CHOICES.map((choice) => choice.id)).toEqual([
      "auto", "linascp", "linascpLight", "dracula", "nord", "gruvbox", "tokyo", "solDark", "solLight", "contrast",
    ]);
    expect(SCHEME_CHOICES[0]?.label).toBe("Wie App-Design");
    expect(isKnownScheme("nord")).toBe(true);
    expect(isKnownScheme("solarizedNeon")).toBe(false);
  });
```

Update the import to `import { FONT_SIZE_MAX, FONT_SIZE_MIN, isKnownScheme, SCHEME_CHOICES, TERMINAL_FONT_FAMILY, terminalTheme } from "./theme";`.

Create `ui/src/screens/terminal/prefs.test.ts`:

```ts
import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useHostsStore } from "../../stores/hosts";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../stores/settings";
import { makeHost } from "../../test/fixtures";
import { effectivePrefs, useTerminalPrefs } from "./prefs";

vi.mock("../../ipc/api");

const settings = { ...DEFAULT_SETTINGS, terminalScheme: "dracula", terminalFontSize: 13 };

beforeEach(() => {
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSettingsStore.setState({ settings, loaded: true });
});

describe("effectivePrefs", () => {
  it("prefers the host override per field and falls back to the global settings", () => {
    expect(effectivePrefs(undefined, settings)).toEqual({ scheme: "dracula", fontSize: 13 });
    expect(effectivePrefs(makeHost({ terminalScheme: "nord", terminalFontSize: null }), settings)).toEqual({ scheme: "nord", fontSize: 13 });
    expect(effectivePrefs(makeHost({ terminalScheme: null, terminalFontSize: 18 }), settings)).toEqual({ scheme: "dracula", fontSize: 18 });
  });

  it("uses the global values for local tabs and deleted hosts", () => {
    useHostsStore.setState({ hosts: [makeHost({ id: "h1", terminalScheme: "nord", terminalFontSize: 18 })] });
    expect(renderHook(() => useTerminalPrefs("h1")).result.current).toEqual({ scheme: "nord", fontSize: 18 });
    expect(renderHook(() => useTerminalPrefs(undefined)).result.current).toEqual({ scheme: "dracula", fontSize: 13 });
    expect(renderHook(() => useTerminalPrefs("gone")).result.current).toEqual({ scheme: "dracula", fontSize: 13 });
  });
});
```

In `ui/src/screens/terminal/TerminalView.test.tsx`:
- Add `useHostsStore.setState(useHostsStore.getInitialState(), true);` to `beforeEach`, and import `useHostsStore`
  and `makeHost`.
- Add:

```ts
  it("applies the host's scheme and font size override and follows changes", () => {
    useHostsStore.setState({ hosts: [makeHost({ id: "h", terminalScheme: "nord", terminalFontSize: 16 })] });
    const { container } = render(<TerminalView tab={tab} visible />);
    expect(term().options).toMatchObject({ fontSize: 16, theme: { background: "#2E3440" } });
    expect((container.querySelector("[data-terminal]") as HTMLElement).style.background).toBe("rgb(46, 52, 64)");
    act(() => useHostsStore.setState({ hosts: [makeHost({ id: "h", terminalScheme: null, terminalFontSize: null })] }));
    expect(term().options).toMatchObject({ fontSize: 14, theme: { background: "#0B0E13" } });
  });
```

Run: `bun run test ui/src/screens/terminal`
Expected: FAIL (missing exports, and the override is ignored).

- [ ] **Step 2: Implement `theme.ts`**

Replace the scheme part of `ui/src/screens/terminal/theme.ts`:

```ts
import type { ITheme } from "@xterm/xterm";
import { t } from "../../i18n/de";
import type { Theme } from "../../ipc/api";
import schemes from "./schemes.json";

export interface Scheme {
  name: string;
  bg: string;
  fg: string;
  dim: string;
  green: string;
  blue: string;
  yellow: string;
  red: string;
  cursor: string;
  bd: string;
}

const SCHEMES: Readonly<Record<string, Scheme>> = schemes.schemes;

/** "Wie App-Design": dark → `linascp`, light → `linascpLight`. */
export const AUTO_SCHEME = "auto";

/** Picker entries: "Wie App-Design" first, then the schemes in `schemes.json` order. */
export const SCHEME_CHOICES: ReadonlyArray<{ id: string; label: string }> = [
  { id: AUTO_SCHEME, label: t.terminal.design.auto },
  ...Object.entries(SCHEMES).map(([id, scheme]) => ({ id, label: scheme.name })),
];

export function isKnownScheme(id: string): boolean {
  return Object.hasOwn(SCHEMES, id);
}

/** Unknown ids (e.g. stored by a newer build) behave like "auto". */
export function resolveScheme(schemeId: string, appTheme: Theme): Scheme {
  const own = isKnownScheme(schemeId) ? SCHEMES[schemeId] : undefined;
  return own ?? (appTheme === "light" ? schemes.schemes.linascpLight : schemes.schemes.linascp);
}

export const TERMINAL_FONT_FAMILY = '"JetBrains Mono", monospace';
export const FONT_SIZE_MIN: number = schemes.fontSize.min;
export const FONT_SIZE_MAX: number = schemes.fontSize.max;

/** xterm theme with an opaque background (transparency causes artifacts in WebKitGTK). */
// ponytail: schemes define 9 colors; magenta/cyan stay xterm defaults, bright = normal.
export function terminalTheme(schemeId: string, appTheme: Theme): ITheme {
  const s = resolveScheme(schemeId, appTheme);
  return {
    background: s.bg,
    foreground: s.fg,
    cursor: s.cursor,
    cursorAccent: s.bg,
    selectionBackground: `${s.cursor}55`,
    black: s.bd,
    brightBlack: s.dim,
    red: s.red,
    brightRed: s.red,
    green: s.green,
    brightGreen: s.green,
    yellow: s.yellow,
    brightYellow: s.yellow,
    blue: s.blue,
    brightBlue: s.blue,
    white: s.fg,
    brightWhite: s.fg,
  };
}
```

In `ui/src/i18n/de.ts`, add a nested `design` block inside `terminal: { … }`, starting with `auto` only. Task 7
extends it:

```ts
    design: {
      auto: "Wie App-Design",
    },
```

- [ ] **Step 3: Implement `prefs.ts`**

```ts
import type { Host, Id, Settings } from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { useSettingsStore } from "../../stores/settings";

type HostPrefs = Pick<Host, "terminalScheme" | "terminalFontSize">;
type GlobalPrefs = Pick<Settings, "terminalScheme" | "terminalFontSize">;

/** Host override per field, else the global setting. */
export function effectivePrefs(host: HostPrefs | undefined, settings: GlobalPrefs): { scheme: string; fontSize: number } {
  return {
    scheme: host?.terminalScheme ?? settings.terminalScheme,
    fontSize: host?.terminalFontSize ?? settings.terminalFontSize,
  };
}

/** The tab's host, or `undefined` for local tabs and deleted hosts. */
export function useHost(hostId: Id | undefined): Host | undefined {
  return useHostsStore((state) => (hostId === undefined ? undefined : state.hosts.find((host) => host.id === hostId)));
}

export function useTerminalPrefs(hostId: Id | undefined): { scheme: string; fontSize: number } {
  const host = useHost(hostId);
  const settings = useSettingsStore((state) => state.settings);
  return effectivePrefs(host, settings);
}
```

If specta emitted the host fields as optional (`?:`), the `??` already covers `undefined`.

- [ ] **Step 4: Use the prefs in `TerminalView.tsx`**

- Replace `const theme = useUiStore(...)` and `const fontSize = useSettingsStore(...terminalFontSize)` with:
  ```ts
  const appTheme = useUiStore((state) => state.theme);
  const { scheme, fontSize } = useTerminalPrefs(tab.hostId);
  ```
- `initial.current = { scheme, appTheme, fontSize }`. The constructor uses
  `theme: terminalTheme(initial.current.scheme, initial.current.appTheme)`.
- The theme effect becomes:
  ```ts
  useEffect(() => {
    const term = termRef.current;
    if (term) term.options.theme = terminalTheme(scheme, appTheme);
  }, [scheme, appTheme, sessionId]);
  ```
- The frame background is `terminalTheme(scheme, appTheme).background`.
- Remove the `useSettingsStore` import only if it is unused now. It is still used for `renderer`.

Run: `bun run typecheck && bun run test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src
git commit -m "feat(terminal): selectable color schemes with per-host overrides

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Sidebar tab "Sitzung" (`SessionPanel`)

**Files:**
- Create: `ui/src/screens/terminal/sidebar/SessionPanel.tsx`
- Create: `ui/src/screens/terminal/sidebar/Sidebar.module.css` (shared by all sidebar parts)
- Create: `ui/src/screens/terminal/sidebar/format.ts`
- Modify: `ui/src/i18n/de.ts`
- Test: `ui/src/screens/terminal/sidebar/SessionPanel.test.tsx`, `ui/src/screens/terminal/sidebar/format.test.ts`

**Interfaces:**
- Consumes:
  - `Tab.info`, `Tab.connectedAt`, `Tab.cols` and `Tab.rows` (Task 4)
  - `useHost` (Task 5)
  - `copyText` from `../clipboard`
- Produces:
  - `SessionPanel({ tab }: { tab: Tab })`
  - `formatSince(since: number, now: number): string`
  - CSS classes `list`, `term`, `value`, `mono`, `hint`, `row` in `Sidebar.module.css`

- [ ] **Step 1: Write the failing tests**

`ui/src/screens/terminal/sidebar/format.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { formatSince } from "./format";

describe("formatSince", () => {
  it("shows the local clock time and the elapsed time", () => {
    const since = new Date(2026, 9, 15, 14, 2).getTime();
    expect(formatSince(since, since + 30_000)).toBe("14:02 (0 Min.)");
    expect(formatSince(since, since + (2 * 60 + 14) * 60_000)).toBe("14:02 (2 Std. 14 Min.)");
    expect(formatSince(since, since - 5_000)).toBe("14:02 (0 Min.)");
  });
});
```

`ui/src/screens/terminal/sidebar/SessionPanel.test.tsx`:

```tsx
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { makeHost, makeProfile, makeSessionInfo, withUsage } from "../../../test/fixtures";
import { SessionPanel } from "./SessionPanel";

vi.mock("../../../ipc/api");
const clipboard = vi.hoisted(() => ({ writeText: vi.fn(async (_text: string) => undefined), readText: vi.fn(async () => "") }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => clipboard);

const since = new Date(2026, 9, 15, 14, 2).getTime();
const sshTab: Tab = {
  id: "t1", title: "web-prod-01", kind: "ssh", hostId: "h-web", sessionId: "s1", status: "connected",
  info: makeSessionInfo({ authMethods: ["key", "keyboardInteractive"] }), connectedAt: since, cols: 120, rows: 40,
};

function value(term: string): string {
  const row = screen.getByText(term, { selector: "dt" }).parentElement as HTMLElement;
  return within(row).getByRole("definition").textContent ?? "";
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(since + 75 * 60_000);
  const profile = makeProfile({ id: "p1", name: "deploy" });
  useHostsStore.setState({
    hosts: [
      makeHost({ id: "h-web", user: "deploy", address: "10.0.1.12", port: 2222, jumpHostId: "h-bastion", keepaliveS: 30, auth: { kind: "profile", profileId: "p1" } }),
      makeHost({ id: "h-bastion", name: "bastion" }),
    ],
    profiles: [withUsage(profile, 1)],
  });
});

afterEach(() => vi.useRealTimers());

describe("SessionPanel", () => {
  it("lists host, login and negotiated algorithms of an SSH session", () => {
    render(<SessionPanel tab={sshTab} />);
    expect(value("Host")).toBe("deploy@10.0.1.12:2222");
    expect(value("Jump-Host")).toBe("bastion");
    expect(value("Anmeldung")).toBe("Schlüssel + Interaktiv / 2FA · Profil deploy");
    expect(value("Host-Key")).toContain("ssh-ed25519 SHA256:abc");
    expect(value("Schlüsselaustausch")).toBe("curve25519-sha256");
    expect(value("Verschlüsselung")).toBe("chacha20-poly1305@openssh.com · none");
    expect(value("Terminal")).toBe("xterm-256color · 120×40");
    expect(value("Keepalive")).toBe("alle 30 s");
    expect(value("Verbunden seit")).toBe("14:02 (1 Std. 15 Min.)");
  });

  it("copies the fingerprint", async () => {
    vi.useRealTimers();
    render(<SessionPanel tab={sshTab} />);
    await userEvent.click(screen.getByRole("button", { name: "Fingerprint kopieren" }));
    expect(clipboard.writeText).toHaveBeenCalledWith("SHA256:abc");
  });

  it("shows dashes for missing values, a deleted host and an ended session", () => {
    const tab: Tab = { ...sshTab, hostId: "gone", status: "closed", info: makeSessionInfo({ kex: "", cipher: "", hostKeySha256: "" }) };
    render(<SessionPanel tab={tab} />);
    expect(value("Host")).toBe("—");
    expect(value("Schlüsselaustausch")).toBe("—");
    expect(value("Verschlüsselung")).toBe("—");
    expect(value("Host-Key")).toBe("—");
    expect(screen.queryByRole("button", { name: "Fingerprint kopieren" })).toBeNull();
    expect(value("Verbunden seit")).toBe("getrennt");
  });

  it("shows only shell, size and start time for local terminals", () => {
    const tab: Tab = { id: "t2", title: "Lokales Terminal", kind: "local", sessionId: "s2", status: "connected", connectedAt: since, cols: 80, rows: 24 };
    render(<SessionPanel tab={tab} />);
    expect(value("Typ")).toBe("Lokale Shell");
    expect(value("Terminal")).toBe("xterm-256color · 80×24");
    expect(value("Geöffnet seit")).toBe("14:02 (1 Std. 15 Min.)");
    expect(screen.queryByText("Host-Key")).toBeNull();
  });
});
```

Add `afterEach` to the vitest import.

Run: `bun run test ui/src/screens/terminal/sidebar`
Expected: FAIL (modules missing).

- [ ] **Step 2: Add the strings**

In `ui/src/i18n/de.ts`, inside `terminal: { … }`, add:

```ts
    sidebar: {
      label: "Sitzungsdetails",
      toggle: "Seitenleiste ein-/ausblenden (Ctrl+Shift+B)",
      session: "Sitzung",
      design: "Design",
      tunnel: "Tunnel",
      tunnelLater: "Tunnel folgen in einer späteren Version.",
    },
    info: {
      host: "Host",
      jump: "Jump-Host",
      login: "Anmeldung",
      profile: (name: string) => `Profil ${name}`,
      hostKey: "Host-Key",
      copyFingerprint: "Fingerprint kopieren",
      kex: "Schlüsselaustausch",
      cipher: "Verschlüsselung",
      terminal: "Terminal",
      keepalive: "Keepalive",
      keepaliveEvery: (seconds: number) => `alle ${seconds} s`,
      keepaliveOff: "aus",
      connectedSince: "Verbunden seit",
      openedSince: "Geöffnet seit",
      disconnected: "getrennt",
      type: "Typ",
      localShell: "Lokale Shell",
      none: "—",
      duration: (hours: number, minutes: number) => (hours > 0 ? `${hours} Std. ${minutes} Min.` : `${minutes} Min.`),
    },
```

- [ ] **Step 3: Implement `format.ts`**

```ts
import { t } from "../../../i18n/de";

/** "14:02 (2 Std. 14 Min.)": local clock time of `since` plus the elapsed time until `now`. */
export function formatSince(since: number, now: number): string {
  const clock = new Date(since).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  const minutes = Math.max(0, Math.floor((now - since) / 60_000));
  return `${clock} (${t.terminal.info.duration(Math.floor(minutes / 60), minutes % 60)})`;
}
```

- [ ] **Step 4: Implement `SessionPanel.tsx` and the CSS**

```tsx
import { Copy } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { IconButton } from "../../../components";
import { t } from "../../../i18n/de";
import type { AuthMethod } from "../../../ipc/api";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { copyText } from "../clipboard";
import { useHost } from "../prefs";
import { formatSince } from "./format";
import styles from "./Sidebar.module.css";

const AUTH_LABELS: Record<AuthMethod, string> = {
  password: t.auth.password,
  key: t.auth.key,
  agent: t.auth.agent,
  keyboardInteractive: t.auth.keyboardInteractive,
};

const or = (value: string | null | undefined) => (value ? value : t.terminal.info.none);

function Row({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className={styles.row}>
      <dt className={styles.term}>{term}</dt>
      <dd className={styles.value}>{children}</dd>
    </div>
  );
}

/** Re-renders once a minute so "Verbunden seit" stays current. */
function useMinuteClock(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

/** Sidebar tab "Sitzung": connection facts of the active tab. */
export function SessionPanel({ tab }: { tab: Tab }) {
  const host = useHost(tab.hostId);
  const hosts = useHostsStore((state) => state.hosts);
  const profiles = useHostsStore((state) => state.profiles);
  const now = useMinuteClock();
  const ended = tab.status === "closed" || tab.status === "error";
  const size = tab.cols && tab.rows ? `xterm-256color · ${tab.cols}×${tab.rows}` : "xterm-256color";
  const since = ended ? t.terminal.info.disconnected : tab.connectedAt ? formatSince(tab.connectedAt, now) : t.terminal.info.none;

  if (tab.kind === "local") {
    return (
      <dl className={styles.list}>
        <Row term={t.terminal.info.type}>{t.terminal.info.localShell}</Row>
        <Row term={t.terminal.info.terminal}>{size}</Row>
        <Row term={t.terminal.info.openedSince}>{since}</Row>
      </dl>
    );
  }

  const profile = host?.auth.kind === "profile" ? profiles.find((p) => p.profile.id === host.auth.profileId)?.profile : undefined;
  const user = host?.auth.kind === "profile" ? profile?.user : host?.user;
  const address = host ? `${user ? `${user}@` : ""}${host.address}:${host.port}` : null;
  const jump = host?.jumpHostId ? hosts.find((h) => h.id === host.jumpHostId)?.name : null;
  const info = tab.info;
  const methods = info?.authMethods.map((method) => AUTH_LABELS[method]).join(" + ");
  const login = methods ? (profile ? `${methods} · ${t.terminal.info.profile(profile.name)}` : methods) : null;
  const keepalive = host ? (host.keepaliveS > 0 ? t.terminal.info.keepaliveEvery(host.keepaliveS) : t.terminal.info.keepaliveOff) : null;

  return (
    <dl className={styles.list}>
      <Row term={t.terminal.info.host}>
        <span className={styles.mono}>{or(address)}</span>
      </Row>
      <Row term={t.terminal.info.jump}>{or(jump)}</Row>
      <Row term={t.terminal.info.login}>{or(login)}</Row>
      <Row term={t.terminal.info.hostKey}>
        {info?.hostKeySha256 ? (
          <>
            <span className={styles.mono}>{`${info.hostKeyAlgorithm} ${info.hostKeySha256}`}</span>
            <IconButton size="sm" label={t.terminal.info.copyFingerprint} icon={Copy} onClick={() => void copyText(info.hostKeySha256)} />
          </>
        ) : (
          t.terminal.info.none
        )}
      </Row>
      <Row term={t.terminal.info.kex}>
        <span className={styles.mono}>{or(info?.kex)}</span>
      </Row>
      <Row term={t.terminal.info.cipher}>
        <span className={styles.mono}>{info?.cipher ? `${info.cipher} · ${or(info.mac)}` : t.terminal.info.none}</span>
      </Row>
      <Row term={t.terminal.info.terminal}>{size}</Row>
      <Row term={t.terminal.info.keepalive}>{or(keepalive)}</Row>
      <Row term={t.terminal.info.connectedSince}>{since}</Row>
    </dl>
  );
}
```

`ui/src/screens/terminal/sidebar/Sidebar.module.css`:

```css
.sidebar {
  display: flex;
  flex: 0 0 280px;
  flex-direction: column;
  gap: 12px;
  width: 280px;
  min-height: 0;
  padding: 12px;
  overflow-y: auto;
  border-left: 1px solid var(--bd);
  background: var(--s1);
}

.list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 0;
}

.row {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.term {
  color: var(--muted);
  font-size: 12px;
}

.value {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  color: var(--text);
  font-size: 13px;
  overflow-wrap: anywhere;
}

.mono {
  font-family: "JetBrains Mono", monospace;
  font-size: 12px;
}

.hint {
  margin: 0;
  color: var(--muted2);
  font-size: 12px;
}
```

Run: `bun run typecheck && bun run test ui/src/screens/terminal/sidebar`
Expected: PASS. If `toLocaleTimeString("de-DE")` yields a different separator in jsdom, keep the test and build the
clock with `String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0")` instead.

- [ ] **Step 5: Commit**

```bash
git add ui/src
git commit -m "feat(terminal): session info panel for the sidebar

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Sidebar tab "Design" (`DesignPanel`)

**Files:**
- Create: `ui/src/screens/terminal/sidebar/DesignPanel.tsx`
- Modify: `ui/src/screens/terminal/sidebar/Sidebar.module.css`
- Modify: `ui/src/i18n/de.ts` (`t.terminal.design.*`)
- Test: `ui/src/screens/terminal/sidebar/DesignPanel.test.tsx`

**Interfaces:**
- Consumes:
  - `SCHEME_CHOICES`, `isKnownScheme`, `resolveScheme`, `FONT_SIZE_MIN`, `FONT_SIZE_MAX` (Task 5)
  - `effectivePrefs`, `useHost` (Task 5)
  - `useHostsStore().saveHost`, `useSettingsStore().save`
- Produces: `DesignPanel({ tab }: { tab: Tab })`. It must be rendered inside `ToastProvider` (true in the app).

- [ ] **Step 1: Write the failing tests**

`ui/src/screens/terminal/sidebar/DesignPanel.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../../components";
import * as api from "../../../ipc/api";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { DEFAULT_SETTINGS, useSettingsStore } from "../../../stores/settings";
import { makeHost } from "../../../test/fixtures";
import { DesignPanel } from "./DesignPanel";

vi.mock("../../../ipc/api");

const sshTab: Tab = { id: "t1", title: "web", kind: "ssh", hostId: "h1", sessionId: "s1", status: "connected" };
const localTab: Tab = { id: "t2", title: "Lokal", kind: "local", sessionId: "s2", status: "connected" };

function renderPanel(tab: Tab) {
  return render(
    <ToastProvider>
      <DesignPanel tab={tab} />
    </ToastProvider>,
  );
}

function seedHost(overrides: Partial<api.Host> = {}) {
  useHostsStore.setState({ hosts: [makeHost({ id: "h1", terminalScheme: null, terminalFontSize: null, ...overrides })] });
}

beforeEach(() => {
  useHostsStore.setState(useHostsStore.getInitialState(), true);
  useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalFontSize: 14 }, loaded: true });
  vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
  vi.mocked(api.hostSave).mockImplementation(async (host) => host);
  seedHost();
});

describe("DesignPanel", () => {
  it("changes the global scheme and font size while 'Alle Sitzungen' is selected", async () => {
    renderPanel(sshTab);
    expect(screen.getByRole("radio", { name: "Alle Sitzungen" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: /Wie App-Design/ })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: /Nord/ }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalScheme: "nord" }));
    await userEvent.click(screen.getByRole("button", { name: "Schrift größer" }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalFontSize: 15 }));
    expect(api.hostSave).not.toHaveBeenCalled();
  });

  it("copies the effective values to the host for 'Nur diesen Host' and edits the host from then on", async () => {
    useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalScheme: "dracula", terminalFontSize: 13 }, loaded: true });
    renderPanel(sshTab);
    await userEvent.click(screen.getByRole("radio", { name: "Nur diesen Host" }));
    expect(api.hostSave).toHaveBeenLastCalledWith(expect.objectContaining({ id: "h1", terminalScheme: "dracula", terminalFontSize: 13 }));
    expect(screen.getByRole("radio", { name: "Nur diesen Host" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: /Gruvbox/ }));
    expect(api.hostSave).toHaveBeenLastCalledWith(expect.objectContaining({ terminalScheme: "gruvbox", terminalFontSize: 13 }));
    expect(api.settingsSet).not.toHaveBeenCalled();
  });

  it("clears both host overrides when switching back to 'Alle Sitzungen'", async () => {
    seedHost({ terminalScheme: "nord", terminalFontSize: 18 });
    renderPanel(sshTab);
    expect(screen.getByRole("radio", { name: "Nur diesen Host" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("radio", { name: "Alle Sitzungen" }));
    expect(api.hostSave).toHaveBeenLastCalledWith(expect.objectContaining({ terminalScheme: null, terminalFontSize: null }));
  });

  it("disables the size buttons at the limits", () => {
    seedHost({ terminalScheme: null, terminalFontSize: 20 });
    renderPanel(sshTab);
    expect(screen.getByRole("button", { name: "Schrift größer" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Schrift kleiner" })).toBeEnabled();
  });

  it("disables 'Nur diesen Host' for local tabs, ssh-config hosts and deleted hosts and says why", () => {
    const { unmount } = renderPanel(localTab);
    expect(screen.getByRole("radio", { name: "Nur diesen Host" })).toBeDisabled();
    expect(screen.getByText("Lokale Terminals verwenden immer die globalen Einstellungen.")).toBeInTheDocument();
    unmount();
    seedHost({ source: "sshConfig" });
    const second = renderPanel(sshTab);
    expect(screen.getByText("Hosts aus ~/.ssh/config lassen sich hier nicht einzeln anpassen.")).toBeInTheDocument();
    second.unmount();
    renderPanel({ ...sshTab, hostId: "gone" });
    expect(screen.getByText("Der Host existiert nicht mehr.")).toBeInTheDocument();
  });

  it("shows an unknown stored scheme as 'Wie App-Design'", () => {
    useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, terminalScheme: "solarizedNeon" }, loaded: true });
    renderPanel(localTab);
    expect(screen.getByRole("radio", { name: /Wie App-Design/ })).toHaveAttribute("aria-checked", "true");
  });

  it("reports a failed save and keeps the stored value", async () => {
    vi.mocked(api.settingsSet).mockRejectedValue({ type: "io", message: "disk full" });
    renderPanel(localTab);
    await userEvent.click(screen.getByRole("radio", { name: /Nord/ }));
    expect(await screen.findByText("Speichern fehlgeschlagen")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Wie App-Design/ })).toHaveAttribute("aria-checked", "true");
  });
});
```

Run: `bun run test ui/src/screens/terminal/sidebar/DesignPanel.test.tsx`
Expected: FAIL (module missing).

- [ ] **Step 2: Add the strings**

Extend `t.terminal.design` in `ui/src/i18n/de.ts`:

```ts
    design: {
      auto: "Wie App-Design",
      scheme: "Farbschema",
      fontSize: "Schriftgröße",
      smaller: "Schrift kleiner",
      bigger: "Schrift größer",
      scope: "Gilt für",
      scopeAll: "Alle Sitzungen",
      scopeHost: "Nur diesen Host",
      scopeLocal: "Lokale Terminals verwenden immer die globalen Einstellungen.",
      scopeSshConfig: "Hosts aus ~/.ssh/config lassen sich hier nicht einzeln anpassen.",
      scopeDeleted: "Der Host existiert nicht mehr.",
    },
```

- [ ] **Step 3: Implement `DesignPanel.tsx`**

```tsx
import { Minus, Plus } from "lucide-react";
import { IconButton, Segmented, useToast } from "../../../components";
import { t } from "../../../i18n/de";
import { errorDetails } from "../../../ipc/errors";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { useSettingsStore } from "../../../stores/settings";
import { useUiStore } from "../../../stores/ui";
import { effectivePrefs, useHost } from "../prefs";
import { AUTO_SCHEME, FONT_SIZE_MAX, FONT_SIZE_MIN, isKnownScheme, resolveScheme, SCHEME_CHOICES } from "../theme";
import styles from "./Sidebar.module.css";

type Scope = "all" | "host";

/** Sidebar tab "Design": scheme + font size, globally or as an override of the tab's host. */
export function DesignPanel({ tab }: { tab: Tab }) {
  const host = useHost(tab.hostId);
  const saveHost = useHostsStore((state) => state.saveHost);
  const settings = useSettingsStore((state) => state.settings);
  const saveSettings = useSettingsStore((state) => state.save);
  const appTheme = useUiStore((state) => state.theme);
  const toast = useToast();
  const { scheme, fontSize } = effectivePrefs(host, settings);
  const selected = isKnownScheme(scheme) ? scheme : AUTO_SCHEME;
  const scope: Scope = host && (host.terminalScheme != null || host.terminalFontSize != null) ? "host" : "all";
  const blockedReason =
    tab.kind === "local"
      ? t.terminal.design.scopeLocal
      : !host
        ? t.terminal.design.scopeDeleted
        : host.source !== "linascp"
          ? t.terminal.design.scopeSshConfig
          : null;

  const fail = (error: unknown) => toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });

  const apply = (next: { scheme: string; fontSize: number }) => {
    if (scope === "host" && host) {
      saveHost({ ...host, terminalScheme: next.scheme, terminalFontSize: next.fontSize }).catch(fail);
    } else {
      saveSettings({ terminalScheme: next.scheme, terminalFontSize: next.fontSize }).catch(fail);
    }
  };

  const changeScope = (next: Scope) => {
    if (!host || next === scope) return;
    const keep = next === "host";
    saveHost({ ...host, terminalScheme: keep ? scheme : null, terminalFontSize: keep ? fontSize : null }).catch(fail);
  };

  return (
    <div className={styles.design}>
      <section className={styles.group} aria-label={t.terminal.design.scheme}>
        <h3 className={styles.heading}>{t.terminal.design.scheme}</h3>
        {/* ponytail: plain tabbable radios, no roving arrow keys; add when the list grows */}
        <div role="radiogroup" aria-label={t.terminal.design.scheme} className={styles.schemes}>
          {SCHEME_CHOICES.map((choice) => {
            const preview = resolveScheme(choice.id, appTheme);
            return (
              <button
                key={choice.id}
                type="button"
                role="radio"
                aria-checked={selected === choice.id}
                className={styles.scheme}
                onClick={() => apply({ scheme: choice.id, fontSize })}
              >
                <span className={styles.preview} style={{ background: preview.bg, color: preview.fg }} aria-hidden="true">
                  <span style={{ color: preview.green }}>user@host</span>:<span style={{ color: preview.blue }}>~</span>$ ls{" "}
                  <span style={{ color: preview.yellow }}>warn</span> <span style={{ color: preview.red }}>error</span>
                </span>
                <span className={styles.schemeName}>{choice.label}</span>
              </button>
            );
          })}
        </div>
      </section>
      <section className={styles.group} aria-label={t.terminal.design.fontSize}>
        <h3 className={styles.heading}>{t.terminal.design.fontSize}</h3>
        <div className={styles.fontRow}>
          <IconButton label={t.terminal.design.smaller} icon={Minus} disabled={fontSize <= FONT_SIZE_MIN} onClick={() => apply({ scheme, fontSize: fontSize - 1 })} />
          <span aria-live="polite">{t.settings.fontSizeOption(fontSize)}</span>
          <IconButton label={t.terminal.design.bigger} icon={Plus} disabled={fontSize >= FONT_SIZE_MAX} onClick={() => apply({ scheme, fontSize: fontSize + 1 })} />
        </div>
      </section>
      <section className={styles.group} aria-label={t.terminal.design.scope}>
        <h3 className={styles.heading}>{t.terminal.design.scope}</h3>
        <Segmented<Scope>
          label={t.terminal.design.scope}
          size="sm"
          value={scope}
          options={[
            { value: "all", label: t.terminal.design.scopeAll },
            { value: "host", label: t.terminal.design.scopeHost, disabled: blockedReason !== null },
          ]}
          onChange={changeScope}
        />
        {blockedReason ? <p className={styles.hint}>{blockedReason}</p> : null}
      </section>
    </div>
  );
}
```

Notes:
- In host scope, `apply` writes both fields, because "Nur diesen Host" pins both effective values.
- A failed `saveHost` changes nothing in the store, and a failed settings save reverts it (the store is optimistic).
  Either way the panel shows the stored value again.
- Check that `Segmented` renders `role="radio"` buttons with `aria-checked` and passes `disabled`. If it uses another
  pattern, adapt the test queries to its roles. Do not change the component.

Append the following to `Sidebar.module.css`:

```css
.design,
.group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.design {
  gap: 16px;
}

.heading {
  margin: 0;
  color: var(--muted);
  font-size: 12px;
  font-weight: 600;
}

.schemes {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.scheme {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 4px;
  border: 1px solid var(--bd);
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}

.scheme:hover {
  border-color: var(--bd-hover);
}

.scheme[aria-checked="true"] {
  border-color: var(--accent);
  box-shadow: 0 0 0 1px var(--accent);
}

.scheme:focus-visible {
  outline: 2px solid var(--accent-ring);
  outline-offset: 1px;
}

.preview {
  display: block;
  padding: 6px;
  overflow: hidden;
  border-radius: 6px;
  font-family: "JetBrains Mono", monospace;
  font-size: 10px;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.schemeName {
  padding: 0 2px;
}

.fontRow {
  display: flex;
  align-items: center;
  gap: 12px;
}
```

Run: `bun run typecheck && bun run test ui/src/screens/terminal/sidebar`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add ui/src
git commit -m "feat(terminal): design panel with scheme picker, font size and host scope

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Sidebar shell, toggle button, `Ctrl+Shift+B`, layout

**Files:**
- Create: `ui/src/screens/terminal/sidebar/Sidebar.tsx`
- Create: `ui/src/screens/terminal/sidebar/toggle.ts`
- Modify: `ui/src/screens/terminal/TerminalScreen.tsx`, `ui/src/screens/terminal/TerminalScreen.module.css`
- Modify: `ui/src/screens/terminal/TabBar.tsx`
- Modify: `ui/src/shell/useGlobalShortcuts.ts`
- Test: `ui/src/screens/terminal/TerminalScreen.test.tsx`, `ui/src/shell/useGlobalShortcuts.test.tsx`

**Interfaces:**
- Consumes: `SessionPanel` (Task 6), `DesignPanel` (Task 7), `Settings.terminalSidebarOpen` (Task 2).
- Produces:
  - `Sidebar({ tab }: { tab: Tab })`
  - `toggleTerminalSidebar(): void` (saves the flag; a failure becomes a notice)
  - `Shortcut` adds `"toggleSidebar"`

- [ ] **Step 1: Write the failing tests**

In `ui/src/screens/terminal/TerminalScreen.test.tsx`:
- Add imports for `* as api from "../../ipc/api"`, `DEFAULT_SETTINGS, useSettingsStore` and `ToastProvider`.
- Wrap renders in `<ToastProvider>` if the file does not do it yet. `DesignPanel` needs it, and
  `vi.mocked(api.settingsSet).mockImplementation(async (s) => s)` must be in place.
- Seed the settings in `beforeEach` with
  `useSettingsStore.setState({ settings: DEFAULT_SETTINGS, loaded: true });`.
- Add:

```tsx
  it("toggles the sidebar from the tab bar, remembers it and shows the active tab's session", async () => {
    vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
    seed([web, local], "a");
    render(<ToastProvider><TerminalScreen /></ToastProvider>);
    expect(screen.queryByRole("complementary", { name: "Sitzungsdetails" })).toBeNull();
    const toggle = screen.getByRole("button", { name: "Seitenleiste ein-/ausblenden (Ctrl+Shift+B)" });
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(toggle);
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalSidebarOpen: true }));
    const sidebar = screen.getByRole("complementary", { name: "Sitzungsdetails" });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(within(sidebar).getByRole("tab", { name: "Sitzung" })).toHaveAttribute("aria-selected", "true");
    expect(within(sidebar).getByRole("tab", { name: "Tunnel" })).toBeDisabled();
    await userEvent.click(within(sidebar).getByRole("tab", { name: "Design" }));
    expect(within(sidebar).getByRole("radiogroup", { name: "Farbschema" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Lokales Terminal" }));
    await userEvent.click(within(sidebar).getByRole("tab", { name: "Sitzung" }));
    expect(within(sidebar).getByText("Lokale Shell")).toBeInTheDocument();
  });
```

In `ui/src/shell/useGlobalShortcuts.test.tsx`, import `* as api` and the settings store, then add:

```tsx
  it("Ctrl+Shift+B toggles the terminal sidebar, also inside the terminal", async () => {
    vi.mocked(api.settingsSet).mockImplementation(async (settings) => settings);
    useSettingsStore.setState({ settings: DEFAULT_SETTINGS, loaded: true });
    render(<Harness />);
    fireEvent.keyDown(screen.getByLabelText("Terminal-Eingabe"), { key: "B", ctrlKey: true, shiftKey: true });
    await vi.waitFor(() => expect(useSettingsStore.getState().settings.terminalSidebarOpen).toBe(true));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalSidebarOpen: true }));
  });
```

Run: `bun run test ui/src/screens/terminal/TerminalScreen.test.tsx ui/src/shell/useGlobalShortcuts.test.tsx`
Expected: FAIL.

- [ ] **Step 2: Implement the toggle**

`ui/src/screens/terminal/sidebar/toggle.ts`:

```ts
import { t } from "../../../i18n/de";
import { errorDetails } from "../../../ipc/errors";
import { pushNotice } from "../../../stores/notices";
import { useSettingsStore } from "../../../stores/settings";

/** Opens/closes the terminal sidebar and persists the state; a failed save becomes a notice. */
export function toggleTerminalSidebar(): void {
  const { settings, save } = useSettingsStore.getState();
  save({ terminalSidebarOpen: !settings.terminalSidebarOpen }).catch((error: unknown) =>
    pushNotice({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) }),
  );
}
```

`ui/src/shell/useGlobalShortcuts.ts`:
- Change the type to `export type Shortcut = "search" | "newHost" | "newLocalTerminal" | "toggleSidebar" | "settings";`.
- Update the doc comment of `shortcutFor` to mention `Ctrl+Shift+B`.
- Replace the shift line with:
  ```ts
  if (event.shiftKey) {
    if (key === "t") return "newLocalTerminal";
    if (key === "b") return "toggleSidebar";
    return null;
  }
  ```
- Add `"toggleSidebar"` to `TERMINAL_SAFE`.
- Add the switch case `case "toggleSidebar": toggleTerminalSidebar(); break;` and import it from
  `../screens/terminal/sidebar/toggle`.

- [ ] **Step 3: Implement `Sidebar.tsx`**

```tsx
import { useId, useState } from "react";
import { type TabItem, tabId, tabPanelId, Tabs } from "../../../components";
import { t } from "../../../i18n/de";
import type { Tab } from "../../../stores/sessions";
import { DesignPanel } from "./DesignPanel";
import { SessionPanel } from "./SessionPanel";
import styles from "./Sidebar.module.css";

type SidebarTab = "session" | "design";

const TABS: readonly TabItem[] = [
  { id: "session", label: t.terminal.sidebar.session },
  { id: "design", label: t.terminal.sidebar.design },
  { id: "tunnel", label: t.terminal.sidebar.tunnel, disabled: true },
];

/** Right-hand terminal sidebar for the active tab (Snippets follow in TP2b, Tunnel in TP5). */
export function Sidebar({ tab }: { tab: Tab }) {
  const [current, setCurrent] = useState<SidebarTab>("session");
  const idBase = useId();
  return (
    <aside className={styles.sidebar} aria-label={t.terminal.sidebar.label}>
      <Tabs
        label={t.terminal.sidebar.label}
        tabs={TABS}
        value={current}
        onChange={(id) => setCurrent(id === "design" ? "design" : "session")}
        idBase={idBase}
      />
      <div role="tabpanel" id={tabPanelId(idBase, current)} aria-labelledby={tabId(idBase, current)}>
        {current === "session" ? <SessionPanel tab={tab} /> : <DesignPanel tab={tab} />}
      </div>
      <p className={styles.hint}>{t.terminal.sidebar.tunnelLater}</p>
    </aside>
  );
}
```

- [ ] **Step 4: Layout and tab-bar button**

`ui/src/screens/terminal/TerminalScreen.tsx`:
- Read `const sidebarOpen = useSettingsStore((state) => state.settings.terminalSidebarOpen);` and
  `const activeTab = tabs.find((tab) => tab.id === activeId);`.
- Wrap the existing `<div className={styles.panels}>…</div>` like this:

```tsx
          <div className={styles.body}>
            <div className={styles.panels}>{/* unchanged */}</div>
            {sidebarOpen && activeTab ? <Sidebar tab={activeTab} /> : null}
          </div>
```

`TerminalScreen.module.css`: add the following. The `ResizeObserver` in `TerminalView` refits the terminal when the
sidebar changes its width, and `onResize` sends the new size to the backend, so no extra code is needed.

```css
.body {
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
}
```

`ui/src/screens/terminal/TabBar.tsx`: after the "+" `IconButton`, add:

```tsx
      <IconButton
        label={t.terminal.sidebar.toggle}
        icon={PanelRight}
        aria-pressed={sidebarOpen}
        onClick={toggleTerminalSidebar}
      />
```

Use `const sidebarOpen = useSettingsStore((state) => state.settings.terminalSidebarOpen);`, and add the imports
`PanelRight` from `lucide-react`, `useSettingsStore` and `toggleTerminalSidebar`.

Run: `bun run typecheck && bun run test`
Expected: PASS, and the output is free of act() warnings.

- [ ] **Step 5: Commit**

```bash
git add ui/src
git commit -m "feat(terminal): collapsible sidebar with session and design tabs (Ctrl+Shift+B)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Color scheme in the settings dialog

**Files:**
- Modify: `ui/src/screens/settings/SettingsDialog.tsx`
- Test: `ui/src/screens/settings/SettingsDialog.test.tsx`

**Interfaces:**
- Consumes: `SCHEME_CHOICES`, `isKnownScheme`, `AUTO_SCHEME` (Task 5), `t.terminal.design.scheme` (Task 7).

- [ ] **Step 1: Write the failing test**

Extend "saves renderer, font size and compatibility rendering" in `SettingsDialog.test.tsx` with:

```ts
    expect(screen.getByLabelText("Farbschema")).toHaveValue("auto");
    await userEvent.selectOptions(screen.getByLabelText("Farbschema"), "nord");
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalScheme: "nord" }));
```

Run: `bun run test ui/src/screens/settings/SettingsDialog.test.tsx`
Expected: FAIL (`Unable to find a label with the text of: Farbschema`).

- [ ] **Step 2: Implement**

In `SettingsDialog.tsx`, import `{ AUTO_SCHEME, isKnownScheme, SCHEME_CHOICES }` from `../terminal/theme`, and add
at module level:

```ts
const SCHEME_OPTIONS = SCHEME_CHOICES.map((choice) => ({ value: choice.id, label: choice.label }));
```

In the terminal grid, after the font size `Select`, add:

```tsx
            <Select
              label={t.terminal.design.scheme}
              value={isKnownScheme(settings.terminalScheme) ? settings.terminalScheme : AUTO_SCHEME}
              options={SCHEME_OPTIONS}
              onChange={(terminalScheme) => persist({ terminalScheme })}
            />
```

Run: `bun run typecheck && bun run test`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add ui/src
git commit -m "feat(settings): terminal color scheme select

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Docs, full verification, manual check

**Files:**
- Create: `docs/acceptance-tp2a.md`
- Modify: `CLAUDE.md` (spec pointer under Layout)

- [ ] **Step 1: Write `docs/acceptance-tp2a.md`**

```markdown
# Manual acceptance: TP2a (terminal sidebar, color schemes, session info)

Run `export PATH="$HOME/.cargo/bin:$PATH"; bun run tauri dev`.

- [ ] Ctrl+Shift+B and the tab-bar button open/close the sidebar; the state survives an app restart.
- [ ] Toggling the sidebar refits the terminal (`tput cols` changes) without garbled output.
- [ ] Sitzung (SSH): host, jump host, login method(s) + profile, host key + copy, kex, cipher/MAC, size, keepalive,
      "Verbunden seit" ticks every minute; after `exit` it shows "getrennt".
- [ ] Sitzung (local): Lokale Shell, size, Geöffnet seit.
- [ ] Design: every scheme changes the open terminal immediately; A−/A+ stop at 10/20 px.
- [ ] "Nur diesen Host" on host A changes only A's tabs; a tab of host B keeps the global scheme; back to
      "Alle Sitzungen" restores the global look for A.
- [ ] Editing host A in the editor keeps its terminal override.
- [ ] Settings dialog: Farbschema select changes all tabs without an override.
- [ ] Export with an override, import into a fresh profile: the override is kept.
```

In `CLAUDE.md` under `## Layout`, add next to the TP1 spec bullet:
`- docs/superpowers/specs/2026-10-15-linascp-tp2a-sidebar-design.md: TP2a spec (terminal sidebar, color schemes, session info).`

- [ ] **Step 2: Run the full check**

```bash
export PATH=$HOME/.cargo/bin:$PATH
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
SHELL=/bin/sh cargo test --workspace 2>&1 | grep -E "test result|FAILED|warning"
cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1 2>&1 | grep -E "test result|FAILED"
bun run typecheck && bun run test 2>&1 | tail -5 && bun run build
git diff --exit-code ui/src/ipc/bindings.ts
```

Expected:
- Every step passes, with no warnings in the test output.
- The bindings are unchanged, because they were regenerated during the tasks.

- [ ] **Step 3: Commit**

```bash
git add docs CLAUDE.md
git commit -m "docs: TP2a acceptance checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 4: Hand over for the manual check.** Do not merge before the user has gone through
  `docs/acceptance-tp2a.md`.
