# LinaSCP TP3c (Bookmarks + Edit) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Folder bookmarks in both SFTP panes, and editing remote files in a local editor with automatic upload on save.

**Architecture:** Bookmarks are a new SQLite table (migration V4) behind three Tauri commands and a pane-header menu.
Editing is a core `EditManager` per SFTP session that downloads into `~/.cache/linascp/edit/<uuid>/`, polls the
local copy and uploads changes through the TP3b `TransferQueue` (fixed `overwrite` decision and fixed file mode),
after checking the remote stamp. The editor is launched by a `Launcher` trait implemented in `src-tauri` with gio.

**Tech Stack:** Rust (rusqlite, tokio, async-trait, russh-sftp), Tauri 2 + specta, `gio` 0.18 (already in
`Cargo.lock`), React 19 + zustand + Vitest.

**Spec:** `docs/superpowers/specs/2026-10-16-linascp-tp3c-bookmarks-edit-design.md`

## Global Constraints

- Toolchain: `export PATH=$HOME/.cargo/bin:$PATH` before every cargo command (system rustc 1.85 is too old).
- Code, comments, identifiers, commits in English; UI strings German, only in `ui/src/i18n/de.ts` under
  `t.sftp.bookmarks`, `t.sftp.edit` (settings label under the existing settings section).
- `ui/src/ipc/bindings.ts` is generated: `cargo test -p linascp exports_bindings`. Every command is wrapped in
  `ui/src/ipc/api.ts`.
- TDD; test output free of warnings (incl. act()/jsdom noise). Checks: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `bun run typecheck && bun run test`.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Never edit a released migration; V4 is appended.
- No new crates beyond `gio = "0.18"` in `src-tauri` (exact version already locked).
- Local edit copies: directories `0700`, file mode `remote & 0o666 | 0o600`. Poll interval 1 s in production.
- Bookmark path: absolute, ≤ 4096 bytes, no control chars. Editor command: non-empty after trim, ≤ 512 bytes, no
  control chars.

## Review Focus

- A remote file that is a symlink: upload must land on the link target, the link stays a link (test in Task 5).
- Saving while the session reconnects: the upload waits and happens once connected, never lost (test in Task 5:
  `upload_waits_while_disconnected`).
- Editor saves via rename-replace (new inode): polling by path still sees it (test in Task 5:
  `rename_replace_save_is_detected`).
- Two quick saves during one upload: the second state is uploaded too (test in Task 5: `change_during_upload_uploads_again`).
- Closing the tab with an unsaved-to-server change never deletes the local copy (tests in Task 5 and Task 8).

---

### Task 1: Bookmarks in core

**Files:**
- Modify: `crates/core/src/store/migrations.rs` (V4), `crates/core/src/store/mod.rs`, `crates/core/src/model.rs`,
  `crates/core/src/validate.rs`
- Test: `crates/core/src/store/tests.rs`, `crates/core/src/store/migrations.rs` (tests module), `validate.rs` tests

**Interfaces:**
- Produces:
  - `model::Bookmark { id: Id, host_id: Option<Id>, path: String, position: i64, created_at: i64 }` with
    `Serialize, Deserialize, specta::Type`, `rename_all = "camelCase"`.
  - `Store::list_bookmarks(&self, host_id: Option<Id>) -> Result<Vec<Bookmark>>` (ordered by `position`)
  - `Store::add_bookmark(&self, host_id: Option<Id>, path: &str) -> Result<Bookmark>` (validates; duplicate path for
    the same `host_id` returns the existing row; new rows get `max(position)+1`)
  - `Store::remove_bookmark(&self, id: Id) -> Result<()>` (unknown id is Ok)
  - `validate::validate_bookmark_path(path: &str) -> Result<()>` (`CoreError::config` on failure)

- [ ] **Step 1: Write failing tests**
  - `migrations::tests::v4_adds_bookmarks_to_a_v3_database`: build V1–V3, set `user_version = 3`, `run`, insert into
    `bookmarks`, `user_version == LATEST_VERSION == 4`.
  - `store::tests`: `bookmarks_are_kept_per_host_and_local` (local `None` and host A/B lists are disjoint),
    `adding_a_bookmark_twice_returns_the_existing_one` (same id, list length 1), `bookmarks_keep_insertion_order`,
    `removing_an_unknown_bookmark_is_ok`, `deleting_a_host_deletes_its_bookmarks` (local ones stay),
    `bookmark_paths_are_validated` (`"rel/x"`, `"/a\nb"`, 4097-byte path → Err).
  - `validate` tests for `validate_bookmark_path`: `/` ok, `/srv/www` ok, `""`, `"x"`, `"/a\u{7}"` err.
- [ ] **Step 2: Run** `cargo test -p linascp-core bookmark` — Expected: FAIL (does not compile: missing items).
- [ ] **Step 3: Implement** V4 exactly as the spec's SQL; `delete_host` deletes `bookmarks WHERE host_id = ?` in
  the same transaction.
- [ ] **Step 4: Run** `cargo test -p linascp-core` — Expected: PASS, no warnings.
- [ ] **Step 5: Commit** `feat(core): bookmarks table and store API`

### Task 2: Bookmark commands

**Files:**
- Create: `src-tauri/src/commands/bookmarks.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (command registration / specta builder),
  `ui/src/ipc/bindings.ts` (generated), `ui/src/ipc/api.ts`

**Interfaces:**
- Consumes: Task 1 store API.
- Produces: commands `bookmarks_list(host_id: Option<Id>) -> Vec<Bookmark>`,
  `bookmark_add(host_id: Option<Id>, path: String) -> Bookmark`, `bookmark_remove(id: Id) -> ()`;
  api wrappers `bookmarksList(hostId: string | null)`, `bookmarkAdd(hostId: string | null, path: string)`,
  `bookmarkRemove(id: string)`.

- [ ] **Step 1: Failing test** in `bookmarks.rs` (same pattern as `commands/snippets.rs` tests with a temp
  `AppState`): `bookmark_commands_roundtrip` (add local + host, list each, remove) and
  `bookmark_add_rejects_relative_paths`.
- [ ] **Step 2: Run** `cargo test -p linascp bookmark` — Expected: FAIL (compile).
- [ ] **Step 3: Implement** thin `_inner` functions + `#[tauri::command] #[specta::specta]` wrappers; register.
- [ ] **Step 4: Run** `cargo test -p linascp` (incl. `exports_bindings`), then `bun run typecheck` — Expected: PASS.
- [ ] **Step 5: Commit** `feat(tauri): bookmark commands`

### Task 3: Bookmarks UI

**Files:**
- Create: `ui/src/stores/bookmarks.ts`, `ui/src/stores/bookmarks.test.ts`,
  `ui/src/screens/sftp/BookmarkMenu.tsx`, `ui/src/screens/sftp/BookmarkMenu.test.tsx`
- Modify: `ui/src/screens/sftp/FilePane.tsx` (header button, `Ctrl+B`), `FilePane.module.css`, `ui/src/i18n/de.ts`

**Interfaces:**
- Consumes: Task 2 api wrappers.
- Produces: `useBookmarksStore` with `byKey: Record<string, Bookmark[]>` (key `"local"` or the host id),
  `load(key)`, `toggle(key, path)` (add or remove the bookmark of `path`), `remove(key, id)`. Pane passes
  `key = side === "local" ? "local" : tab.hostId`.

- [ ] **Step 1: Failing tests**
  - Store: `load` fills `byKey`; `toggle` adds when absent and removes when present (calls `bookmarkAdd` /
    `bookmarkRemove`); failures surface via the existing notices store, no unhandled rejection.
  - `BookmarkMenu.test.tsx`: button `"Lesezeichen"` opens `role="menu"`; empty shows `"Noch keine Lesezeichen"`;
    `"Diesen Ordner merken"` calls `bookmarkAdd(hostId, "/srv")`; when `/srv` is bookmarked the item reads
    `"Lesezeichen entfernen"` and the button has `aria-pressed="true"`; clicking a bookmark calls the store
    `navigate(tabId, side, path)`; per-row remove button `"{name} entfernen"`; ArrowDown/ArrowUp move focus; `Esc`
    closes and focuses the button.
  - `FilePane.test.tsx`: `Ctrl+B` in the remote grid toggles the bookmark for the current path; local pane uses key
    `"local"` (`bookmarkAdd(null, "/l")`).
- [ ] **Step 2: Run** `bun run test -- bookmarks BookmarkMenu FilePane` — Expected: FAIL.
- [ ] **Step 3: Implement.** Menu label = last path segment (`"/"` for root), `title` = full path. Reuse the
  context menu's CSS classes (`styles.menu`, `styles.menuItem`). Icon `Bookmark` from lucide (`fill="currentColor"`
  when pressed). Strings: `t.sftp.bookmarks = { button: "Lesezeichen", add: "Diesen Ordner merken",
  remove: "Lesezeichen entfernen", empty: "Noch keine Lesezeichen", removeOne: (name) => `${name} entfernen` }`.
- [ ] **Step 4: Run** `bun run typecheck && bun run test` — Expected: PASS, no warnings.
- [ ] **Step 5: Commit** `feat(ui): bookmark menu in both panes`

### Task 4: Engine hooks for editing

**Files:**
- Modify: `crates/core/src/transfer/queue.rs`, `crates/core/src/transfer/mod.rs`, `crates/core/src/transfer/side.rs`,
  `crates/core/src/transfer/local_side.rs`, the `Side` impl for `SftpSession` (in `crates/core/src/sftp/mod.rs`),
  `crates/core/src/transfer/test_support.rs` (fault wrapper forwards `realpath`)
- Test: queue tests (existing test module), `local_side.rs` tests

**Interfaces:**
- Produces:
  - `transfer::EnqueueOptions { pub decision: Option<Decision>, pub file_mode: Option<u32> }` (`Default`).
  - `TransferQueue::enqueue_with(&self, direction, sources: Vec<String>, target_dir: String, opts: EnqueueOptions) -> Result<BatchId>`;
    `enqueue` = `enqueue_with(.., EnqueueOptions::default())`. `decision` initialises `Batch::default`;
    `file_mode` (masked `& 0o7777`) replaces `options.file_mode(..)` for file jobs of that batch.
  - `TransferQueue::wait_batch(&self, id: BatchId) -> Vec<Job>`: returns once every job of the batch is final or the
    queue is closed; unknown id returns `vec![]` immediately.
  - `TransferQueue::remote(&self) -> Option<Arc<dyn Side>>` (current connection, `None` while disconnected).
  - `Side::realpath(&self, path: &str) -> Result<String>` (LocalSide: `tokio::fs::canonicalize`, UTF-8 or
    `CoreError::config`; SftpSession: SFTP `realpath`).

- [ ] **Step 1: Failing tests**
  - `enqueue_with_overwrite_never_asks`: target exists, `decision: Some(Overwrite)` → no `Conflict` event, content
    replaced.
  - `enqueue_with_file_mode_sets_exactly_that_mode`: upload with `upload_mode = Some(0o600)` in options and
    `file_mode: Some(0o640)` → target mode `0o640`.
  - `wait_batch_returns_final_jobs` (state `Done`), `wait_batch_of_unknown_batch_is_empty`,
    `wait_batch_returns_on_close`.
  - `local_realpath_resolves_symlinks`.
- [ ] **Step 2: Run** `cargo test -p linascp-core transfer` — Expected: FAIL (compile).
- [ ] **Step 3: Implement.** For waiting, a `tokio::sync::Notify` notified (`notify_waiters`) whenever a job reaches a
  final state or the queue closes; `wait_batch` loops: register `notified()`, check state, await.
- [ ] **Step 4: Run** `cargo test -p linascp-core` and `cargo clippy --workspace --all-targets -- -D warnings` —
  Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): enqueue options, wait_batch and realpath for editing`

### Task 5: EditManager in core

**Files:**
- Create: `crates/core/src/edit/mod.rs` (types, manager), `crates/core/src/edit/tests.rs`
- Modify: `crates/core/src/lib.rs` (`pub mod edit;`), `crates/core/tests/ssh_integration.rs` (Docker test)

**Interfaces:**
- Consumes: Task 4 (`enqueue_with`, `wait_batch`, `remote`, `realpath`).
- Produces:
  - `pub type EditId = u64;`
  - `#[serde(rename_all = "camelCase")] pub enum EditState { Downloading, Synced, Changed, Uploading, Conflict, Failed }`
  - `pub struct EditFile { pub id: EditId, pub name: String, pub remote_path: String, pub state: EditState, pub dirty: bool, pub error: Option<CoreError> }`
    (camelCase, `specta::Type`)
  - `#[serde(tag = "kind", rename_all = "camelCase")] pub enum EditEvent { File { file: EditFile } }`
  - `pub trait Launcher: Send + Sync { fn open(&self, path: &Path) -> Result<()>; }`
  - `pub type EditEmit = Arc<dyn Fn(EditEvent) + Send + Sync>;`
  - `EditManager::new(queue: TransferQueue, root: PathBuf, launcher: Arc<dyn Launcher>, emit: EditEmit, poll: Duration) -> Self` (cheap `Clone`)
  - `async fn open(&self, remote_path: &str) -> Result<EditId>`: returns after the download finished and the
    launcher ran (download failure → `Err` and the file is emitted as `Failed`).
  - `fn resolve(&self, id: EditId, overwrite: bool) -> Result<()>`, `fn reopen(&self, id: EditId) -> Result<()>`,
    `async fn stop(&self, id: EditId) -> Result<Option<PathBuf>>`, `async fn close(&self) -> Vec<PathBuf>`.

Behaviour (from the spec, §2 Ablauf): realpath + stat (must be `NodeKind::File`, else `Fs { kind: NotAFile }`-style
config error); same canonical path already open → only `launcher.open` again, same id; download via
`enqueue_with(Download, [real], dir, default)` + `wait_batch`; chmod copy `remote & 0o666 | 0o600`; remember remote
`FileStamp` and the local stamp `(len, mtime as SystemTime)`; one poll task per file at `poll`: change seen →
`Changed`, `dirty = true`; unchanged for one further tick → upload step: `queue.remote()` is `None` → stay
`Changed`, retry next tick; stat remote; stamp differs from remembered (and file exists) → `Conflict`, wait for
`resolve`; else `Uploading`, `enqueue_with(Upload, [local], parent(real), { decision: Some(Overwrite),
file_mode: Some(current or remembered remote mode & 0o777) })` + `wait_batch`; job `Done` → stat remote, remember
stamp, `Synced`, `dirty = false` unless the copy changed meanwhile (then `Changed` again); job failed → `Failed` with
its error, `dirty` stays true. `resolve(false)` → back to `Changed`-but-idle until the next local change (stamp not
taken). `stop`: abort poll task; `dirty` → keep dir, return `Some(path_of_copy)`, else `remove_dir_all` and `None`.
`close`: one last poll per file, then `stop` for all.

- [ ] **Step 1: Failing tests** (`edit/tests.rs`, two `LocalSide`s in tempdirs: "remote" root and edit root, fake
  launcher counting calls, `poll = 20 ms`, helper `wait_for_state(events, id, state)` with a timeout):
  - `open_downloads_a_private_copy_and_launches_once` (content equal, copy mode of a remote `0755` file is `0644`,
    edit dir and its parent `0700`, launcher calls == 1, state `Synced`, `dirty == false`)
  - `opening_the_same_file_again_only_relaunches` (same id, one download batch, launcher calls == 2)
  - `local_change_is_uploaded_and_keeps_the_remote_mode` (remote `0640` stays `0640`, content equal)
  - `rename_replace_save_is_detected` (write `x.tmp`, rename over the copy)
  - `change_during_upload_uploads_again` (fault wrapper delays the upload; final remote content = second write)
  - `remote_change_asks_and_overwrite_uploads`, `keep_does_not_upload_and_asks_again_on_next_change`
  - `deleted_remote_file_is_recreated`
  - `symlinked_remote_file_stays_a_link` (`link -> real.txt`; after upload `link` is still a symlink, `real.txt` has
    new content)
  - `upload_waits_while_disconnected` (`set_remote(None)`, change, nothing uploaded; `set_remote(Some)` → uploaded)
  - `stop_after_upload_removes_the_copy`, `stop_with_dirty_copy_keeps_it_and_returns_its_path`,
    `close_keeps_only_dirty_copies`
  - `opening_a_directory_fails`
- [ ] **Step 2: Run** `cargo test -p linascp-core edit` — Expected: FAIL (compile).
- [ ] **Step 3: Implement** `edit/mod.rs` per the behaviour above. State in `Mutex<HashMap<EditId, FileState>>`;
  each file's poll loop is a spawned task whose `AbortHandle` lives in its state; `resolve` signals through a
  per-file `Notify` or `oneshot`.
- [ ] **Step 4: Run** `cargo test -p linascp-core` — Expected: PASS, no warnings, stable over 5 runs
  (`for i in 1 2 3 4 5; do cargo test -p linascp-core edit -q || break; done`).
- [ ] **Step 5: Docker test** `edit_roundtrip_against_sshd` in `ssh_integration.rs` (`#[ignore]`): open a `0640` file
  through a real `SftpSession`-backed queue, change the copy, wait for `Synced`, read it back over SFTP, mode still
  `0640`. Run: `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1` — Expected: all pass.
- [ ] **Step 6: Commit** `feat(core): edit remote files with automatic upload`

### Task 6: Editor setting, launcher and edit commands

**Files:**
- Create: `src-tauri/src/editor.rs` (`SystemLauncher`), `src-tauri/src/commands/sftp_edit.rs`
- Modify: `src-tauri/Cargo.toml` (`gio = "0.18"`), `src-tauri/src/dto.rs` (`Settings.editor_command`),
  `src-tauri/src/commands/settings.rs`, `crates/core/src/store/mod.rs` (`pub const EDITOR_COMMAND: &str = "editor_command";`),
  `crates/core/src/validate.rs` (`validate_editor_command`), `src-tauri/src/commands/sftp.rs` (`SftpEntry.edits`,
  `SftpEvent::Edit { event: EditEvent }`, `sftp_close -> Vec<String>`), `crates/core/src/paths.rs`
  (`edit_dir()` = `~/.cache/linascp/edit`, respects `XDG_CACHE_HOME`), `src-tauri/src/lib.rs`, bindings, `ui/src/ipc/api.ts`

**Interfaces:**
- Consumes: Task 5 `EditManager`, `Launcher`, `EditEvent`.
- Produces:
  - `Settings.editor_command: Option<String>` (TS `editorCommand: string | null`); empty string saved as `None`.
  - `SystemLauncher { store }` implementing `Launcher`: reads `editor_command` on every `open`. `None` →
    `gio::AppInfo::default_for_type("text/plain", false)` + `launch_uris(&[file_uri], None::<&gio::AppLaunchContext>)`;
    no app → `CoreError::config("no text editor found")`. `Some(cmd)` → `split_whitespace`, spawn
    `Command::new(first).args(rest).arg(path)` with null stdio, not awaited.
  - `pub(crate) fn editor_argv(cmd: &str, path: &Path) -> Vec<OsString>` (pure, tested).
  - Commands: `edit_open(session_id, path: String) -> EditId`, `local_edit(path: String) -> ()`,
    `edit_resolve(session_id, edit_id: EditId, overwrite: bool)`, `edit_reopen(session_id, edit_id)`,
    `edit_stop(session_id, edit_id) -> Option<String>`; `sftp_close(session_id) -> Vec<String>` (kept copies).
  - api: `editOpen`, `localEdit`, `editResolve`, `editReopen`, `editStop`; `sftpClose` now `Promise<string[]>`.
- `local_edit` checks: absolute path, exists, is a file; then `SystemLauncher::open`.

- [ ] **Step 1: Failing tests**
  - `validate_editor_command`: `"code -w"` ok, `""`, `"   "`, `"a\nb"`, 513 bytes err.
  - `editor_argv("code  -w", "/t/a b.txt")` == `["code", "-w", "/t/a b.txt"]`.
  - settings: `editor_command_roundtrips_and_empty_is_none`.
  - `sftp_edit.rs`: `unknown_sessions_and_edit_ids_are_errors` (open/resolve/reopen/stop),
    `local_edit_rejects_relative_and_missing_paths`.
  - `sftp_events_serialize_for_the_ui` extended: `SftpEvent::Edit` serialises as
    `{"kind":"edit","event":{"kind":"file","file":{...,"remotePath":...,"dirty":false}}}`.
- [ ] **Step 2: Run** `cargo test -p linascp` — Expected: FAIL (compile).
- [ ] **Step 3: Implement.** `new_entry` builds `EditManager::new(queue.clone(), paths.edit_dir(),
  Arc::new(SystemLauncher::new(store)), emit→SftpEvent::Edit, Duration::from_secs(1))`. `sftp_close`: `edits.close()`
  first, then queue and conn as before; returns the kept paths as strings.
- [ ] **Step 4: Run** `cargo test -p linascp` (regenerates bindings), `cargo clippy --workspace --all-targets -- -D warnings`,
  `bun run typecheck` — Expected: PASS (fix the `sftpClose` callers' types only; behaviour comes in Task 7).
- [ ] **Step 5: Commit** `feat(tauri): editor launcher, editor setting and edit commands`

### Task 7: Edit UI

**Files:**
- Create: `ui/src/stores/edits.ts`, `ui/src/stores/edits.test.ts`, `ui/src/screens/sftp/EditConflictDialog.tsx`
  (+ test)
- Modify: `ui/src/stores/sftp.ts` (route `edit` events, `closeTab` handles kept paths), `ui/src/screens/sftp/FilePane.tsx`
  (`F4`, `Enter`/double-click on files, menu item), `ui/src/screens/sftp/SftpScreen.tsx` (toolbar „Bearbeiten“,
  dialog, close count, tab dot), `ui/src/screens/sftp/TransferPanel.tsx` (tab „Bearbeitet“),
  `ui/src/screens/settings/*` (field „Editor“), `ui/src/i18n/de.ts`, related tests

**Interfaces:**
- Consumes: Task 6 api + `EditFile` binding types.
- Produces: `useEditsStore` with `byTab: Record<string, EditFile[]>`, `onEvent(tabId, event)`, `dirtyCount(tabId)`,
  `conflictOf(tabId): EditFile | undefined`, `drop(tabId)`. `FilePane` prop `onEdit(name: string)`.

- [ ] **Step 1: Failing tests**
  - `edits.test.ts`: events upsert by id; `dirtyCount`; `conflictOf` returns the first `conflict` file.
  - `FilePane.test.tsx`: `F4`, `Enter` and double-click on a file call `onEdit("a.txt")`; `Enter` on a dir still
    navigates; symlink to file edits, symlink to dir navigates; lossy names do nothing; context menu item
    „Bearbeiten“.
  - `SftpScreen.test.tsx`: remote `onEdit` → `editOpen("s1", "/srv/a.txt")`; local → `localEdit("/l/a.txt")`;
    toolbar „Bearbeiten“ disabled while not connected (remote active pane); `editOpen` rejection shows a notice;
    close dialog counts dirty edits („Noch 1 Übertragungen offen“ with zero jobs and one dirty file); after
    `sftpClose` resolves `["/c/x/a.txt"]` a notice contains that path.
  - `EditConflictDialog.test.tsx`: title „Datei auf dem Server geändert“, text with name, focus on
    „Nicht hochladen“, `Esc` → `editResolve(sid, id, false)`, „Überschreiben“ → `editResolve(sid, id, true)`; only
    in the active tab, other tab gets the dot (reuse the TP3b dot condition: conflict OR edit conflict).
  - `TransferPanel.test.tsx`: tab „Bearbeitet“ with counter; row status texts („Synchron“, „Geändert“,
    „Wird hochgeladen“, „Auf dem Server geändert“, error text via `t.sftp.fsErrors`); „Im Editor öffnen“ →
    `editReopen`; „Bearbeitung beenden“ → `editStop`, a returned path shows the kept-copy notice; panel visible with
    only edits.
  - Settings: field „Editor“ (placeholder „Standard-Texteditor des Systems“) saves `editorCommand`, empty → `null`.
- [ ] **Step 2: Run** `bun run test` — Expected: the new tests FAIL.
- [ ] **Step 3: Implement.** Strings under `t.sftp.edit`: `button: "Bearbeiten"`, `tab: "Bearbeitet"`,
  states as above, `reopen: "Im Editor öffnen"`, `stop: "Bearbeitung beenden"`,
  `conflictTitle: "Datei auf dem Server geändert"`,
  `conflictText: (name) => `${name} wurde auf dem Server geändert, seit sie geöffnet wurde. Mit deiner Version überschreiben?``,
  `keep: "Nicht hochladen"`, `overwrite: "Überschreiben"`,
  `kept: (path) => `Nicht hochgeladene Änderungen liegen unter ${path}``,
  `noEditor: "Kein Editor gefunden. In den Einstellungen festlegen."` (shown when the error is
  `config` from the launcher), settings `editor: "Editor"`, `editorPlaceholder: "Standard-Texteditor des Systems"`,
  `editorHint: "Befehl, die Datei wird angehängt, z. B. code -w"`.
- [ ] **Step 4: Run** `bun run typecheck && bun run test` — Expected: PASS, no warnings.
- [ ] **Step 5: Commit** `feat(ui): edit files from both panes with upload status and conflict dialog`

### Task 8: Docs and full verification

**Files:**
- Create: `docs/acceptance-tp3c.md`
- Modify: `CLAUDE.md` (layout: `edit/`, spec + acceptance list, roadmap „3c bookmarks + edit done“)

- [ ] **Step 1:** Acceptance checklist: bookmarks (add/remove/navigate/Ctrl+B, survive restart, local vs host,
  ssh_config host), edit (GNOME text editor opens, save uploads, rights unchanged `stat -c %a`, symlinked config
  stays a link, conflict with `echo >> file` from a terminal, „Nicht hochladen“ asks again, Wi‑Fi off during save
  uploads after reconnect, close tab with dirty file keeps the copy and names the path, custom `editorCommand`,
  local F4 opens the local file, a `.desktop`/script file opens as text and never runs).
- [ ] **Step 2: Run all checks:** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings &&
  cargo test --workspace && bun run typecheck && bun run test` and the Docker suite — Expected: all pass, no warnings.
- [ ] **Step 3: Commit** `docs: TP3c acceptance checklist and project notes`
