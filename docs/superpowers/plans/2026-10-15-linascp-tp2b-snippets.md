# LinaSCP TP2b (Snippets) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Saved commands (snippets), global or bound to one host. They can be inserted into or run in the active
terminal, may contain `{{name}}` placeholders, and are managed in the terminal sidebar and a "Snippets" screen.

**Architecture:**
- **Core:**
  - `Snippet` model and migration V3 (table `snippets`, `host_id` FK with `ON DELETE CASCADE`).
  - `validate_snippet` and store CRUD.
  - Export/import carries snippets through the same conflict plan as hosts (`ItemKind::Snippet`).
- **Tauri:** `snippets_list` / `snippet_save` / `snippet_delete`.
- **UI:**
  - `useSnippetsStore`.
  - A paste-target registry, so the sidebar can reach the active tab's xterm (`term.paste`, `term.input("\r")`).
  - Pure placeholder helpers and a placeholder dialog.
  - A shared editor modal, the sidebar panel "Snippets", and a rail screen "Snippets".

**Tech Stack:** Rust 2024 (rustc ≥ 1.88 via rustup: `export PATH=$HOME/.cargo/bin:$PATH`), rusqlite, tauri-specta;
React 19, zustand, xterm.js 6, Vitest + Testing Library, bun.

**Spec:** `docs/superpowers/specs/2026-10-15-linascp-tp2b-snippets-design.md`

## Global Constraints

- **Branch and commits:** work on branch `feat/tp2b-snippets`. It already exists and holds the spec. Every commit
  ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Language and naming:**
  - Code, comments and identifiers are in English.
  - UI strings are in German and live only in `ui/src/i18n/de.ts`.
  - The app name is "LinaSCP".
- **Snippet limits:**
  - The name is non-empty after `trim`, at most 64 characters, and contains no control characters.
  - The command is non-empty after `trim`, at most 8192 bytes, and contains no NUL byte. Line breaks and tabs are
    allowed.
- **Placeholders:**
  - Syntax is `{{name}}` with `name` matching `[A-Za-z0-9_-]{1,32}`. Anything else stays literal.
  - Values are inserted 1:1, are never re-resolved and are never stored.
- **Scope:**
  - `hostId: null` means the snippet is global.
  - Only stored hosts (`source === "linascp"`) can own snippets. Hosts that come from `~/.ssh/config` are not in the
    DB, so the FK would reject them.
- **Errors:**
  - Core validation errors are `CoreError::config` with a German message, as in the existing `validate.rs`.
  - The UI checks for an empty name or command at the field. It shows core errors as a red toast and keeps the dialog
    open (pattern of `ExportDialog`).
- **Tests:**
  - Test output stays free of warnings, including act() warnings.
  - src-tauri tests need `SHELL=/bin/sh`.
  - `bindings.ts` is generated only through `SHELL=/bin/sh cargo test -p linascp exports_bindings`.

## Review Focus

1. **Snippet for an ssh-config host:**
   - "+ Snippet anlegen" in a tab of a host from `~/.ssh/config` must not fail with an FK error.
   - The editor falls back to "Global" and does not list ssh-config hosts.
   - Test in Task 5.
2. **Running a multi-line snippet:**
   - The Enter must arrive after the pasted text, not before it or inside the bracketed paste.
   - `run` uses `term.input("\r")`, which goes through the same `onData → sessionWrite` queue as the paste.
   - Test in Task 4.
3. **Placeholder dialog focus trap:**
   - Text must be sent only after the dialog has closed. Otherwise the focus trap of `Dialog` pulls the focus back
     from the terminal.
   - Test in Task 6.
4. **Old export files:**
   - A `.linascp` file without a `snippets` key still imports.
   - A snippet whose host is not in the store and not in the file becomes global.
   - Test in Task 2.
5. **Deleting a host while the sidebar is open:**
   - Its snippets disappear from the list right away (cascade plus reload).
   - Test in Task 3.

---

### Task 1: Core model, validation, migration V3, store CRUD

**Files:**
- Modify: `crates/core/src/model.rs` (add `Snippet` after `Group`)
- Modify: `crates/core/src/validate.rs` (add `validate_snippet` and tests)
- Modify: `crates/core/src/store/migrations.rs` (V3)
- Modify: `crates/core/src/store/mod.rs` (snippet CRUD)
- Test: `crates/core/src/store/tests.rs`

**Interfaces:**
- Produces:
  - `model::Snippet { id: Id, name: String, command: String, host_id: Option<Id>, created_at: i64, updated_at: i64 }`
    (serde camelCase, specta).
  - `Snippet::new(name: &str, command: &str, host_id: Option<Id>) -> Snippet`.
  - `validate::validate_snippet(&Snippet) -> Result<()>`.
  - `Store::list_snippets() -> Result<Vec<Snippet>>`, sorted by name (NOCASE) and then id.
  - `Store::get_snippet(Id) -> Result<Option<Snippet>>`.
  - `Store::save_snippet(&Snippet) -> Result<()>`:
    - an upsert that keeps `created_at` and sets `updated_at = now`;
    - returns `Config` for an unknown `host_id`.
  - `Store::delete_snippet(Id) -> Result<()>`. An unknown id is fine.
  - Private helper `fn upsert_snippet(conn: &Connection, s: &Snippet) -> Result<()>`, also used by Task 2.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/core/src/validate.rs` (it already has `use super::*;`; add
`use crate::model::Snippet;` at the top of the module):

```rust
    fn snippet(name: &str, command: &str) -> Snippet {
        Snippet::new(name, command, None)
    }

    #[test]
    fn snippet_allows_multiline_commands_up_to_the_limits() {
        assert!(validate_snippet(&snippet("Deploy", "cd /srv\n\tgit pull\r\n")).is_ok());
        assert!(validate_snippet(&snippet(&"n".repeat(64), &"c".repeat(8192))).is_ok());
        assert!(validate_snippet(&snippet(&"ä".repeat(64), "x")).is_ok(), "64 characters, not bytes");
    }

    #[test]
    fn snippet_rejects_bad_names_and_commands() {
        let long_name = "n".repeat(65);
        let long_command = "c".repeat(8193);
        let cases = [
            ("", "ls"),
            ("   ", "ls"),
            (long_name.as_str(), "ls"),
            ("a\nb", "ls"),
            ("a\tb", "ls"),
            ("ok", ""),
            ("ok", " \n\t "),
            ("ok", long_command.as_str()),
            ("ok", "ls\0"),
        ];
        for (name, command) in cases {
            assert!(
                matches!(validate_snippet(&snippet(name, command)), Err(CoreError::Config { .. })),
                "{name:?} / {command:?}"
            );
        }
    }
```

Append to `crates/core/src/store/tests.rs` (add `Snippet` to the imports: `use crate::model::{EnvVar, SftpSettings, Snippet, TransferMode};`):

```rust
#[test]
fn snippet_roundtrip_keeps_created_at_and_sorts_by_name() {
    let store = Store::open_in_memory().unwrap();
    let mut beta = Snippet::new("beta", "echo b\necho c", None);
    beta.created_at = 1_000;
    beta.updated_at = 1_000;
    let alpha = Snippet::new("Alpha", "ls", None);
    store.save_snippet(&beta).unwrap();
    store.save_snippet(&alpha).unwrap();
    beta.command = "echo changed".into();
    beta.created_at = 5_000;
    store.save_snippet(&beta).unwrap();

    let names: Vec<String> = store.list_snippets().unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["Alpha", "beta"]);
    let loaded = store.get_snippet(beta.id).unwrap().unwrap();
    assert_eq!(loaded.command, "echo changed");
    assert_eq!(loaded.created_at, 1_000);
    assert!(loaded.updated_at >= now_unix() - 5);
}

#[test]
fn snippet_for_unknown_host_is_rejected() {
    let store = Store::open_in_memory().unwrap();
    let snippet = Snippet::new("x", "ls", Some(Id::new_v4()));
    assert!(matches!(store.save_snippet(&snippet), Err(CoreError::Config { .. })));
    assert!(store.list_snippets().unwrap().is_empty());
}

#[test]
fn deleting_a_host_deletes_only_its_snippets() {
    let store = Store::open_in_memory().unwrap();
    let host = Host::new("web", "10.0.0.5");
    store.save_host(&host).unwrap();
    let bound = Snippet::new("bound", "ls", Some(host.id));
    let global = Snippet::new("global", "ls", None);
    store.save_snippet(&bound).unwrap();
    store.save_snippet(&global).unwrap();

    store.delete_host(host.id).unwrap();
    assert_eq!(store.list_snippets().unwrap(), vec![store.get_snippet(global.id).unwrap().unwrap()]);

    store.delete_snippet(global.id).unwrap();
    store.delete_snippet(global.id).unwrap();
    assert!(store.list_snippets().unwrap().is_empty());
}
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `export PATH=$HOME/.cargo/bin:$PATH; cargo test -p linascp-core 2>&1 | tail -20`
Expected: compile errors `cannot find type Snippet` / `validate_snippet` / `save_snippet`.

- [ ] **Step 3: Implement**

`crates/core/src/model.rs`, after `impl Group { … }`:

```rust
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
```

`crates/core/src/validate.rs`:
- Change the import to `use crate::model::{AuthConfig, Host, HostAuth, LoginProfile, Snippet};`.
- Append after `validate_profile`:

```rust
const SNIPPET_NAME_MAX_CHARS: usize = 64;
const SNIPPET_COMMAND_MAX_BYTES: usize = 8192;

/// Snippets never reach ssh_config; the limits keep the UI and the terminal sane.
pub fn validate_snippet(snippet: &Snippet) -> Result<()> {
    if snippet.name.trim().is_empty() {
        return Err(CoreError::config("Name darf nicht leer sein"));
    }
    if snippet.name.chars().count() > SNIPPET_NAME_MAX_CHARS {
        return Err(CoreError::config("Name darf höchstens 64 Zeichen lang sein"));
    }
    if snippet.name.chars().any(char::is_control) {
        return Err(CoreError::config("Name darf keine Steuerzeichen enthalten"));
    }
    if snippet.command.trim().is_empty() {
        return Err(CoreError::config("Befehl darf nicht leer sein"));
    }
    if snippet.command.len() > SNIPPET_COMMAND_MAX_BYTES {
        return Err(CoreError::config("Befehl darf höchstens 8192 Bytes lang sein"));
    }
    if snippet.command.contains('\0') {
        return Err(CoreError::config("Befehl darf keine NUL-Zeichen enthalten"));
    }
    Ok(())
}
```

`crates/core/src/store/migrations.rs`: add V3 after V2 and extend the list.

```rust
/// TP2b: snippets, global (host_id NULL) or bound to one host.
const V3: &str = r#"
CREATE TABLE snippets (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    command     TEXT NOT NULL,
    host_id     TEXT REFERENCES hosts(id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
CREATE INDEX snippets_host_id ON snippets(host_id);
"#;

const MIGRATIONS: &[&str] = &[V1, V2, V3];
```

`crates/core/src/store/mod.rs`:
- Change the module doc to `//! SQLite persistence for hosts, login profiles, groups, snippets and settings.`
- Add `Snippet` to the `crate::model` import.
- Add after `PROFILE_COLUMNS`:

```rust
const SNIPPET_COLUMNS: &str = "id, name, command, host_id, created_at, updated_at";
```

Add this section inside `impl Store`, after `// ---- groups ----` and its methods:

```rust
    // ---- snippets ----

    pub fn list_snippets(&self) -> Result<Vec<Snippet>> {
        let conn = self.conn();
        let sql = format!("SELECT {SNIPPET_COLUMNS} FROM snippets ORDER BY name COLLATE NOCASE, id");
        let mut stmt = conn.prepare(&sql)?;
        let snippets = stmt.query_map([], snippet_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(snippets)
    }

    pub fn get_snippet(&self, id: Id) -> Result<Option<Snippet>> {
        let conn = self.conn();
        let sql = format!("SELECT {SNIPPET_COLUMNS} FROM snippets WHERE id = ?1");
        Ok(conn.query_row(&sql, [id.to_string()], snippet_from_row).optional()?)
    }

    /// Upserts the snippet (`created_at` of an existing row is kept, `updated_at` set to now).
    /// Fails with `Config` when `host_id` names no stored host.
    pub fn save_snippet(&self, s: &Snippet) -> Result<()> {
        let conn = self.conn();
        if let Some(host_id) = s.host_id {
            let known = conn
                .query_row("SELECT 1 FROM hosts WHERE id = ?1", [host_id.to_string()], |_| Ok(()))
                .optional()?
                .is_some();
            if !known {
                return Err(CoreError::config("Der Host des Snippets existiert nicht"));
            }
        }
        upsert_snippet(&conn, s)
    }

    pub fn delete_snippet(&self, id: Id) -> Result<()> {
        self.conn().execute("DELETE FROM snippets WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }
```

Add these free functions next to `upsert_setting` and `profile_from_row`:

```rust
fn upsert_snippet(conn: &Connection, s: &Snippet) -> Result<()> {
    conn.execute(
        "INSERT INTO snippets (id, name, command, host_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, command = excluded.command, \
         host_id = excluded.host_id, updated_at = excluded.updated_at",
        params![s.id.to_string(), s.name, s.command, s.host_id.map(|id| id.to_string()), s.created_at, now_unix()],
    )?;
    Ok(())
}

fn snippet_from_row(row: &Row<'_>) -> rusqlite::Result<Snippet> {
    Ok(Snippet {
        id: id_col(row, 0)?,
        name: row.get(1)?,
        command: row.get(2)?,
        host_id: opt_id_col(row, 3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}
```

- [ ] **Step 4: Run the core tests**

Run: `cargo test -p linascp-core 2>&1 | grep -E "test result|FAILED|panicked|warning"`
Expected: every `test result: ok`, no FAILED and no warnings. The existing migration test passes with
`LATEST_VERSION == 3`.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/model.rs crates/core/src/validate.rs crates/core/src/store
git commit -m "feat(core): snippet model, validation, migration V3 and store CRUD

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Export and import snippets

**Files:**
- Modify: `crates/core/src/export.rs`
- Modify: `crates/core/src/store/mod.rs` (`save_import` gains `snippets`)
- Test: `crates/core/src/export/tests.rs`

**Interfaces:**
- Consumes:
  - Task 1: `Snippet`, `validate_snippet`, `Store::list_snippets`, `Store::get_snippet`, `Store::save_snippet`, and
    `upsert_snippet`.
- Produces:
  - `ItemKind::Snippet`, which serializes as `"snippet"` and reaches the UI in Task 3.
  - `Store::save_import(groups, profiles, hosts, snippets: &[Snippet], settings)`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/core/src/export/tests.rs`, and add `Snippet` to `use crate::model::{…}`:

```rust
/// host "web" + snippet bound to it + global snippet; list order: "Nginx neu laden", "Speicher".
fn snippet_source() -> (Store, Host, Snippet, Snippet) {
    let store = Store::open_in_memory().unwrap();
    let host = Host::new("web", "10.0.0.5");
    store.save_host(&host).unwrap();
    let bound = Snippet::new("Nginx neu laden", "sudo nginx -t &&\nsudo systemctl reload nginx", Some(host.id));
    let global = Snippet::new("Speicher", "df -h", None);
    store.save_snippet(&bound).unwrap();
    store.save_snippet(&global).unwrap();
    (store, host, bound, global)
}

fn import_into(dst: &Store, bytes: &[u8], resolutions: &HashMap<Id, Resolution>) -> ImportReport {
    let tmp = tempfile::tempdir().unwrap();
    import_apply(bytes, None, resolutions, dst, &MemorySecretStore::new(), tmp.path()).unwrap()
}

fn export_json(store: &Store) -> serde_json::Value {
    serde_json::from_slice(&export_data(store, &MemorySecretStore::new(), &no_secrets()).unwrap().bytes).unwrap()
}

#[test]
fn snippets_roundtrip_with_their_host() {
    let (src, host, bound, global) = snippet_source();
    let bytes = export_data(&src, &MemorySecretStore::new(), &no_secrets()).unwrap().bytes;
    let dst = Store::open_in_memory().unwrap();

    let preview = import_preview(&bytes, &dst).unwrap();
    assert_eq!(preview.items.iter().filter(|i| i.kind == ItemKind::Snippet).count(), 2);
    let report = import_into(&dst, &bytes, &HashMap::new());

    assert_eq!(report, ImportReport { imported: 3, skipped: 0, secrets_restored: 0, invalid: 0 });
    let loaded = dst.get_snippet(bound.id).unwrap().unwrap();
    assert_eq!((loaded.command.as_str(), loaded.host_id), (bound.command.as_str(), Some(host.id)));
    assert_eq!(dst.get_snippet(global.id).unwrap().unwrap().host_id, None);
}

#[test]
fn files_without_snippets_still_import() {
    let src = source(None);
    let mut json = export_json(&src.store);
    json.as_object_mut().unwrap().remove("snippets");
    let dst = Store::open_in_memory().unwrap();

    let report = import_into(&dst, &serde_json::to_vec(&json).unwrap(), &HashMap::new());

    assert_eq!((report.imported, report.invalid), (3, 0));
    assert!(dst.list_snippets().unwrap().is_empty());
}

#[test]
fn snippet_of_a_host_that_is_not_imported_becomes_global() {
    let (src, _host, bound, _global) = snippet_source();
    let mut json = export_json(&src);
    json["hosts"] = serde_json::Value::Array(Vec::new());
    let dst = Store::open_in_memory().unwrap();

    import_into(&dst, &serde_json::to_vec(&json).unwrap(), &HashMap::new());

    assert_eq!(dst.get_snippet(bound.id).unwrap().unwrap().host_id, None);
}

#[test]
fn duplicated_host_takes_its_duplicated_snippets_along() {
    let (src, host, bound, _global) = snippet_source();
    let bytes = export_data(&src, &MemorySecretStore::new(), &no_secrets()).unwrap().bytes;
    let resolutions = HashMap::from([(host.id, Resolution::Duplicate), (bound.id, Resolution::Duplicate)]);

    import_into(&src, &bytes, &resolutions);

    let copy_host = src.list_hosts().unwrap().into_iter().find(|h| h.name == "web (Import)").unwrap();
    let copy = src.list_snippets().unwrap().into_iter().find(|s| s.name == "Nginx neu laden (Import)").unwrap();
    assert_eq!(copy.host_id, Some(copy_host.id));
    assert_eq!(src.get_snippet(bound.id).unwrap().unwrap().host_id, Some(host.id));
}

#[test]
fn snippet_conflicts_skip_by_default_and_replace_on_request() {
    let (src, _host, bound, _global) = snippet_source();
    let bytes = export_data(&src, &MemorySecretStore::new(), &no_secrets()).unwrap().bytes;
    let mut changed = bound.clone();
    changed.command = "changed".into();
    src.save_snippet(&changed).unwrap();

    let report = import_into(&src, &bytes, &HashMap::new());
    assert_eq!((report.imported, report.skipped), (0, 3));
    assert_eq!(src.get_snippet(bound.id).unwrap().unwrap().command, "changed");

    let report = import_into(&src, &bytes, &HashMap::from([(bound.id, Resolution::Replace)]));
    assert_eq!((report.imported, report.skipped), (1, 2));
    assert_eq!(src.get_snippet(bound.id).unwrap().unwrap().command, bound.command);
}

#[test]
fn invalid_snippets_are_counted_and_not_written() {
    let (src, _host, bound, global) = snippet_source();
    let mut json = export_json(&src);
    json["snippets"][0]["command"] = serde_json::Value::String(String::new());
    let dst = Store::open_in_memory().unwrap();

    let report = import_into(&dst, &serde_json::to_vec(&json).unwrap(), &HashMap::new());

    assert_eq!((report.imported, report.invalid), (2, 1));
    assert_eq!(dst.get_snippet(bound.id).unwrap(), None);
    assert!(dst.get_snippet(global.id).unwrap().is_some());
}
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `cargo test -p linascp-core export 2>&1 | tail -20`
Expected: compile error: `no variant named Snippet` in `ItemKind`.

- [ ] **Step 3: Implement**

`crates/core/src/store/mod.rs`: extend `save_import`. The doc comment stays the same.

```rust
    pub fn save_import(
        &self,
        groups: &[Group],
        profiles: &[LoginProfile],
        hosts: &[Host],
        snippets: &[Snippet],
        settings: &[(&str, &str)],
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for g in groups {
            upsert_group(&tx, g)?;
        }
        for p in profiles {
            upsert_profile(&tx, p)?;
        }
        for h in hosts {
            upsert_host(&tx, h)?;
        }
        for s in snippets {
            upsert_snippet(&tx, s)?;
        }
        for (key, value) in settings {
            upsert_setting(&tx, key, value)?;
        }
        tx.commit()?;
        Ok(())
    }
```

`crates/core/src/export.rs`:
1. Imports:
   - `use crate::model::{AuthConfig, AuthMethod, Group, Host, HostAuth, Id, LoginProfile, Snippet, now_unix};`
   - `use crate::validate::{validate_host, validate_profile, validate_snippet, validate_terminal_overrides};`
2. `ItemKind` gains `Snippet` as its last variant.
3. In `ExportDoc`, add this field after `hosts`:
   ```rust
       /// Missing in files from builds before TP2b.
       #[serde(default)]
       snippets: Vec<Snippet>,
   ```
4. `export_data`:
   - Add `let snippets = store.list_snippets()?;` after `let hosts = …`.
   - Add `snippets,` to the `ExportDoc { … }` literal after `hosts,`.
5. `import_preview`, after the hosts loop:
   ```rust
       for s in &doc.snippets {
           items.push(item(s.id, &s.name, ItemKind::Snippet, existing.conflict(ItemKind::Snippet, s.id, &s.name)));
       }
   ```
6. `import_apply`:
   - After the `for h in &doc.hosts { plan.decide(…) }` loop, add:
     ```rust
         for s in &doc.snippets {
             plan.decide(&existing, resolutions, ItemKind::Snippet, s.id, &s.name);
         }
     ```
   - After the `for h in doc.hosts.iter().filter(|h| validate_host(h).is_err())` loop, add:
     ```rust
         for s in doc.snippets.iter().filter(|s| validate_snippet(s).is_err()) {
             plan.reject(s.id);
         }
     ```
   - After the `let mut hosts = Vec::new(); for h in &doc.hosts { … }` block, add:
     ```rust
         // A snippet keeps its host only if that host exists after the import; otherwise it turns global.
         let known_hosts: HashSet<Id> = hosts.iter().map(|h| h.id).chain(existing.ids(ItemKind::Host)).collect();
         let mut snippets = Vec::new();
         for s in &doc.snippets {
             match plan.action(s.id) {
                 Action::Write { id, name } => {
                     let host_id = s.host_id.map(|h| plan.map_id(h)).filter(|h| known_hosts.contains(h));
                     snippets.push(Snippet { id, name, host_id, ..s.clone() });
                 }
                 Action::Skip if plan.is_rejected(s.id) => report.invalid += 1,
                 Action::Skip => report.skipped += 1,
             }
         }
     ```
   - Change the store call to `store.save_import(&groups, &profiles, &hosts, &snippets, &settings)`.
   - Change the counter to
     `report.imported = (groups.len() + profiles.len() + hosts.len() + snippets.len()) as u32;`.
7. `Existing::load` gains:
   ```rust
           items.extend(store.list_snippets()?.into_iter().map(|s| (ItemKind::Snippet, s.id, s.name)));
   ```
   Add this method to `impl Existing`:
   ```rust
       fn ids(&self, kind: ItemKind) -> impl Iterator<Item = Id> + '_ {
           self.items.iter().filter(move |(k, _, _)| *k == kind).map(|(_, id, _)| *id)
       }
   ```

- [ ] **Step 4: Run the core tests**

Run: `cargo test -p linascp-core 2>&1 | grep -E "test result|FAILED|panicked|warning"`
Expected: all ok, including the older export tests whose counts did not change.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/export.rs crates/core/src/export/tests.rs crates/core/src/store/mod.rs
git commit -m "feat(core): export and import snippets through the conflict plan

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Tauri commands, bindings, UI api and store

**Files:**
- Create: `src-tauri/src/commands/snippets.rs`
- Modify: `src-tauri/src/commands/mod.rs` and `src-tauri/src/lib.rs`
- Regenerate: `ui/src/ipc/bindings.ts`
- Modify:
  - `ui/src/ipc/api.ts`
  - `ui/src/test/fixtures.ts`
  - `ui/src/stores/hosts.ts`
  - `ui/src/shell/AppShell.tsx`
  - `ui/src/i18n/de.ts` (import kinds)
- Create: `ui/src/stores/snippets.ts` and `ui/src/stores/snippets.test.ts`
- Test: `ui/src/stores/stores.test.ts` and `ui/src/shell/AppShell.test.tsx`

**Interfaces:**
- Consumes:
  - Task 1: `Store::{list_snippets, get_snippet, save_snippet, delete_snippet}` and `validate_snippet`.
  - Task 2: `ItemKind::Snippet`.
- Produces:
  - Tauri commands `snippets_list() -> Vec<Snippet>`, `snippet_save(snippet) -> Snippet` and `snippet_delete(id)`.
  - `api.ts`: `type Snippet`, `snippetsList(): Promise<Snippet[]>`, `snippetSave(s: Snippet): Promise<Snippet>` and
    `snippetDelete(id: Id): Promise<void>`.
  - `useSnippetsStore`:
    - `snippets: Snippet[]` (core order: by name);
    - `loaded: boolean`;
    - `load(): Promise<void>`;
    - `save(s: Snippet): Promise<Snippet>`;
    - `remove(id: Id): Promise<void>`.
  - Test fixture `makeSnippet(overrides?: Partial<Snippet>): Snippet`.

- [ ] **Step 1: Write the failing Rust test and the command module skeleton**

Create `src-tauri/src/commands/snippets.rs`:

```rust
use linascp_core::error::{CoreError, Result};
use linascp_core::model::{Id, Snippet};
use linascp_core::validate::validate_snippet;
use tauri::State;

use crate::state::AppState;

/// Validates and upserts; returns the stored snippet (with its timestamps).
pub(crate) fn save_snippet(state: &AppState, snippet: Snippet) -> Result<Snippet> {
    validate_snippet(&snippet)?;
    state.store.save_snippet(&snippet)?;
    state
        .store
        .get_snippet(snippet.id)?
        .ok_or_else(|| CoreError::internal(format!("snippet {} vanished after save", snippet.id)))
}

#[tauri::command]
#[specta::specta]
pub async fn snippets_list(state: State<'_, AppState>) -> Result<Vec<Snippet>> {
    state.store.list_snippets()
}

#[tauri::command]
#[specta::specta]
pub async fn snippet_save(state: State<'_, AppState>, snippet: Snippet) -> Result<Snippet> {
    save_snippet(&state, snippet)
}

#[tauri::command]
#[specta::specta]
pub async fn snippet_delete(state: State<'_, AppState>, id: Id) -> Result<()> {
    state.store.delete_snippet(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use linascp_core::model::Host;

    #[test]
    fn save_validates_and_returns_the_stored_snippet() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        assert!(matches!(save_snippet(&state, Snippet::new(" ", "ls", None)), Err(CoreError::Config { .. })));

        let host = Host::new("web", "192.0.2.20");
        state.store.save_host(&host).unwrap();
        let mut snippet = Snippet::new("Logs", "journalctl -f", Some(host.id));
        snippet.updated_at = 0;
        let saved = save_snippet(&state, snippet.clone()).unwrap();

        assert_eq!((saved.id, saved.host_id), (snippet.id, Some(host.id)));
        assert!(saved.updated_at > 0);
        assert_eq!(state.store.list_snippets().unwrap(), vec![saved]);
    }
}
```

Add `pub mod snippets;` to `src-tauri/src/commands/mod.rs`. Leave `lib.rs` unchanged for now.

- [ ] **Step 2: Run the test**

Run: `SHELL=/bin/sh cargo test -p linascp snippets 2>&1 | grep -E "test result|FAILED|warning|error"`
Expected: 1 passed. The code under test is the core from Tasks 1–2, so this step pins the command layer rather than
showing a red phase.

- [ ] **Step 3: Register the commands and regenerate the bindings**

`src-tauri/src/lib.rs`:
- Change the import to `use commands::{connect, hosts, profiles, secrets, sessions, settings, snippets, transfer};`.
- After `profiles::profile_delete,`, add:

```rust
            snippets::snippets_list,
            snippets::snippet_save,
            snippets::snippet_delete,
```

Run: `SHELL=/bin/sh cargo test -p linascp exports_bindings && bun run typecheck`
Expected:
- the bindings test passes;
- `bindings.ts` now contains `snippetsList`, `Snippet` and `"snippet"` in `ItemKind`;
- typecheck FAILS in `ImportDialog.tsx`, because `texts.kinds` has no `snippet` key.

- [ ] **Step 4: Write the failing UI tests**

Add this to `ui/src/test/fixtures.ts`, and add `Snippet` to its type import:

```ts
export function makeSnippet(overrides: Partial<Snippet> = {}): Snippet {
  return {
    id: "sn-1",
    name: "Logs folgen",
    command: "docker compose logs -f",
    hostId: null,
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}
```

Create `ui/src/stores/snippets.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import { makeSnippet } from "../test/fixtures";
import { useSnippetsStore } from "./snippets";

vi.mock("../ipc/api");

beforeEach(() => {
  useSnippetsStore.setState(useSnippetsStore.getInitialState(), true);
});

describe("snippets store", () => {
  it("reloads the list after save and remove", async () => {
    const snippet = makeSnippet();
    vi.mocked(api.snippetSave).mockResolvedValue(snippet);
    vi.mocked(api.snippetsList).mockResolvedValueOnce([snippet]).mockResolvedValueOnce([]);

    await expect(useSnippetsStore.getState().save(snippet)).resolves.toEqual(snippet);
    expect(useSnippetsStore.getState()).toMatchObject({ snippets: [snippet], loaded: true });

    await useSnippetsStore.getState().remove(snippet.id);
    expect(api.snippetDelete).toHaveBeenCalledWith(snippet.id);
    expect(useSnippetsStore.getState().snippets).toEqual([]);
  });
});
```

Add a test to the `describe("hosts store", …)` block in `ui/src/stores/stores.test.ts` for Review Focus 5. Import
`useSnippetsStore` from `./snippets` and `makeSnippet` from `../test/fixtures`:

```ts
  it("reloads the snippets after deleting a host (cascade)", async () => {
    useSnippetsStore.setState({ snippets: [makeSnippet({ hostId: "h1" })], loaded: true });
    vi.mocked(api.snippetsList).mockResolvedValue([]);
    await useHostsStore.getState().deleteHost("h1");
    expect(useSnippetsStore.getState().snippets).toEqual([]);
  });
```

In `ui/src/shell/AppShell.test.tsx`:
- Next to `vi.mocked(api.hostsList).mockResolvedValue([]);`, add
  `vi.mocked(api.snippetsList).mockResolvedValue([]);`.
- Add this test to the file's `describe` block:

```ts
  it("loads the snippets on start", async () => {
    renderShell();
    await waitFor(() => expect(api.snippetsList).toHaveBeenCalled());
  });
```

The existing test file already has a render helper. If it is not called `renderShell`, use its name, and import
`waitFor` from `@testing-library/react` if it is missing.

- [ ] **Step 5: Run the UI tests and verify that they fail**

Run: `bun run test ui/src/stores ui/src/shell/AppShell.test.tsx 2>&1 | tail -20`
Expected: FAIL. The cause is that `./snippets` cannot be resolved and `api.snippetsList` is not a function.

- [ ] **Step 6: Implement the UI plumbing**

`ui/src/ipc/api.ts`:
- Add `Snippet` to both the `import type { … } from "./bindings"` list and the `export type { … }` list, in
  alphabetical order.
- After the profiles/secrets section, add:

```ts
// Snippets
export function snippetsList(): Promise<Snippet[]> {
  return unwrap(commands.snippetsList());
}
export function snippetSave(snippet: Snippet): Promise<Snippet> {
  return unwrap(commands.snippetSave(snippet));
}
export function snippetDelete(id: Id): Promise<void> {
  return unwrapVoid(commands.snippetDelete(id));
}
```

Create `ui/src/stores/snippets.ts`:

```ts
import { create } from "zustand";
import { type Id, type Snippet, snippetDelete, snippetSave, snippetsList } from "../ipc/api";

export interface SnippetsState {
  /** In core order: by name. */
  snippets: Snippet[];
  loaded: boolean;
  load(): Promise<void>;
  save(snippet: Snippet): Promise<Snippet>;
  remove(id: Id): Promise<void>;
}

export const useSnippetsStore = create<SnippetsState>()((set, get) => ({
  snippets: [],
  loaded: false,
  load: async () => {
    set({ snippets: await snippetsList(), loaded: true });
  },
  save: async (snippet) => {
    const saved = await snippetSave(snippet);
    await get().load();
    return saved;
  },
  remove: async (id) => {
    await snippetDelete(id);
    await get().load();
  },
}));
```

In `ui/src/stores/hosts.ts`, import `useSnippetsStore` from `./snippets` and change `deleteHost` to:

```ts
  deleteHost: async (id) => {
    await hostDelete(id);
    set((state) => ({ selectedId: state.selectedId === id ? null : state.selectedId }));
    // The host's snippets are gone too (ON DELETE CASCADE).
    await Promise.all([get().load(), useSnippetsStore.getState().load()]);
  },
```

`ui/src/shell/AppShell.tsx`:
- Import `useSnippetsStore`.
- Add `const loadSnippets = useSnippetsStore((state) => state.load);`.
- In the load effect, add `loadSnippets().catch(report);` and add `loadSnippets` to the dependency array.

`ui/src/i18n/de.ts`: change the import kinds line to
`kinds: { host: "Host", profile: "Login-Profil", group: "Gruppe", snippet: "Snippet" },`.

- [ ] **Step 7: Run all checks**

Run: `bun run typecheck && bun run test 2>&1 | tail -8 && SHELL=/bin/sh cargo test -p linascp 2>&1 | grep -E "test result|FAILED"`
Expected:
- typecheck is clean;
- all UI tests pass without warnings;
- the src-tauri tests are ok.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src ui/src/ipc ui/src/stores ui/src/shell ui/src/test/fixtures.ts ui/src/i18n/de.ts
git commit -m "feat: snippet commands, bindings, api wrappers and store

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Placeholder helpers and the paste-target registry

**Files:**
- Create:
  - `ui/src/screens/snippets/placeholders.ts`
  - `ui/src/screens/snippets/placeholders.test.ts`
  - `ui/src/screens/terminal/pasteTargets.ts`
- Modify: `ui/src/screens/terminal/TerminalView.tsx`
- Test: `ui/src/screens/terminal/TerminalView.test.tsx`

**Interfaces:**
- Produces:
  - `placeholderNames(text: string): string[]`: unique names, in order of first use.
  - `fillPlaceholders(text: string, values: Record<string, string>): string`. A missing value is replaced by `""`.
  - `interface PasteTarget { insert(text: string): void; run(text: string): void }`.
  - `registerPasteTarget(tabId: string, target: PasteTarget)`, `dropPasteTarget(tabId: string)` and
    `getPasteTarget(tabId: string): PasteTarget | undefined`.

- [ ] **Step 1: Write the failing tests**

Create `ui/src/screens/snippets/placeholders.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { fillPlaceholders, placeholderNames } from "./placeholders";

describe("placeholderNames", () => {
  it("lists unique names in order of first use", () => {
    expect(placeholderNames("scp {{file}} {{host}}:{{file}}")).toEqual(["file", "host"]);
  });

  it("ignores everything that is not {{name}}", () => {
    const tooLong = `{{${"n".repeat(33)}}}`;
    expect(placeholderNames(`{{ x y }} {{}} {{a.b}} ${tooLong} \${HOME} {x}`)).toEqual([]);
  });
});

describe("fillPlaceholders", () => {
  it("replaces every occurrence literally, without quoting", () => {
    expect(fillPlaceholders("cp {{f}} {{f}}.bak && echo '{{ x }}'", { f: "a b;$(rm)" })).toBe(
      "cp a b;$(rm) a b;$(rm).bak && echo '{{ x }}'",
    );
  });

  it("does not resolve placeholders inside values", () => {
    expect(fillPlaceholders("{{a}}{{b}}", { a: "{{b}}", b: "x" })).toBe("{{b}}x");
  });

  it("fills missing values with an empty string", () => {
    expect(fillPlaceholders("x{{a}}y", {})).toBe("xy");
  });
});
```

`ui/src/screens/terminal/TerminalView.test.tsx`:
- Add to `FakeTerminal`, next to `pasted`:
  ```ts
      inputs: string[] = [];
  ```
  and next to `paste(…)`:
  ```ts
      input(data: string) {
        this.inputs.push(data);
      }
  ```
- Import `getPasteTarget` from `./pasteTargets`.
- Add this test (Review Focus 2):

```ts
  it("registers a paste target: insert pastes, run pastes then sends Enter through xterm input", () => {
    const { unmount } = render(<TerminalView tab={tab} visible />);
    const target = getPasteTarget(tab.id);
    target?.insert("ls -la");
    target?.run("cd /srv\ngit pull");
    expect(term().pasted).toEqual(["ls -la", "cd /srv\ngit pull"]);
    // Enter goes through term.input → onData → the same ordered sessionWrite queue as the paste.
    expect(term().inputs).toEqual(["\r"]);
    unmount();
    expect(getPasteTarget(tab.id)).toBeUndefined();
  });
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `bun run test ui/src/screens/snippets ui/src/screens/terminal/TerminalView.test.tsx 2>&1 | tail -15`
Expected: FAIL. The modules `./placeholders` and `./pasteTargets` cannot be resolved.

- [ ] **Step 3: Implement**

Create `ui/src/screens/snippets/placeholders.ts`:

```ts
/** `{{name}}` with a 1–32 character name; anything else stays literal text. */
const PLACEHOLDER = /\{\{([A-Za-z0-9_-]{1,32})\}\}/g;

/** Unique placeholder names in order of first use. */
export function placeholderNames(text: string): string[] {
  const names: string[] = [];
  for (const match of text.matchAll(PLACEHOLDER)) {
    const name = match[1] as string;
    if (!names.includes(name)) names.push(name);
  }
  return names;
}

/** Replaces every placeholder with its value, verbatim (no quoting, no re-resolution). */
export function fillPlaceholders(text: string, values: Record<string, string>): string {
  return text.replace(PLACEHOLDER, (_match, name: string) => values[name] ?? "");
}
```

Create `ui/src/screens/terminal/pasteTargets.ts`:

```ts
/** What the snippets panel can do with a tab's terminal. `TerminalView` registers it for its lifetime. */
export interface PasteTarget {
  /** Pastes like Ctrl+Shift+V (bracketed paste if the shell enabled it) and focuses the terminal. */
  insert(text: string): void;
  /** `insert`, then Enter. */
  run(text: string): void;
}

const targets = new Map<string, PasteTarget>();

export function registerPasteTarget(tabId: string, target: PasteTarget): void {
  targets.set(tabId, target);
}

export function dropPasteTarget(tabId: string): void {
  targets.delete(tabId);
}

export function getPasteTarget(tabId: string): PasteTarget | undefined {
  return targets.get(tabId);
}
```

`ui/src/screens/terminal/TerminalView.tsx`:
- Import `dropPasteTarget` and `registerPasteTarget` from `./pasteTargets`.
- In the main effect, directly after `container.addEventListener("mouseup", onMouseUp, true);`, add:

```ts
    registerPasteTarget(tab.id, {
      insert: (text) => {
        term.paste(text);
        term.focus();
      },
      // term.input feeds onData, so Enter is queued behind the pasted text (and outside the paste brackets).
      run: (text) => {
        term.paste(text);
        term.input("\r");
        term.focus();
      },
    });
```

In the cleanup, after `detach?.();`, add `dropPasteTarget(tab.id);`.

- [ ] **Step 4: Run the tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -8`
Expected: everything passes without warnings.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/snippets ui/src/screens/terminal
git commit -m "feat(ui): snippet placeholders and terminal paste targets

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Snippet editor modal

**Files:**
- Create:
  - `ui/src/screens/snippets/SnippetEditor.tsx`
  - `ui/src/screens/snippets/SnippetEditor.test.tsx`
  - `ui/src/screens/snippets/Snippets.module.css`
- Modify: `ui/src/stores/ui.ts`, `ui/src/shell/ModalLayer.tsx`, `ui/src/i18n/de.ts`

**Interfaces:**
- Consumes:
  - Task 3: `useSnippetsStore`, `makeSnippet`, `api.snippetSave` and `api.snippetDelete`.
- Produces:
  - `ModalState` variant `{ type: "snippet"; id?: Id; hostId?: Id | null }`:
    - `id` undefined means a new snippet;
    - `hostId` preselects the scope of a new snippet.
  - The `t.snippets` strings block, used by Tasks 6–7.
  - `Snippets.module.css` with the classes `form`, `confirm`, `panel`, `cards`, `card`, `insert`, `cardHead`, `name`,
    `scope`, `command`, `actions`, `add`, `empty`, `screen`, `header`, `title`, `table` and `link`. The later tasks
    reuse them.

- [ ] **Step 1: Add the strings**

In `ui/src/i18n/de.ts`:
- Add `snippets: "Snippets",` to `terminal.sidebar`, after `label`.
- Add this new top-level block directly before `settings: {`:

```ts
  snippets: {
    title: "Snippets",
    search: "Snippets suchen",
    add: "+ Snippet anlegen",
    global: "Global",
    empty: "Noch keine Snippets",
    noMatch: "Keine Treffer",
    insert: (name: string) => `${name} einfügen`,
    run: "Ausführen",
    edit: "Bearbeiten",
    filter: { label: "Gültigkeit", all: "Alle", global: "Global", host: "Host" },
    columns: { name: "Name", scope: "Gilt für", command: "Befehl" },
    editor: {
      titleNew: "Snippet anlegen",
      titleEdit: "Snippet bearbeiten",
      name: "Name",
      command: "Befehl",
      commandHint: "Platzhalter: {{name}} · Keine Passwörter speichern",
      scope: "Gilt für",
      nameRequired: "Name fehlt",
      commandRequired: "Befehl fehlt",
      deleteConfirm: "Snippet wirklich löschen?",
      saveFailed: "Snippet konnte nicht gespeichert werden",
      deleteFailed: "Snippet konnte nicht gelöscht werden",
    },
    placeholders: {
      title: "Platzhalter ausfüllen",
      insert: "Einfügen",
      run: "Ausführen",
    },
  },
```

- [ ] **Step 2: Write the failing tests**

Create `ui/src/screens/snippets/SnippetEditor.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../components";
import * as api from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { useSnippetsStore } from "../../stores/snippets";
import { useUiStore } from "../../stores/ui";
import { makeHost, makeSnippet } from "../../test/fixtures";
import { SnippetEditor } from "./SnippetEditor";

vi.mock("../../ipc/api");

function renderEditor(props: { id?: string; hostId: string | null }) {
  return render(
    <ToastProvider>
      <SnippetEditor {...props} />
    </ToastProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState({ ...useUiStore.getInitialState(), modal: { type: "snippet" } }, true);
  useHostsStore.setState(
    {
      ...useHostsStore.getInitialState(),
      hosts: [
        makeHost({ id: "h1", name: "web-prod-01" }),
        makeHost({ id: "h2", name: "from-config", source: "sshConfig" }),
      ],
    },
    true,
  );
  useSnippetsStore.setState(useSnippetsStore.getInitialState(), true);
  vi.mocked(api.newId).mockReturnValue("sn-new");
  vi.mocked(api.snippetSave).mockImplementation(async (snippet) => snippet);
  vi.mocked(api.snippetsList).mockResolvedValue([]);
});

describe("SnippetEditor", () => {
  it("creates a snippet for the preselected host and closes", async () => {
    renderEditor({ hostId: "h1" });
    const scope = screen.getByRole("combobox", { name: "Gilt für" });
    expect(scope).toHaveValue("h1");
    expect(screen.queryByRole("option", { name: "from-config" })).toBeNull();
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "Logs");
    await userEvent.type(screen.getByRole("textbox", { name: "Befehl" }), "journalctl -u nginx");
    await userEvent.click(screen.getByRole("button", { name: "Speichern" }));
    expect(api.snippetSave).toHaveBeenCalledWith(
      expect.objectContaining({ id: "sn-new", name: "Logs", command: "journalctl -u nginx", hostId: "h1" }),
    );
    expect(useUiStore.getState().modal).toBeNull();
  });

  it("falls back to Global for a host from ~/.ssh/config", () => {
    renderEditor({ hostId: "h2" });
    expect(screen.getByRole("combobox", { name: "Gilt für" })).toHaveValue("global");
  });

  it("requires name and command", async () => {
    renderEditor({ hostId: null });
    await userEvent.click(screen.getByRole("button", { name: "Speichern" }));
    expect(screen.getByText("Name fehlt")).toBeInTheDocument();
    expect(screen.getByText("Befehl fehlt")).toBeInTheDocument();
    expect(api.snippetSave).not.toHaveBeenCalled();
  });

  it("keeps the dialog open when the core rejects the snippet", async () => {
    vi.mocked(api.snippetSave).mockRejectedValue({ type: "config", message: "Befehl darf keine NUL-Zeichen enthalten" });
    renderEditor({ hostId: null });
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "x");
    await userEvent.type(screen.getByRole("textbox", { name: "Befehl" }), "ls");
    await userEvent.click(screen.getByRole("button", { name: "Speichern" }));
    expect(await screen.findByText("Snippet konnte nicht gespeichert werden")).toBeInTheDocument();
    expect(useUiStore.getState().modal).toEqual({ type: "snippet" });
  });

  it("edits an existing snippet and deletes it only after confirmation", async () => {
    useSnippetsStore.setState({ snippets: [makeSnippet({ id: "sn-1", hostId: null })], loaded: true });
    renderEditor({ id: "sn-1", hostId: null });
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("Logs folgen");
    expect(screen.getByRole("combobox", { name: "Gilt für" })).toHaveValue("global");
    await userEvent.click(screen.getByRole("button", { name: "Löschen" }));
    expect(screen.getByText("Snippet wirklich löschen?")).toBeInTheDocument();
    expect(api.snippetDelete).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Löschen" }));
    expect(api.snippetDelete).toHaveBeenCalledWith("sn-1");
    expect(useUiStore.getState().modal).toBeNull();
  });
});
```

- [ ] **Step 3: Run the tests and verify that they fail**

Run: `bun run test ui/src/screens/snippets/SnippetEditor.test.tsx 2>&1 | tail -10`
Expected: FAIL. The module `./SnippetEditor` cannot be resolved.

- [ ] **Step 4: Implement**

`ui/src/stores/ui.ts`: add this to the `ModalState` union:

```ts
  /** `id` undefined = new snippet; `hostId` preselects its scope. */
  | { type: "snippet"; id?: Id; hostId?: Id | null }
```

`ui/src/shell/ModalLayer.tsx`: import `SnippetEditor` from `../screens/snippets/SnippetEditor` and add this case:

```tsx
    case "snippet":
      return <SnippetEditor key={modal.id ?? "new"} id={modal.id} hostId={modal.hostId ?? null} />;
```

Create `ui/src/screens/snippets/SnippetEditor.tsx`:

```tsx
import { useState } from "react";
import { Button, Dialog, Select, TextArea, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import { type Id, newId, type Snippet } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useHostsStore } from "../../stores/hosts";
import { useSnippetsStore } from "../../stores/snippets";
import { useUiStore } from "../../stores/ui";
import styles from "./Snippets.module.css";

const GLOBAL = "global";

/** Create (`id` undefined) or edit a snippet. `hostId` preselects the scope of a new one. */
export function SnippetEditor({ id, hostId }: { id?: Id; hostId: Id | null }) {
  const texts = t.snippets.editor;
  const existing = useSnippetsStore((state) => state.snippets.find((snippet) => snippet.id === id));
  const save = useSnippetsStore((state) => state.save);
  const remove = useSnippetsStore((state) => state.remove);
  const allHosts = useHostsStore((state) => state.hosts);
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  // Only stored hosts can own snippets: ssh-config hosts are not in the DB (FK).
  const hosts = allHosts.filter((host) => host.source === "linascp");
  const preset = existing ? existing.hostId : hostId;

  const [name, setName] = useState(existing?.name ?? "");
  const [command, setCommand] = useState(existing?.command ?? "");
  const [scope, setScope] = useState(preset && hosts.some((host) => host.id === preset) ? preset : GLOBAL);
  const [touched, setTouched] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);

  const run = async (action: () => Promise<unknown>, failed: string) => {
    setBusy(true);
    try {
      await action();
      closeModal();
    } catch (error) {
      toast.show({ tone: "red", message: failed, details: errorDetails(error) });
    } finally {
      setBusy(false);
    }
  };

  const submit = () => {
    setTouched(true);
    if (!name.trim() || !command.trim()) return;
    const snippet: Snippet = {
      id: existing?.id ?? newId(),
      name: name.trim(),
      command,
      hostId: scope === GLOBAL ? null : scope,
      createdAt: existing?.createdAt ?? 0,
      updatedAt: existing?.updatedAt ?? 0,
    };
    void run(() => save(snippet), texts.saveFailed);
  };

  const footer = confirmDelete ? (
    <>
      <span className={styles.confirm}>{texts.deleteConfirm}</span>
      <Button variant="ghost" onClick={() => setConfirmDelete(false)}>
        {t.common.cancel}
      </Button>
      <Button
        variant="danger"
        disabled={busy}
        onClick={() => existing && void run(() => remove(existing.id), texts.deleteFailed)}
      >
        {t.common.delete}
      </Button>
    </>
  ) : (
    <>
      {existing ? (
        <Button variant="danger" onClick={() => setConfirmDelete(true)}>
          {t.common.delete}
        </Button>
      ) : null}
      <Button variant="ghost" onClick={closeModal}>
        {t.common.cancel}
      </Button>
      <Button variant="primary" disabled={busy} onClick={submit}>
        {t.common.save}
      </Button>
    </>
  );

  return (
    <Dialog open size="md" title={existing ? texts.titleEdit : texts.titleNew} onClose={closeModal} footer={footer}>
      <div className={styles.form}>
        <TextField
          label={texts.name}
          value={name}
          onChange={setName}
          error={touched && !name.trim() ? texts.nameRequired : null}
          maxLength={64}
        />
        <TextArea
          label={texts.command}
          value={command}
          onChange={setCommand}
          error={touched && !command.trim() ? texts.commandRequired : null}
          hint={texts.commandHint}
          mono
          rows={6}
        />
        <Select
          label={texts.scope}
          value={scope}
          options={[
            { value: GLOBAL, label: t.snippets.global },
            ...hosts.map((host) => ({ value: host.id, label: host.name })),
          ]}
          onChange={setScope}
        />
      </div>
    </Dialog>
  );
}
```

Create `ui/src/screens/snippets/Snippets.module.css`. Tasks 6 and 7 use the classes as well.

```css
.form {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.confirm {
  margin-right: auto;
  color: var(--text);
  font-size: 13px;
}

.panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.cards {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.card {
  position: relative;
  border: 1px solid var(--bd);
  border-radius: 10px;
  background: var(--card);
}

.insert {
  all: unset;
  display: flex;
  flex-direction: column;
  gap: 6px;
  box-sizing: border-box;
  width: 100%;
  padding: 12px;
  cursor: pointer;
}

.insert:disabled {
  cursor: default;
  opacity: 0.6;
}

.insert:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
  border-radius: 10px;
}

.cardHead {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  padding-right: 56px;
}

.name {
  color: var(--text);
  font-size: 13px;
  font-weight: 500;
}

.scope {
  color: var(--muted);
  font-size: 11px;
}

.command {
  overflow: hidden;
  color: var(--text3);
  font-family: "JetBrains Mono", monospace;
  font-size: 12px;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.actions {
  position: absolute;
  top: 6px;
  right: 6px;
  display: flex;
  gap: 2px;
  opacity: 0;
}

.card:hover .actions,
.card:focus-within .actions {
  opacity: 1;
}

.add {
  height: 40px;
  border: 1px dashed var(--bd-dash);
  border-radius: 10px;
  background: transparent;
  color: var(--muted);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}

.empty {
  margin: 0;
  color: var(--muted);
  font-size: 13px;
}

.screen {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 16px;
  min-height: 0;
  padding: 24px;
  overflow-y: auto;
}

.header {
  display: flex;
  align-items: flex-end;
  gap: 12px;
  flex-wrap: wrap;
}

.title {
  margin: 0 auto 0 0;
  color: var(--text);
  font-size: 20px;
  font-weight: 600;
}

.table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.table th {
  padding: 8px;
  color: var(--muted);
  font-weight: 500;
  text-align: left;
  border-bottom: 1px solid var(--bd);
}

.table td {
  max-width: 0;
  padding: 8px;
  border-bottom: 1px solid var(--bd);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.link {
  all: unset;
  color: var(--text);
  font-weight: 500;
  cursor: pointer;
}

.link:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
```

Check the variable names before you rely on them: `grep -n "\-\-accent\|\-\-text3\|\-\-bd-dash\|\-\-card" ui/src/styles/*.css`. Use only tokens that exist. If one is missing, take the closest existing token.

- [ ] **Step 5: Run the tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -8`
Expected: everything passes without warnings.

- [ ] **Step 6: Commit**

```bash
git add ui/src/screens/snippets ui/src/stores/ui.ts ui/src/shell/ModalLayer.tsx ui/src/i18n/de.ts
git commit -m "feat(ui): snippet editor modal

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Placeholder dialog, sidebar panel "Snippets"

**Files:**
- Create:
  - `ui/src/screens/snippets/PlaceholderDialog.tsx`
  - `ui/src/screens/snippets/format.ts`
  - `ui/src/screens/terminal/sidebar/SnippetsPanel.tsx`
  - `ui/src/screens/terminal/sidebar/SnippetsPanel.test.tsx`
- Modify: `ui/src/screens/terminal/sidebar/Sidebar.tsx`
- Test: `ui/src/screens/terminal/TerminalScreen.test.tsx`

**Interfaces:**
- Consumes:
  - Task 3: `useSnippetsStore`, `makeSnippet`.
  - Task 4: `placeholderNames`, `fillPlaceholders`, `getPasteTarget`, `registerPasteTarget`, `dropPasteTarget`.
  - Task 5: `t.snippets`, `Snippets.module.css`, `ModalState` `"snippet"`.
- Produces:
  - `firstLine(text: string): string`.
  - `matchesQuery(snippet: Snippet, query: string): boolean`. The query is trimmed, the match is case-insensitive on
    name and command, and an empty query matches everything.
  - `PlaceholderDialog`. Task 7 consumes `format.ts`.

- [ ] **Step 1: Write the failing tests**

Create `ui/src/screens/terminal/sidebar/SnippetsPanel.test.tsx`:

```tsx
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { useSnippetsStore } from "../../../stores/snippets";
import { useUiStore } from "../../../stores/ui";
import { makeHost, makeSnippet } from "../../../test/fixtures";
import { dropPasteTarget, registerPasteTarget } from "../pasteTargets";
import { SnippetsPanel } from "./SnippetsPanel";

vi.mock("../../../ipc/api");

const sshTab: Tab = { id: "t1", title: "web", kind: "ssh", hostId: "h1", sessionId: "s1", status: "connected" };
const localTab: Tab = { id: "t1", title: "Lokal", kind: "local", sessionId: "s2", status: "connected" };
const target = { insert: vi.fn(), run: vi.fn() };

// Core order (by name).
const SNIPPETS = [
  makeSnippet({ id: "a", name: "Fremd", command: "uptime", hostId: "h9" }),
  makeSnippet({ id: "b", name: "Logs", command: "journalctl -u {{unit}} -n {{lines}} # {{unit}}", hostId: null }),
  makeSnippet({ id: "c", name: "Nginx", command: "sudo nginx -t &&\nsudo systemctl reload nginx", hostId: "h1" }),
  makeSnippet({ id: "d", name: "Speicher", command: "df -h", hostId: null }),
];

const cardNames = () =>
  screen.getAllByRole("button", { name: / einfügen$/ }).map((button) => button.getAttribute("aria-label"));
const card = (name: string) => screen.getByRole("button", { name: `${name} einfügen` }).closest("li") as HTMLElement;

beforeEach(() => {
  target.insert.mockClear();
  target.run.mockClear();
  registerPasteTarget("t1", target);
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState({ ...useHostsStore.getInitialState(), hosts: [makeHost({ id: "h1", name: "web" })] }, true);
  useSnippetsStore.setState({ snippets: SNIPPETS, loaded: true });
});

afterEach(() => dropPasteTarget("t1"));

describe("SnippetsPanel", () => {
  it("lists the host's snippets first, then global ones, and filters by name and command", async () => {
    render(<SnippetsPanel tab={sshTab} />);
    expect(cardNames()).toEqual(["Nginx einfügen", "Logs einfügen", "Speicher einfügen"]);
    expect(within(card("Nginx")).getByText("web")).toBeInTheDocument();
    expect(within(card("Speicher")).getByText("Global")).toBeInTheDocument();
    await userEvent.type(screen.getByRole("searchbox", { name: "Snippets suchen" }), "DF");
    expect(cardNames()).toEqual(["Speicher einfügen"]);
    await userEvent.type(screen.getByRole("searchbox", { name: "Snippets suchen" }), "zzz");
    expect(screen.getByText("Keine Treffer")).toBeInTheDocument();
  });

  it("shows only global snippets in a local terminal", () => {
    render(<SnippetsPanel tab={localTab} />);
    expect(cardNames()).toEqual(["Logs einfügen", "Speicher einfügen"]);
  });

  it("shows an empty state", () => {
    useSnippetsStore.setState({ snippets: [] });
    render(<SnippetsPanel tab={sshTab} />);
    expect(screen.getByText("Noch keine Snippets")).toBeInTheDocument();
  });

  it("inserts on click and runs with the play button", async () => {
    render(<SnippetsPanel tab={sshTab} />);
    await userEvent.click(screen.getByRole("button", { name: "Speicher einfügen" }));
    expect(target.insert).toHaveBeenCalledWith("df -h");
    await userEvent.click(within(card("Nginx")).getByRole("button", { name: "Ausführen" }));
    expect(target.run).toHaveBeenCalledWith("sudo nginx -t &&\nsudo systemctl reload nginx");
  });

  it("asks for placeholder values first; cancel sends nothing", async () => {
    render(<SnippetsPanel tab={sshTab} />);
    await userEvent.click(screen.getByRole("button", { name: "Logs einfügen" }));
    const dialog = screen.getByRole("dialog", { name: "Platzhalter ausfüllen" });
    expect(within(dialog).getAllByRole("textbox").map((box) => box.getAttribute("aria-label") ?? "")).toHaveLength(2);
    await userEvent.click(within(dialog).getByRole("button", { name: "Abbrechen" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(target.insert).not.toHaveBeenCalled();

    await userEvent.click(within(card("Logs")).getByRole("button", { name: "Ausführen" }));
    const again = screen.getByRole("dialog", { name: "Platzhalter ausfüllen" });
    await userEvent.type(within(again).getByRole("textbox", { name: "unit" }), "nginx");
    await userEvent.type(within(again).getByRole("textbox", { name: "lines" }), "50{Enter}");
    // Review Focus 3: sent only after the dialog (and its focus trap) is gone.
    await waitFor(() => expect(target.run).toHaveBeenCalledWith("journalctl -u nginx -n 50 # nginx"));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("disables insert and run while the tab is not connected", () => {
    render(<SnippetsPanel tab={{ ...sshTab, status: "closed" }} />);
    expect(screen.getByRole("button", { name: "Speicher einfügen" })).toBeDisabled();
    expect(within(card("Speicher")).getByRole("button", { name: "Ausführen" })).toBeDisabled();
  });

  it("opens the editor for new and existing snippets", async () => {
    render(<SnippetsPanel tab={sshTab} />);
    await userEvent.click(screen.getByRole("button", { name: "+ Snippet anlegen" }));
    expect(useUiStore.getState().modal).toEqual({ type: "snippet", hostId: "h1" });
    await userEvent.click(within(card("Nginx")).getByRole("button", { name: "Bearbeiten" }));
    expect(useUiStore.getState().modal).toEqual({ type: "snippet", id: "c" });
  });
});
```

In `ui/src/screens/terminal/TerminalScreen.test.tsx`, after the line that asserts the "Tunnel" tab is disabled, add:

```ts
    await userEvent.click(within(sidebar).getByRole("tab", { name: "Snippets" }));
    expect(within(sidebar).getByRole("searchbox", { name: "Snippets suchen" })).toBeInTheDocument();
    await userEvent.click(within(sidebar).getByRole("tab", { name: "Sitzung" }));
```

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `bun run test ui/src/screens/terminal 2>&1 | tail -15`
Expected: FAIL. `./SnippetsPanel` cannot be resolved, and there is no "Snippets" tab.

- [ ] **Step 3: Implement**

Create `ui/src/screens/snippets/format.ts`:

```ts
import type { Snippet } from "../../ipc/api";

export function firstLine(text: string): string {
  return text.split("\n", 1)[0] ?? "";
}

/** Case-insensitive match on name and command; an empty query matches everything. */
export function matchesQuery(snippet: Snippet, query: string): boolean {
  const needle = query.trim().toLowerCase();
  return !needle || snippet.name.toLowerCase().includes(needle) || snippet.command.toLowerCase().includes(needle);
}
```

Create `ui/src/screens/snippets/PlaceholderDialog.tsx`:

```tsx
import { type FormEvent, useId, useRef, useState } from "react";
import { Button, Dialog, TextField } from "../../components";
import { t } from "../../i18n/de";
import styles from "./Snippets.module.css";

export type SnippetMode = "insert" | "run";

/** One field per placeholder name; values are never stored. */
export function PlaceholderDialog({
  names,
  mode,
  onConfirm,
  onCancel,
}: {
  names: string[];
  mode: SnippetMode;
  onConfirm: (values: Record<string, string>) => void;
  onCancel: () => void;
}) {
  const texts = t.snippets.placeholders;
  const formId = useId();
  const firstRef = useRef<HTMLInputElement>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const submit = (event: FormEvent) => {
    event.preventDefault();
    onConfirm(values);
  };
  return (
    <Dialog
      open
      size="sm"
      title={texts.title}
      onClose={onCancel}
      initialFocusRef={firstRef}
      footer={
        <>
          <Button variant="ghost" onClick={onCancel}>
            {t.common.cancel}
          </Button>
          <Button variant="primary" type="submit" form={formId}>
            {mode === "run" ? texts.run : texts.insert}
          </Button>
        </>
      }
    >
      <form id={formId} className={styles.form} onSubmit={submit}>
        {names.map((name, index) => (
          <TextField
            key={name}
            ref={index === 0 ? firstRef : undefined}
            label={name}
            mono
            value={values[name] ?? ""}
            onChange={(value) => setValues((previous) => ({ ...previous, [name]: value }))}
          />
        ))}
      </form>
    </Dialog>
  );
}
```

Create `ui/src/screens/terminal/sidebar/SnippetsPanel.tsx`:

```tsx
import { Pencil, Play } from "lucide-react";
import { useState } from "react";
import { IconButton, TextField } from "../../../components";
import { t } from "../../../i18n/de";
import type { Snippet } from "../../../ipc/api";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { useSnippetsStore } from "../../../stores/snippets";
import { useUiStore } from "../../../stores/ui";
import { firstLine, matchesQuery } from "../../snippets/format";
import { PlaceholderDialog, type SnippetMode } from "../../snippets/PlaceholderDialog";
import { fillPlaceholders, placeholderNames } from "../../snippets/placeholders";
import styles from "../../snippets/Snippets.module.css";
import { getPasteTarget } from "../pasteTargets";

interface Pending {
  command: string;
  names: string[];
  mode: SnippetMode;
}

/** Sidebar tab "Snippets": the active host's snippets first, then the global ones. */
export function SnippetsPanel({ tab }: { tab: Tab }) {
  const texts = t.snippets;
  const snippets = useSnippetsStore((state) => state.snippets);
  const hosts = useHostsStore((state) => state.hosts);
  const openModal = useUiStore((state) => state.openModal);
  const [query, setQuery] = useState("");
  const [pending, setPending] = useState<Pending | null>(null);

  const hostId = tab.kind === "ssh" ? tab.hostId : undefined;
  const hostName = hosts.find((host) => host.id === hostId)?.name ?? "";
  const ready = tab.status === "connected";
  const visible = [
    ...snippets.filter((snippet) => hostId !== undefined && snippet.hostId === hostId),
    ...snippets.filter((snippet) => snippet.hostId === null),
  ].filter((snippet) => matchesQuery(snippet, query));

  const send = (command: string, mode: SnippetMode) => getPasteTarget(tab.id)?.[mode](command);
  const use = (snippet: Snippet, mode: SnippetMode) => {
    const names = placeholderNames(snippet.command);
    if (names.length > 0) setPending({ command: snippet.command, names, mode });
    else send(snippet.command, mode);
  };
  const confirm = (values: Record<string, string>) => {
    if (!pending) return;
    const { command, mode } = pending;
    setPending(null);
    // Send after the dialog unmounted: its focus trap would pull focus back from the terminal.
    queueMicrotask(() => send(fillPlaceholders(command, values), mode));
  };

  return (
    <div className={styles.panel}>
      <TextField type="search" label={texts.search} value={query} onChange={setQuery} />
      {visible.length === 0 ? (
        <p className={styles.empty}>{snippets.length === 0 ? texts.empty : texts.noMatch}</p>
      ) : (
        <ul className={styles.cards}>
          {visible.map((snippet) => (
            <li key={snippet.id} className={styles.card}>
              <button
                type="button"
                className={styles.insert}
                aria-label={texts.insert(snippet.name)}
                disabled={!ready}
                onClick={() => use(snippet, "insert")}
              >
                <span className={styles.cardHead}>
                  <span className={styles.name}>{snippet.name}</span>
                  <span className={styles.scope}>{snippet.hostId ? hostName : texts.global}</span>
                </span>
                <code className={styles.command}>{firstLine(snippet.command)}</code>
              </button>
              <span className={styles.actions}>
                <IconButton
                  label={texts.run}
                  icon={Play}
                  size="sm"
                  disabled={!ready}
                  onClick={() => use(snippet, "run")}
                />
                <IconButton
                  label={texts.edit}
                  icon={Pencil}
                  size="sm"
                  onClick={() => openModal({ type: "snippet", id: snippet.id })}
                />
              </span>
            </li>
          ))}
        </ul>
      )}
      <button type="button" className={styles.add} onClick={() => openModal({ type: "snippet", hostId: hostId ?? null })}>
        {texts.add}
      </button>
      {pending ? (
        <PlaceholderDialog
          names={pending.names}
          mode={pending.mode}
          onCancel={() => setPending(null)}
          onConfirm={confirm}
        />
      ) : null}
    </div>
  );
}
```

`ui/src/screens/terminal/sidebar/Sidebar.tsx`:

```tsx
import { useId, useState } from "react";
import { type TabItem, tabId, tabPanelId, Tabs } from "../../../components";
import { t } from "../../../i18n/de";
import type { Tab } from "../../../stores/sessions";
import { DesignPanel } from "./DesignPanel";
import { SessionPanel } from "./SessionPanel";
import styles from "./Sidebar.module.css";
import { SnippetsPanel } from "./SnippetsPanel";

type SidebarTab = "snippets" | "session" | "design";

const TABS: readonly TabItem[] = [
  { id: "snippets", label: t.terminal.sidebar.snippets },
  { id: "session", label: t.terminal.sidebar.session },
  { id: "design", label: t.terminal.sidebar.design },
  { id: "tunnel", label: t.terminal.sidebar.tunnel, disabled: true },
];

const PANELS = {
  snippets: SnippetsPanel,
  session: SessionPanel,
  design: DesignPanel,
} satisfies Record<SidebarTab, (props: { tab: Tab }) => React.ReactNode>;

/** Right-hand terminal sidebar for the active tab (Tunnel follows in TP5). */
export function Sidebar({ tab }: { tab: Tab }) {
  const [current, setCurrent] = useState<SidebarTab>("session");
  const idBase = useId();
  const Panel = PANELS[current];
  return (
    <aside className={styles.sidebar} aria-label={t.terminal.sidebar.label}>
      <Tabs
        label={t.terminal.sidebar.label}
        tabs={TABS}
        value={current}
        onChange={(id) => {
          if (id in PANELS) setCurrent(id as SidebarTab);
        }}
        idBase={idBase}
      />
      <div role="tabpanel" id={tabPanelId(idBase, current)} aria-labelledby={tabId(idBase, current)}>
        <Panel tab={tab} />
      </div>
      <p className={styles.hint}>{t.terminal.sidebar.tunnelLater}</p>
    </aside>
  );
}
```

If `React.ReactNode` is not available as a global namespace in this tsconfig, import `type ReactNode` from "react"
and use that instead.

- [ ] **Step 4: Run the tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -8`
Expected: everything passes without warnings or act() noise. If the `searchbox` role is not found, check that
`TextField` forwards `type="search"` to the `<input>`. It spreads the rest props, so the input gets the
`searchbox` role.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/snippets ui/src/screens/terminal
git commit -m "feat(ui): snippets tab in the terminal sidebar with placeholder dialog

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Snippets screen and rail entry

**Files:**
- Create:
  - `ui/src/screens/snippets/SnippetsScreen.tsx`
  - `ui/src/screens/snippets/SnippetsScreen.test.tsx`
- Modify: `ui/src/stores/ui.ts` (`View`), `ui/src/shell/Rail.tsx`, `ui/src/shell/AppShell.tsx`
- Test: `ui/src/shell/Rail.test.tsx`

**Interfaces:**
- Consumes:
  - Task 3: `useSnippetsStore`.
  - Task 5: `t.snippets`, `Snippets.module.css`, `ModalState` `"snippet"`.
  - Task 6: `firstLine`, `matchesQuery`.
- Produces: `View` now includes `"snippets"`.

- [ ] **Step 1: Write the failing tests**

Create `ui/src/screens/snippets/SnippetsScreen.test.tsx`:

```tsx
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useHostsStore } from "../../stores/hosts";
import { useSnippetsStore } from "../../stores/snippets";
import { useUiStore } from "../../stores/ui";
import { makeHost, makeSnippet } from "../../test/fixtures";
import { SnippetsScreen } from "./SnippetsScreen";

vi.mock("../../ipc/api");

const rowNames = () =>
  screen
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("cell")[0]?.textContent);

beforeEach(() => {
  useUiStore.setState(useUiStore.getInitialState(), true);
  useHostsStore.setState({ ...useHostsStore.getInitialState(), hosts: [makeHost({ id: "h1", name: "web" })] }, true);
  useSnippetsStore.setState({
    snippets: [
      makeSnippet({ id: "a", name: "Deploy", command: "git pull\nmake", hostId: "h1" }),
      makeSnippet({ id: "b", name: "Speicher", command: "df -h", hostId: null }),
    ],
    loaded: true,
  });
});

describe("SnippetsScreen", () => {
  it("lists all snippets with scope and first command line", () => {
    render(<SnippetsScreen />);
    expect(rowNames()).toEqual(["Deploy", "Speicher"]);
    const deploy = screen.getAllByRole("row")[1] as HTMLElement;
    expect(within(deploy).getByText("web")).toBeInTheDocument();
    expect(within(deploy).getByText("git pull")).toBeInTheDocument();
  });

  it("filters by scope and search", async () => {
    render(<SnippetsScreen />);
    await userEvent.click(screen.getByRole("radio", { name: "Global" }));
    expect(rowNames()).toEqual(["Speicher"]);
    await userEvent.click(screen.getByRole("radio", { name: "Host" }));
    expect(rowNames()).toEqual(["Deploy"]);
    await userEvent.click(screen.getByRole("radio", { name: "Alle" }));
    await userEvent.type(screen.getByRole("searchbox", { name: "Snippets suchen" }), "df");
    expect(rowNames()).toEqual(["Speicher"]);
  });

  it("opens the editor for a row and for a new global snippet", async () => {
    render(<SnippetsScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Deploy" }));
    expect(useUiStore.getState().modal).toEqual({ type: "snippet", id: "a" });
    await userEvent.click(screen.getByRole("button", { name: "+ Snippet anlegen" }));
    expect(useUiStore.getState().modal).toEqual({ type: "snippet", hostId: null });
  });

  it("shows an empty state", () => {
    useSnippetsStore.setState({ snippets: [] });
    render(<SnippetsScreen />);
    expect(screen.getByText("Noch keine Snippets")).toBeInTheDocument();
  });
});
```

In `ui/src/shell/Rail.test.tsx`:
- In "navigates between the views", append:
  ```ts
      await userEvent.click(screen.getByRole("button", { name: "Snippets" }));
      expect(useUiStore.getState().view).toBe("snippets");
  ```
- In "shows later features as disabled with a tooltip", change the list to `["Schlüsselbund", "Port-Forwarding"]`.

Check how `Segmented` renders its options first (`grep -n "role=" ui/src/components/Segmented/Segmented.tsx`). The
test expects `role="radio"` buttons. If it uses another role, adjust the three `getByRole("radio", …)` queries to
that role.

- [ ] **Step 2: Run the tests and verify that they fail**

Run: `bun run test ui/src/screens/snippets/SnippetsScreen.test.tsx ui/src/shell/Rail.test.tsx 2>&1 | tail -15`
Expected: FAIL. `./SnippetsScreen` cannot be resolved, and the rail "Snippets" entry is still disabled.

- [ ] **Step 3: Implement**

`ui/src/stores/ui.ts`: `export type View = "hosts" | "terminal" | "sftp" | "snippets";`

`ui/src/shell/Rail.tsx`:
- Add `{ view: "snippets", label: t.rail.snippets, icon: CodeXml },` as the last entry of `VIEW_ITEMS`.
- Remove the snippets entry from `LATER_ITEMS`.

`ui/src/shell/AppShell.tsx`:
- Import `SnippetsScreen` from `../screens/snippets/SnippetsScreen`.
- After the sftp section, add:

```tsx
        <section className={styles.view} hidden={view !== "snippets"}>
          <SnippetsScreen />
        </section>
```

Update the component doc: "Rail + the views + modal layer."

Create `ui/src/screens/snippets/SnippetsScreen.tsx`:

```tsx
import { useState } from "react";
import { Button, Segmented, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { Id, Snippet } from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { useSnippetsStore } from "../../stores/snippets";
import { useUiStore } from "../../stores/ui";
import { firstLine, matchesQuery } from "./format";
import styles from "./Snippets.module.css";

type ScopeFilter = "all" | "global" | "host";

const inScope = (snippet: Snippet, filter: ScopeFilter) =>
  filter === "all" || (filter === "global") === (snippet.hostId === null);

/** Rail screen "Snippets": manage every snippet; inserting happens in the terminal sidebar. */
export function SnippetsScreen() {
  const texts = t.snippets;
  const snippets = useSnippetsStore((state) => state.snippets);
  const hosts = useHostsStore((state) => state.hosts);
  const openModal = useUiStore((state) => state.openModal);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<ScopeFilter>("all");

  const scopeLabel = (hostId: Id | null) =>
    hostId === null ? texts.global : (hosts.find((host) => host.id === hostId)?.name ?? "");
  const rows = snippets.filter((snippet) => inScope(snippet, filter) && matchesQuery(snippet, query));

  return (
    <div className={styles.screen}>
      <header className={styles.header}>
        <h1 className={styles.title}>{texts.title}</h1>
        <TextField type="search" label={texts.search} value={query} onChange={setQuery} />
        <Segmented
          label={texts.filter.label}
          size="sm"
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: texts.filter.all },
            { value: "global", label: texts.filter.global },
            { value: "host", label: texts.filter.host },
          ]}
        />
        <Button variant="primary" onClick={() => openModal({ type: "snippet", hostId: null })}>
          {texts.add}
        </Button>
      </header>
      {rows.length === 0 ? (
        <p className={styles.empty}>{snippets.length === 0 ? texts.empty : texts.noMatch}</p>
      ) : (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>{texts.columns.name}</th>
              <th>{texts.columns.scope}</th>
              <th>{texts.columns.command}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((snippet) => (
              <tr key={snippet.id}>
                <td>
                  <button
                    type="button"
                    className={styles.link}
                    onClick={() => openModal({ type: "snippet", id: snippet.id })}
                  >
                    {snippet.name}
                  </button>
                </td>
                <td>{scopeLabel(snippet.hostId)}</td>
                <td>
                  <code className={styles.command}>{firstLine(snippet.command)}</code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run all UI checks**

Run: `bun run typecheck && bun run test 2>&1 | tail -8`
Expected: everything passes without warnings. If `AppShell.test.tsx` counts the rail buttons or views, update the
expected count.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/snippets ui/src/stores/ui.ts ui/src/shell
git commit -m "feat(ui): snippets screen and rail entry

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Acceptance checklist, docs, full verification

**Files:**
- Create: `docs/acceptance-tp2b.md`
- Modify: `CLAUDE.md` (layout list and acceptance pointer)

- [ ] **Step 1: Write `docs/acceptance-tp2b.md`**

```markdown
# TP2b acceptance: Snippets

Before the first start of this build: `cp ~/.local/share/linascp/linascp.db ~/.local/share/linascp/linascp.db.before-tp2b`
(migration V3; older builds refuse the migrated DB).

## Sidebar
- [ ] Terminal → open the sidebar (Ctrl+Shift+B) → tab "Snippets" is first; "Sitzung" is still selected by default.
- [ ] "+ Snippet anlegen" in an SSH tab: scope is preset to the host; save → card appears at the top.
- [ ] In a tab of a host from `~/.ssh/config` the scope is preset to "Global".
- [ ] Click on a card inserts the command at the prompt without running it; the terminal has focus.
- [ ] ▶ runs the command (Enter is sent after the text).
- [ ] Multi-line snippet with bracketed paste (bash ≥ 5.1 / zsh): click inserts all lines without running; ▶ runs them.
- [ ] Snippet with `{{unit}}` twice and `{{lines}}`: dialog shows two fields; Enter confirms; Esc cancels and sends nothing.
- [ ] Search filters by name and command; "Keine Treffer" without matches.
- [ ] Local terminal shows only global snippets.
- [ ] Disconnected tab: card and ▶ are disabled.

## Screen
- [ ] Rail "Snippets" is active and opens the screen with all snippets (name, scope, first command line).
- [ ] Filter Alle / Global / Host and search work; clicking a name opens the editor.
- [ ] Editor: empty name/command shows the field errors; a 9 KB command shows a red toast and the dialog stays open.
- [ ] Delete asks "Snippet wirklich löschen?" first.

## Data
- [ ] Delete a host that owns snippets → its snippets disappear from sidebar and screen.
- [ ] Export → import into a fresh profile: snippets come back, bound to their host.
- [ ] Import the export again: snippets show up as conflicts ("Snippet", gleiche ID).
- [ ] An export from v0.1.0 (no snippets) still imports.
```

- [ ] **Step 2: Update `CLAUDE.md`**

In the "Layout" list, after the TP2a spec bullet, add:

```markdown
- `docs/superpowers/specs/2026-10-15-linascp-tp2b-snippets-design.md`: TP2b spec (snippets).
```

Change the acceptance bullet to:
`` - `docs/acceptance-tp1.md`: manual acceptance checklist (`docs/acceptance-tp2a.md`, `docs/acceptance-tp2b.md` for TP2a/TP2b). ``

- [ ] **Step 3: Full verification**

Run:
```bash
export PATH=$HOME/.cargo/bin:$PATH
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings \
  && SHELL=/bin/sh cargo test --workspace 2>&1 | grep -E "test result|FAILED|warning" \
  && bun run typecheck && bun run test 2>&1 | tail -6 \
  && git status --short ui/src/ipc/bindings.ts
```
Expected:
- fmt and clippy are clean;
- every Rust `test result: ok`;
- the UI tests pass;
- `bindings.ts` is unchanged, because it was already committed in Task 3.

- [ ] **Step 4: Commit**

```bash
git add docs/acceptance-tp2b.md CLAUDE.md
git commit -m "docs: TP2b acceptance checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
