# LinaSCP TP3a SFTP Browse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open a host over SFTP into a two-pane commander (local | remote) with navigation, sorting, hidden files, new folder, rename, delete (local: trash), full keyboard control and a "connection lost" banner with manual reconnect.

**Architecture:** Core gets shared listing types and local file operations in `crates/core/src/sftp/`, and an
`SftpSession` (remote operations over `russh_sftp::client::RawSftpSession`) in `crates/core/src/connect/sftp_session.rs`.
`connect_sftp` reuses `Pipeline::establish` and adds the steps `Subsystem` and `Directory`. `src-tauri` keeps SFTP
sessions in their own map and exposes `connect_sftp`, `sftp_*` and `local_*` commands. The UI adds `useSftpStore`,
routes `useConnectStore.start(hostId, "sftp")` into it, and renders `SftpScreen` with two `FilePane`s.

**Tech Stack:** Rust (russh 0.64, russh-sftp 3.0, libc, tokio), Tauri 2 + tauri-specta, React 19 + TypeScript strict + zustand, Vitest + Testing Library.

**Spec:** `docs/superpowers/specs/2026-10-15-linascp-tp3a-sftp-browse-design.md`

## Global Constraints

- Chat German; code, comments, identifiers, commit messages English; UI strings German, only in `ui/src/i18n/de.ts`.
- TDD: failing test first, watched failing. Test output free of warnings (incl. act() and jsdom noise).
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Toolchain first: `export PATH=$HOME/.cargo/bin:$PATH`.
- `ui/src/ipc/bindings.ts` is generated: `cargo test -p linascp exports_bindings`. Never edit by hand.
- Exactly one terminal `ConnectEvent` (`connected` | `failed`) per connect id, also for SFTP, cancel and pre-pipeline errors. `useConnectStore.start`/`retry` never reject.
- Secrets only in the OS keyring; never in logs, events, errors, notices.
- Host keys only in `~/.ssh/known_hosts`; fail closed.
- Rename never overwrites. Delete never follows symlinks. No lossy file name ever reaches the file system.
- Full check: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bun run typecheck && bun run test`.
- Docker suite: `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`.

## Deviations from the spec (decided in this plan)

- **Trash:** no `trash` crate. A ~60-line freedesktop implementation (`sftp/local.rs::trash`) moves into
  `$XDG_DATA_HOME/Trash` (`AppPaths::trash_dir()`) with `rename`. It is testable against a temp dir without touching
  process env. Cost: files on another file system (USB stick) cannot be trashed; they fail with a clear `Fs`
  error telling the user to use Shift+Entf. Upgrade path: per-mount `.Trash-$uid` dirs.
- **Lossy names:** the core cannot tell a lossy name from a real one once it is a `String`, so
  `validate_file_name` rejects names containing U+FFFD. Cost: a real file whose name contains U+FFFD cannot be
  renamed/deleted from LinaSCP (vanishingly rare).
- **Unknown SFTP session id:** `CoreError::Internal` (as `sessions.rs` does for terminals); there is no `NotFound` variant.
- **SFTP version:** russh-sftp speaks v3; the version from `init()` is passed through as `version`.

## Review Focus

1. A remote dir with a symlink loop or a link to `/` — listing must stay one level (one `stat` per link), never recurse; delete must remove only the link. (Task 4 docker test `delete_removes_links_not_targets`.)
2. Connection dies while a request is in flight (keepalive off) — the panel must not hang: request timeout = `host.timeout_s`, then `Network` and exactly one `Lost`. (Task 4 docker test `killed_session_reports_lost_once`.)
3. User closes the SFTP tab while "Neu verbinden" is still connecting — the new session must be closed, not leaked into a dead tab. (Task 7 store test `reconnect into a closed tab closes the new session`.)
4. Rapid navigation (double-click into A, then B before A's listing returns) — the older response must not overwrite the newer path. (Task 7 store test `a stale listing does not overwrite a newer navigation`.)
5. Keys typed into the path field or a dialog (Backspace, F2, Entf, letters) — must not trigger pane actions. (Task 8 test `keys inside the path field stay with the field`.)

---

### Task 1: Core types, `Fs` error, name validation, trash path

**Files:**
- Create: `crates/core/src/sftp/mod.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/src/error.rs`, `crates/core/src/validate.rs`, `crates/core/src/paths.rs`

**Interfaces:**
- Produces:
  - `linascp_core::sftp::{Entry, EntryKind, LinkTarget, Listing}` (serde camelCase, `specta::Type`)
  - `CoreError::Fs { kind: FsKind, path: String, message: String }`, `linascp_core::error::FsKind { PermissionDenied, NotFound, AlreadyExists, NotEmpty, Other }`
  - `CoreError::fs(kind: FsKind, path: impl Into<String>, message: impl Into<String>) -> CoreError`
  - `linascp_core::validate::validate_file_name(name: &str) -> Result<()>`
  - `AppPaths::trash_dir(&self) -> PathBuf`
  - `linascp_core::sftp::join(dir: &str, name: &str) -> String`

- [ ] **Step 1: Write the failing tests**

In `crates/core/src/validate.rs` `mod tests`:

```rust
    #[test]
    fn file_names_must_be_single_path_components() {
        for bad in ["", ".", "..", "a/b", "/", "a\0b", "bad\u{FFFD}name"] {
            assert!(matches!(validate_file_name(bad), Err(CoreError::Config { .. })), "{bad:?} accepted");
        }
        for good in ["a", ".hidden", "with space", "ümlaut.txt", "a..b"] {
            assert!(validate_file_name(good).is_ok(), "{good:?} rejected");
        }
    }
```

New file `crates/core/src/sftp/mod.rs` ends with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_handles_root_and_plain_dirs() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/var/www", "html"), "/var/www/html");
        assert_eq!(join("/var/www/", "html"), "/var/www/html");
    }

    #[test]
    fn entries_serialize_camel_case_with_tagged_kind() {
        let entry = Entry {
            name: "l".into(),
            kind: EntryKind::Symlink { target: Some(LinkTarget::Dir) },
            size: 3,
            mtime: Some(1),
            mode: Some(0o120777),
            owner: Some("root".into()),
            name_lossy: false,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], serde_json::json!({ "type": "symlink", "target": "dir" }));
        assert_eq!(json["nameLossy"], false);
    }
}
```

In `crates/core/src/error.rs` `mod tests`:

```rust
    #[test]
    fn fs_error_serializes_with_kind_and_path() {
        let json = serde_json::to_value(CoreError::fs(FsKind::AlreadyExists, "/tmp/a", "exists")).unwrap();
        assert_eq!(json, serde_json::json!({ "type": "fs", "kind": "alreadyExists", "path": "/tmp/a", "message": "exists" }));
    }
```

In `crates/core/src/paths.rs` tests (create `#[cfg(test)] mod tests` if absent):

```rust
    #[test]
    fn trash_dir_is_next_to_the_data_dir() {
        let paths = AppPaths::for_root(std::path::Path::new("/r"));
        assert_eq!(paths.trash_dir(), std::path::PathBuf::from("/r/Trash"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p linascp-core file_names_must join_handles entries_serialize fs_error_serializes trash_dir_is 2>&1 | tail -20`
Expected: compile errors (`validate_file_name`, `sftp`, `FsKind`, `trash_dir` not found).

- [ ] **Step 3: Implement**

`crates/core/src/error.rs` — add before `CoreError`:

```rust
/// Classification of file-system failures (local or SFTP), used for UI texts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum FsKind {
    PermissionDenied,
    NotFound,
    AlreadyExists,
    NotEmpty,
    Other,
}
```

Variant in `CoreError` (after `Io`):

```rust
    #[error("file error ({kind:?}) at {path}: {message}")]
    Fs { kind: FsKind, path: String, message: String },
```

Constructor in `impl CoreError`:

```rust
    pub fn fs(kind: FsKind, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Fs { kind, path: path.into(), message: message.into() }
    }
```

Export in `lib.rs`: `pub use error::{CoreError, FsKind, NetworkKind, Result};` and `pub mod sftp;`.

`crates/core/src/validate.rs`:

```rust
/// A single path component typed by the user (new folder, rename) or sent back from a listing.
/// U+FFFD marks a name that was lossily decoded from a non-UTF-8 local name: it is rejected so a
/// lossy name never addresses a (different) real file.
// ponytail: also rejects real names containing U+FFFD; add raw-byte names if that ever matters.
pub fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0', '\u{FFFD}']) {
        return Err(CoreError::config(format!("ungültiger Dateiname: {name:?}")));
    }
    Ok(())
}
```

`crates/core/src/paths.rs` in `impl AppPaths`:

```rust
    /// freedesktop.org home trash (`$XDG_DATA_HOME/Trash`), the parent of `data_dir`.
    pub fn trash_dir(&self) -> PathBuf {
        self.data_dir.parent().unwrap_or(&self.data_dir).join("Trash")
    }
```

`crates/core/src/sftp/mod.rs`:

```rust
//! Directory listings shared by the local side (`local`) and SFTP (`connect::SftpSession`).

pub mod local;

use serde::{Deserialize, Serialize};

/// What a symlink points to; `None` in `EntryKind::Symlink` = dangling or unreadable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum LinkTarget {
    File,
    Dir,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink { target: Option<LinkTarget> },
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    /// Unix seconds.
    pub mtime: Option<i64>,
    /// `st_mode` as reported (type and permission bits).
    pub mode: Option<u32>,
    /// User name, else the numeric uid.
    pub owner: Option<String>,
    /// Local only: the real name is not valid UTF-8; `name` is a lossy rendering and must not be used for operations.
    pub name_lossy: bool,
}

/// One directory: canonical absolute `path` and its entries without `.` and `..`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
}

/// POSIX join of an absolute directory and one validated name.
pub fn join(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}
```

Create an empty `crates/core/src/sftp/local.rs` with `//! Local file operations of the commander's left pane.` so the module compiles.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p linascp-core file_names_must join_handles entries_serialize fs_error_serializes trash_dir_is 2>&1 | grep -E "test result|FAILED"`
Expected: all PASS. Then `cargo clippy --workspace --all-targets -- -D warnings` clean (a non-exhaustive `match` on `CoreError` in `src-tauri` or core, if any, gets an `Fs` arm mapped like `Io`).

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): listing types, Fs error kind and file-name validation for SFTP

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Local file operations

**Files:**
- Modify: `crates/core/src/sftp/local.rs`

**Interfaces:**
- Consumes: Task 1 types, `validate_file_name`, `CoreError::fs`, `join`.
- Produces (all sync, run in `spawn_blocking` by the backend):
  - `pub fn list(path: &str) -> Result<Listing>`
  - `pub fn home(local_start: Option<&str>, home: &Path) -> String`
  - `pub fn mkdir(dir: &str, name: &str) -> Result<()>`
  - `pub fn rename(dir: &str, from: &str, to: &str) -> Result<()>`
  - `pub fn delete(dir: &str, names: &[String]) -> Result<()>`
  - `pub fn trash(trash_root: &Path, dir: &str, names: &[String]) -> Result<()>`
  - `pub(crate) fn io_error(error: std::io::Error, path: &str) -> CoreError`

- [ ] **Step 1: Write the failing tests**

Append to `local.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FsKind;
    use crate::sftp::{EntryKind, LinkTarget};
    use std::os::unix::ffi::OsStrExt;

    fn tmp() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().to_str().unwrap().to_string();
        (dir, path)
    }

    fn entry<'a>(listing: &'a Listing, name: &str) -> &'a crate::sftp::Entry {
        listing.entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} missing"))
    }

    #[test]
    fn list_reports_kinds_sizes_and_links() {
        let (_t, d) = tmp();
        std::fs::write(format!("{d}/f.txt"), b"abc").unwrap();
        std::fs::create_dir(format!("{d}/sub")).unwrap();
        std::os::unix::fs::symlink(format!("{d}/sub"), format!("{d}/to-dir")).unwrap();
        std::os::unix::fs::symlink(format!("{d}/gone"), format!("{d}/dangling")).unwrap();
        let listing = list(&d).unwrap();
        assert_eq!(listing.path, d);
        assert_eq!(listing.entries.len(), 4, "no . or ..");
        let f = entry(&listing, "f.txt");
        assert_eq!((f.kind.clone(), f.size), (EntryKind::File, 3));
        assert!(f.mtime.is_some() && f.mode.is_some() && f.owner.is_some());
        assert_eq!(entry(&listing, "sub").kind, EntryKind::Dir);
        assert_eq!(entry(&listing, "to-dir").kind, EntryKind::Symlink { target: Some(LinkTarget::Dir) });
        assert_eq!(entry(&listing, "dangling").kind, EntryKind::Symlink { target: None });
    }

    #[test]
    fn list_canonicalizes_the_path() {
        let (_t, d) = tmp();
        std::fs::create_dir(format!("{d}/sub")).unwrap();
        assert_eq!(list(&format!("{d}/sub/..")).unwrap().path, d);
    }

    #[test]
    fn list_of_a_missing_dir_is_not_found() {
        let (_t, d) = tmp();
        let error = list(&format!("{d}/nope")).unwrap_err();
        assert!(matches!(error, CoreError::Fs { kind: FsKind::NotFound, .. }), "{error:?}");
    }

    #[test]
    fn mkdir_refuses_an_existing_name() {
        let (_t, d) = tmp();
        mkdir(&d, "new").unwrap();
        assert!(std::path::Path::new(&format!("{d}/new")).is_dir());
        assert!(matches!(mkdir(&d, "new"), Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
    }

    #[test]
    fn rename_never_overwrites() {
        let (_t, d) = tmp();
        std::fs::write(format!("{d}/a"), b"a").unwrap();
        std::fs::write(format!("{d}/b"), b"b").unwrap();
        assert!(matches!(rename(&d, "a", "b"), Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
        assert_eq!(std::fs::read(format!("{d}/b")).unwrap(), b"b");
        rename(&d, "a", "c").unwrap();
        assert_eq!(std::fs::read(format!("{d}/c")).unwrap(), b"a");
    }

    #[test]
    fn delete_removes_links_not_their_targets() {
        let (_t, d) = tmp();
        let (_o, outside) = tmp();
        std::fs::write(format!("{outside}/keep"), b"k").unwrap();
        std::fs::create_dir_all(format!("{d}/tree/inner")).unwrap();
        std::fs::write(format!("{d}/tree/inner/x"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, format!("{d}/tree/link")).unwrap();
        std::os::unix::fs::symlink(&outside, format!("{d}/toplink")).unwrap();
        delete(&d, &["tree".into(), "toplink".into()]).unwrap();
        assert!(list(&d).unwrap().entries.is_empty());
        assert!(std::path::Path::new(&format!("{outside}/keep")).exists());
    }

    #[test]
    fn invalid_names_are_config_errors() {
        let (_t, d) = tmp();
        assert!(matches!(mkdir(&d, "a/b"), Err(CoreError::Config { .. })));
        assert!(matches!(rename(&d, "..", "x"), Err(CoreError::Config { .. })));
        assert!(matches!(delete(&d, &["".into()]), Err(CoreError::Config { .. })));
    }

    #[test]
    fn non_utf8_names_are_listed_lossy_and_cannot_be_operated_on() {
        let (_t, d) = tmp();
        let raw = std::ffi::OsStr::from_bytes(b"bad\xffname");
        std::fs::write(std::path::Path::new(&d).join(raw), b"").unwrap();
        let listing = list(&d).unwrap();
        let lossy = &listing.entries[0];
        assert!(lossy.name_lossy);
        assert!(matches!(delete(&d, &[lossy.name.clone()]), Err(CoreError::Config { .. })));
        assert_eq!(list(&d).unwrap().entries.len(), 1);
    }

    #[test]
    fn trash_moves_into_files_and_writes_trashinfo() {
        let (_t, d) = tmp();
        let (_tr, trash_root) = tmp();
        let trash_root = std::path::Path::new(&trash_root).join("Trash");
        std::fs::write(format!("{d}/a b.txt"), b"1").unwrap();
        trash(&trash_root, &d, &["a b.txt".into()]).unwrap();
        std::fs::write(format!("{d}/a b.txt"), b"2").unwrap();
        trash(&trash_root, &d, &["a b.txt".into()]).unwrap();
        assert!(list(&d).unwrap().entries.is_empty());
        assert_eq!(std::fs::read(trash_root.join("files/a b.txt")).unwrap(), b"1");
        assert_eq!(std::fs::read(trash_root.join("files/a b.txt.1")).unwrap(), b"2");
        let info = std::fs::read_to_string(trash_root.join("info/a b.txt.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath="), "{info}");
        assert!(info.contains("/a%20b.txt\n"), "{info}");
        assert!(info.contains("\nDeletionDate=20"), "{info}");
    }

    #[test]
    fn home_prefers_local_start() {
        let home_dir = std::path::Path::new("/home/u");
        assert_eq!(home(Some("/srv"), home_dir), "/srv");
        assert_eq!(home(Some("~/proj"), home_dir), "/home/u/proj");
        assert_eq!(home(Some("  "), home_dir), "/home/u");
        assert_eq!(home(None, home_dir), "/home/u");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p linascp-core sftp::local 2>&1 | tail -5`
Expected: compile errors (`list`, `mkdir`, … not found).

- [ ] **Step 3: Implement** (above the tests in `local.rs`)

```rust
//! Local file operations of the commander's left pane.

use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::OnceLock;

use super::{Entry, EntryKind, LinkTarget, Listing, join};
use crate::error::{CoreError, FsKind, Result};
use crate::paths::expand_tilde;
use crate::validate::validate_file_name;

pub(crate) fn io_error(error: io::Error, path: &str) -> CoreError {
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => FsKind::PermissionDenied,
        io::ErrorKind::NotFound => FsKind::NotFound,
        io::ErrorKind::AlreadyExists => FsKind::AlreadyExists,
        io::ErrorKind::DirectoryNotEmpty => FsKind::NotEmpty,
        _ => FsKind::Other,
    };
    CoreError::fs(kind, path, error.to_string())
}

/// uid -> user name from `/etc/passwd`, read once.
// ponytail: ignores NSS (LDAP/sssd users show their uid); fine for a desktop.
fn user_name(uid: u32) -> String {
    static USERS: OnceLock<HashMap<u32, String>> = OnceLock::new();
    let users = USERS.get_or_init(|| {
        fs::read_to_string("/etc/passwd")
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let mut fields = line.split(':');
                let name = fields.next()?;
                let uid = fields.nth(1)?.parse().ok()?;
                Some((uid, name.to_string()))
            })
            .collect()
    });
    users.get(&uid).cloned().unwrap_or_else(|| uid.to_string())
}

fn link_target(path: &Path) -> Option<LinkTarget> {
    let meta = fs::metadata(path).ok()?;
    Some(if meta.is_dir() {
        LinkTarget::Dir
    } else if meta.is_file() {
        LinkTarget::File
    } else {
        LinkTarget::Other
    })
}

fn entry(dir: &Path, name: &std::ffi::OsStr, meta: &Metadata) -> Entry {
    let kind = if meta.file_type().is_symlink() {
        EntryKind::Symlink { target: link_target(&dir.join(name)) }
    } else if meta.is_dir() {
        EntryKind::Dir
    } else if meta.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let utf8 = name.to_str();
    Entry {
        name: utf8.map_or_else(|| name.to_string_lossy().into_owned(), str::to_string),
        kind,
        size: meta.len(),
        mtime: Some(meta.mtime()),
        mode: Some(meta.mode()),
        owner: Some(user_name(meta.uid())),
        name_lossy: utf8.is_none(),
    }
}

pub fn list(path: &str) -> Result<Listing> {
    let canonical = fs::canonicalize(path).map_err(|e| io_error(e, path))?;
    let shown = canonical.to_string_lossy().into_owned();
    let mut entries = Vec::new();
    for item in fs::read_dir(&canonical).map_err(|e| io_error(e, &shown))? {
        let item = item.map_err(|e| io_error(e, &shown))?;
        // An entry that vanished between readdir and lstat is simply skipped.
        if let Ok(meta) = item.metadata() {
            entries.push(entry(&canonical, &item.file_name(), &meta));
        }
    }
    Ok(Listing { path: shown, entries })
}

/// Start directory of the local pane: the host's `local_start` (`~` expanded), else `$HOME`.
pub fn home(local_start: Option<&str>, home: &Path) -> String {
    match local_start.map(str::trim).filter(|s| !s.is_empty()) {
        Some(start) => expand_tilde(start, home).to_string_lossy().into_owned(),
        None => home.to_string_lossy().into_owned(),
    }
}

pub fn mkdir(dir: &str, name: &str) -> Result<()> {
    validate_file_name(name)?;
    let path = join(dir, name);
    fs::DirBuilder::new().mode(0o755).create(&path).map_err(|e| io_error(e, &path))
}

/// Same-directory rename that never replaces an existing target (`RENAME_NOREPLACE`).
pub fn rename(dir: &str, from: &str, to: &str) -> Result<()> {
    validate_file_name(from)?;
    validate_file_name(to)?;
    let (old, new) = (join(dir, from), join(dir, to));
    let c = |p: &str| CString::new(p).map_err(|_| CoreError::config("Pfad enthält NUL"));
    let (c_old, c_new) = (c(&old)?, c(&new)?);
    // SAFETY: both pointers are valid NUL-terminated strings for the duration of the call.
    let rc = unsafe {
        libc::renameat2(libc::AT_FDCWD, c_old.as_ptr(), libc::AT_FDCWD, c_new.as_ptr(), libc::RENAME_NOREPLACE)
    };
    if rc == 0 { Ok(()) } else { Err(io_error(io::Error::last_os_error(), &new)) }
}

/// Permanent delete; directories recursively. `remove_dir_all` never follows symlinks, and a link
/// itself (even one to a directory) is removed with `remove_file`.
pub fn delete(dir: &str, names: &[String]) -> Result<()> {
    for name in names {
        validate_file_name(name)?;
    }
    for name in names {
        let path = join(dir, name);
        let meta = fs::symlink_metadata(&path).map_err(|e| io_error(e, &path))?;
        let result = if meta.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        result.map_err(|e| io_error(e, &path))?;
    }
    Ok(())
}

/// Percent-encodes a path for `.trashinfo` (RFC 2396 escaping, `/` kept).
fn percent_encode(path: &[u8]) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Local time as `YYYY-MM-DDThh:mm:ss` (freedesktop `DeletionDate`).
fn deletion_date() -> String {
    // SAFETY: `time` and `localtime_r` only write into the provided, properly sized structs.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

/// Moves entries into the freedesktop home trash `trash_root` (`files/` + `info/*.trashinfo`).
// ponytail: home trash only (rename); other file systems fail with EXDEV -> user deletes permanently.
pub fn trash(trash_root: &Path, dir: &str, names: &[String]) -> Result<()> {
    for name in names {
        validate_file_name(name)?;
    }
    let (files, info) = (trash_root.join("files"), trash_root.join("info"));
    for d in [&files, &info] {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(d).map_err(|e| io_error(e, &d.to_string_lossy()))?;
    }
    for name in names {
        let path = join(dir, name);
        fs::symlink_metadata(&path).map_err(|e| io_error(e, &path))?;
        let mut n = 0u32;
        loop {
            let candidate = if n == 0 { name.clone() } else { format!("{name}.{n}") };
            let info_path = info.join(format!("{candidate}.trashinfo"));
            let mut file = match OpenOptions::new().write(true).create_new(true).mode(0o600).open(&info_path) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    n += 1;
                    continue;
                }
                Err(e) => return Err(io_error(e, &info_path.to_string_lossy())),
            };
            let body = format!(
                "[Trash Info]\nPath={}\nDeletionDate={}\n",
                percent_encode(Path::new(&path).as_os_str().as_bytes()),
                deletion_date()
            );
            let moved = file.write_all(body.as_bytes()).and_then(|()| fs::rename(&path, files.join(&candidate)));
            if let Err(e) = moved {
                let _ = fs::remove_file(&info_path);
                if e.raw_os_error() == Some(libc::EXDEV) {
                    return Err(CoreError::fs(
                        FsKind::Other,
                        &path,
                        "liegt auf einem anderen Laufwerk ohne Papierkorb – mit Umschalt+Entf endgültig löschen",
                    ));
                }
                return Err(io_error(e, &path));
            }
            break;
        }
    }
    Ok(())
}
```

If `paths::expand_tilde` takes other arguments than `(&str, &Path) -> PathBuf`, adapt the call and ledger a ruling.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p linascp-core sftp:: 2>&1 | grep -E "test result|FAILED|panicked"`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/sftp/local.rs
git commit -m "feat(core): local file operations for the commander (list, mkdir, rename, delete, trash)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: SFTP error mapping, longname owner, SFTP steps

**Files:**
- Modify: `crates/core/Cargo.toml` (add `russh-sftp = "3.0"`), `crates/core/src/connect/mod.rs`
- Create: `crates/core/src/connect/sftp_session.rs` (helpers only in this task)

**Interfaces:**
- Produces:
  - `Step::Subsystem`, `Step::Directory` (serde `"subsystem"`, `"directory"`)
  - `pub fn plan_sftp_steps(req: &ConnectRequest) -> Vec<Step>` → `[JumpHost?, Tcp, HostKey, Auth, Subsystem, Directory]`
  - `pub(crate) fn map_sftp_error(error: russh_sftp::client::error::Error, path: &str) -> CoreError`
  - `pub(crate) fn owner_from_longname(longname: &str) -> Option<String>`

- [ ] **Step 1: Write the failing tests**

In `connect/mod.rs` tests (reuse the existing `request(with_jump)` helper):

```rust
    #[test]
    fn sftp_steps_replace_channel_and_shell() {
        assert_eq!(
            plan_sftp_steps(&request(true)),
            vec![Step::JumpHost, Step::Tcp, Step::HostKey, Step::Auth, Step::Subsystem, Step::Directory]
        );
        assert_eq!(plan_sftp_steps(&request(false))[0], Step::Tcp);
    }
```

`connect/sftp_session.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{FsKind, NetworkKind};
    use russh_sftp::client::error::Error as SftpError;
    use russh_sftp::protocol::{Status, StatusCode};

    fn status(code: StatusCode) -> SftpError {
        SftpError::Status(Status { id: 1, status_code: code, error_message: "m".into(), language_tag: "en".into() })
    }

    #[test]
    fn status_codes_map_to_fs_kinds() {
        let kind = |e| match map_sftp_error(e, "/p") {
            CoreError::Fs { kind, path, .. } => {
                assert_eq!(path, "/p");
                kind
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(kind(status(StatusCode::NoSuchFile)), FsKind::NotFound);
        assert_eq!(kind(status(StatusCode::PermissionDenied)), FsKind::PermissionDenied);
        assert_eq!(kind(status(StatusCode::Failure)), FsKind::Other);
    }

    #[test]
    fn transport_failures_are_network_errors() {
        assert!(matches!(map_sftp_error(SftpError::Timeout, "/p"), CoreError::Network { kind: NetworkKind::Timeout, .. }));
        assert!(matches!(
            map_sftp_error(SftpError::UnexpectedBehavior("session closed".into()), "/p"),
            CoreError::Network { kind: NetworkKind::Other, .. }
        ));
        assert!(matches!(map_sftp_error(SftpError::IO("broken pipe".into()), "/p"), CoreError::Network { .. }));
    }

    #[test]
    fn owner_is_the_third_longname_column() {
        assert_eq!(
            owner_from_longname("-rw-r--r--    1 www-data www-data     1234 Oct  8 10:41 index.php").as_deref(),
            Some("www-data")
        );
        assert_eq!(owner_from_longname(""), None);
        assert_eq!(owner_from_longname("-rw-r--r-- 1"), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p linascp-core sftp_steps status_codes transport_failures owner_is 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement**

`crates/core/Cargo.toml` `[dependencies]`: `russh-sftp = "3.0"` (alphabetical after `russh`).

`connect/mod.rs`: add `mod sftp_session; pub use sftp_session::{LostFn, SftpSession};` (the `LostFn`/`SftpSession` items arrive in Task 4; in this task declare only `mod sftp_session;`). Extend `Step`:

```rust
    Channel,
    Shell,
    Subsystem,
    Directory,
```

```rust
/// Steps of an SFTP connect: the SSH part, then subsystem and start directory.
pub fn plan_sftp_steps(req: &ConnectRequest) -> Vec<Step> {
    let mut steps = plan_steps(req);
    steps.truncate(steps.len() - 2);
    steps.extend([Step::Subsystem, Step::Directory]);
    steps
}
```

`connect/sftp_session.rs` (helpers):

```rust
//! SFTP session over a russh connection (remote side of the commander).

use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::StatusCode;

use super::classify::network_error;
use crate::error::{CoreError, FsKind, NetworkKind};

/// SFTP status -> `Fs`; transport problems (timeout, closed channel) -> `Network`.
pub(crate) fn map_sftp_error(error: SftpError, path: &str) -> CoreError {
    match error {
        SftpError::Status(status) => {
            let kind = match status.status_code {
                StatusCode::NoSuchFile => FsKind::NotFound,
                StatusCode::PermissionDenied => FsKind::PermissionDenied,
                _ => FsKind::Other,
            };
            CoreError::fs(kind, path, status.error_message)
        }
        SftpError::Timeout => network_error(NetworkKind::Timeout, "SFTP request timed out"),
        SftpError::IO(message) | SftpError::UnexpectedBehavior(message) => network_error(NetworkKind::Other, message),
        other => CoreError::internal(other.to_string()),
    }
}

/// `ls -l` style longname: the owner is the third whitespace-separated column.
pub(crate) fn owner_from_longname(longname: &str) -> Option<String> {
    longname.split_whitespace().nth(2).map(str::to_string)
}
```

Check `network_error`'s signature in `connect/classify.rs` (`network_error(kind, message: impl Into<String>)`); adapt the calls if it differs.

UI must compile after bindings regenerate in Task 5; `ui/src/screens/connect/steps.ts` gets the new cases there.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p linascp-core sftp_steps status_codes transport_failures owner_is 2>&1 | grep -E "test result|FAILED"`
Expected: PASS. `cargo clippy --workspace --all-targets -- -D warnings` may flag unused helpers: add `#[allow(dead_code)]`? No — Task 4 uses them; if clippy fails here, keep them `#[cfg_attr(not(test), allow(dead_code))]` until Task 4 and remove the attribute there.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/core
git commit -m "feat(core): SFTP connect steps and error mapping

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `SftpSession` and `connect_sftp` (Docker-tested)

**Files:**
- Modify: `crates/core/src/connect/sftp_session.rs`, `crates/core/src/connect/mod.rs`
- Test: `crates/core/tests/ssh_integration.rs`

**Interfaces:**
- Consumes: Task 1 (`Listing`, `Entry`, `join`, `validate_file_name`), Task 3 helpers, `Pipeline::establish`, `Established`.
- Produces:
  - `pub type LostFn = Arc<dyn Fn(CoreError) + Send + Sync>;`
  - `pub async fn connect_sftp(req: ConnectRequest, deps: ConnectDeps, broker: Arc<PromptBroker>, emit: EmitFn, on_lost: LostFn, cancel: CancellationToken) -> Result<(SftpSession, Listing)>`
  - `impl SftpSession { pub fn info(&self) -> &SessionInfo; pub fn version(&self) -> u32; pub async fn home(&self) -> Result<String>; pub async fn list(&self, path: &str) -> Result<Listing>; pub async fn mkdir(&self, dir: &str, name: &str) -> Result<()>; pub async fn rename(&self, dir: &str, from: &str, to: &str) -> Result<()>; pub async fn delete(&self, dir: &str, names: &[String]) -> Result<()>; pub async fn close(&self) }`

- [ ] **Step 1: Write the failing Docker tests**

Append to `crates/core/tests/ssh_integration.rs` (imports: add `connect_sftp, LostFn` to the `linascp_core::connect` import, `use linascp_core::error::FsKind;`, `use linascp_core::sftp::{EntryKind, LinkTarget};`):

```rust
async fn sftp(env: &Env, h: Host, lost: LostFn) -> (linascp_core::connect::SftpSession, linascp_core::sftp::Listing, Vec<ConnectEvent>) {
    let p = prompter(trust_all);
    let (session, listing) = connect_sftp(request(h, None), env.deps(None), p.broker.clone(), p.emit.clone(), lost, CancellationToken::new())
        .await
        .expect("sftp connected");
    (session, listing, p.events())
}

fn no_lost() -> LostFn {
    Arc::new(|e| panic!("unexpected lost: {e:?}"))
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_connects_into_home_with_sftp_steps() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let (session, listing, events) = sftp(&env, h, no_lost()).await;
    assert_eq!(listing.path, "/home/pwuser");
    assert!(listing.entries.iter().any(|e| e.name == ".bashrc" && e.owner.as_deref() == Some("pwuser")));
    assert!(events.iter().any(|e| matches!(e, ConnectEvent::StepDone { step: Step::Subsystem })));
    assert!(events.iter().any(|e| matches!(e, ConnectEvent::StepDone { step: Step::Directory })));
    assert_eq!(session.version(), 3);
    session.close().await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_unreadable_remote_start_fails_the_directory_step_once() {
    let c = Container::start(None);
    let env = Env::new();
    let mut h = host(c.port, "pwuser", AuthMethod::Password, None);
    h.sftp.remote_start = Some("/root".into());
    env.store_password(&h, "pwpass");
    let p = prompter(trust_all);
    let result = connect_sftp(request(h, None), env.deps(None), p.broker.clone(), p.emit.clone(), no_lost(), CancellationToken::new()).await;
    assert!(matches!(result, Err(CoreError::Fs { kind: FsKind::PermissionDenied, .. })), "{:?}", result.err());
    let events = p.events();
    assert!(events.iter().any(|e| matches!(e, ConnectEvent::StepFailed { step: Step::Directory, .. })));
    assert_eq!(events.iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count(), 1);
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_mkdir_rename_and_errors() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let (s, home, _) = sftp(&env, h, no_lost()).await;
    let d = home.path;
    s.mkdir(&d, "a").await.unwrap();
    s.mkdir(&d, "b").await.unwrap();
    assert!(matches!(s.mkdir(&d, "a").await, Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
    assert!(matches!(s.rename(&d, "a", "b").await, Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
    s.rename(&d, "a", "c").await.unwrap();
    let names: Vec<_> = s.list(&d).await.unwrap().entries.into_iter().map(|e| e.name).collect();
    assert!(names.contains(&"c".into()) && names.contains(&"b".into()) && !names.contains(&"a".into()));
    assert!(matches!(s.list("/root").await, Err(CoreError::Fs { kind: FsKind::PermissionDenied, .. })));
    assert!(matches!(s.mkdir(&d, "x/y").await, Err(CoreError::Config { .. })));
    s.close().await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_delete_removes_links_not_targets() {
    let c = Container::start(None);
    let env = Env::new();
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let setup = "mkdir -p /home/pwuser/keep /home/pwuser/tree/inner && touch /home/pwuser/keep/k /home/pwuser/tree/inner/x \
        && ln -s /home/pwuser/keep /home/pwuser/tree/link && ln -s / /home/pwuser/rootlink && ln -s loop /home/pwuser/loop \
        && chown -R pwuser: /home/pwuser";
    let out = std::process::Command::new("docker").args(["exec", &c.id, "sh", "-c", setup]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let (s, home, _) = sftp(&env, h, no_lost()).await;
    let kind = |name: &str| home.entries.iter().find(|e| e.name == name).unwrap().kind.clone();
    assert_eq!(kind("rootlink"), EntryKind::Symlink { target: Some(LinkTarget::Dir) });
    assert_eq!(kind("loop"), EntryKind::Symlink { target: None });
    s.delete(&home.path, &["tree".into(), "rootlink".into(), "loop".into()]).await.unwrap();
    let after: Vec<_> = s.list(&home.path).await.unwrap().entries.into_iter().map(|e| e.name).collect();
    assert!(!after.iter().any(|n| n == "tree" || n == "rootlink" || n == "loop"), "{after:?}");
    assert!(s.list("/home/pwuser/keep").await.unwrap().entries.iter().any(|e| e.name == "k"));
    s.close().await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sftp_killed_session_reports_lost_once() {
    use std::sync::Mutex;
    let c = Container::start(None);
    let env = Env::new();
    let mut h = host(c.port, "pwuser", AuthMethod::Password, None);
    h.keepalive_s = 1;
    h.timeout_s = 3;
    env.store_password(&h, "pwpass");
    let lost: Arc<Mutex<Vec<CoreError>>> = Arc::default();
    let on_lost: LostFn = {
        let lost = lost.clone();
        Arc::new(move |e| lost.lock().unwrap().push(e))
    };
    let (s, home, _) = sftp(&env, h, on_lost).await;
    let kill = r#"for p in /proc/[0-9]*; do grep -qaE 'sshd(-session)?[:] pwuser' "$p/cmdline" 2>/dev/null && kill -9 "${p#/proc/}"; done; true"#;
    let _ = std::process::Command::new("docker").args(["exec", &c.id, "sh", "-c", kill]).output();
    let _ = s.list(&home.path).await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while lost.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline, "no lost event");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(matches!(s.list(&home.path).await, Err(CoreError::Network { .. })));
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(lost.lock().unwrap().len(), 1);
    s.close().await;
}
```

If `Prompter` has no `events()` accessor, add one in `tests/support/mod.rs` next to `prompts()`, collecting every emitted `ConnectEvent` (clone) in a `Mutex<Vec<_>>` inside the emit closure.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p linascp-core --test ssh_integration sftp_ -- --ignored --test-threads=1 2>&1 | tail -5`
Expected: compile errors (`connect_sftp`, `LostFn` not found).

- [ ] **Step 3: Implement**

`connect/sftp_session.rs` (above the helpers' tests):

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use russh::client;
use russh_sftp::client::RawSftpSession;
use russh_sftp::protocol::{FileAttributes, StatusCode};
use tokio::task::AbortHandle;

use super::SessionInfo;
use super::handler::ClientHandler;
use crate::error::Result;
use crate::sftp::{Entry, EntryKind, LinkTarget, Listing, join};
use crate::validate::validate_file_name;

/// Called at most once when the connection of an SFTP session is gone.
pub type LostFn = Arc<dyn Fn(CoreError) + Send + Sync>;

struct Lost {
    fired: AtomicBool,
    on_lost: LostFn,
}

impl Lost {
    /// Reports `error` once; later calls (and calls after `close`) do nothing.
    fn fire(&self, error: CoreError) {
        if !self.fired.swap(true, Ordering::SeqCst) {
            (self.on_lost)(error);
        }
    }
}

pub struct SftpSession {
    raw: RawSftpSession,
    handle: Arc<client::Handle<ClientHandler>>,
    jump: Option<client::Handle<ClientHandler>>,
    info: SessionInfo,
    version: u32,
    lost: Arc<Lost>,
    watcher: AbortHandle,
}

fn lost_error() -> CoreError {
    network_error(NetworkKind::Other, "connection lost")
}

impl SftpSession {
    pub(crate) fn start(
        raw: RawSftpSession,
        version: u32,
        handle: client::Handle<ClientHandler>,
        jump: Option<client::Handle<ClientHandler>>,
        info: SessionInfo,
        on_lost: LostFn,
    ) -> Self {
        let handle = Arc::new(handle);
        let lost = Arc::new(Lost { fired: AtomicBool::new(false), on_lost });
        // russh has no awaitable "closed": poll. Keepalive (host.keepalive_s) closes the handle on a dead link.
        // ponytail: 1 s polling per session; a handler `disconnected` hook if this ever matters.
        let watcher = {
            let (handle, lost) = (handle.clone(), lost.clone());
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    if handle.is_closed() {
                        lost.fire(lost_error());
                        break;
                    }
                }
            })
            .abort_handle()
        };
        Self { raw, handle, jump, info, version, lost, watcher }
    }

    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// Maps the error and reports a lost connection (once) when the transport is gone.
    fn check<T>(&self, result: std::result::Result<T, SftpError>, path: &str) -> Result<T> {
        result.map_err(|e| {
            let error = map_sftp_error(e, path);
            if matches!(error, CoreError::Network { .. }) && self.handle.is_closed() {
                self.lost.fire(error.clone());
            }
            error
        })
    }

    fn ensure_alive(&self) -> Result<()> {
        if self.lost.fired.load(Ordering::SeqCst) { Err(lost_error()) } else { Ok(()) }
    }

    pub async fn home(&self) -> Result<String> {
        self.canonical(".").await
    }

    async fn canonical(&self, path: &str) -> Result<String> {
        self.ensure_alive()?;
        let name = self.check(self.raw.realpath(path).await, path)?;
        name.files.into_iter().next().map(|f| f.filename).ok_or_else(|| CoreError::internal("empty realpath reply"))
    }

    async fn link_target(&self, path: &str) -> Option<LinkTarget> {
        let attrs = self.raw.stat(path).await.ok()?.attrs;
        Some(if attrs.is_dir() {
            LinkTarget::Dir
        } else if attrs.is_regular() {
            LinkTarget::File
        } else {
            LinkTarget::Other
        })
    }

    pub async fn list(&self, path: &str) -> Result<Listing> {
        let path = self.canonical(path).await?;
        let handle = self.check(self.raw.opendir(path.as_str()).await, &path)?.handle;
        let mut files = Vec::new();
        loop {
            match self.raw.readdir(handle.as_str()).await {
                Ok(name) => files.extend(name.files),
                Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => break,
                Err(e) => {
                    let _ = self.raw.close(handle.as_str()).await;
                    return self.check(Err(e), &path);
                }
            }
        }
        let _ = self.raw.close(handle.as_str()).await;
        let mut entries = Vec::with_capacity(files.len());
        for file in files.into_iter().filter(|f| f.filename != "." && f.filename != "..") {
            let attrs: &FileAttributes = &file.attrs;
            let kind = if attrs.is_symlink() {
                EntryKind::Symlink { target: self.link_target(&join(&path, &file.filename)).await }
            } else if attrs.is_dir() {
                EntryKind::Dir
            } else if attrs.is_regular() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            let owner = attrs
                .user
                .clone()
                .or_else(|| owner_from_longname(&file.longname))
                .or_else(|| attrs.uid.map(|uid| uid.to_string()));
            entries.push(Entry {
                kind,
                size: attrs.size.unwrap_or(0),
                mtime: attrs.mtime.map(i64::from),
                mode: attrs.permissions,
                owner,
                name: file.filename,
                name_lossy: false,
            });
        }
        Ok(Listing { path, entries })
    }

    async fn exists(&self, path: &str) -> bool {
        self.raw.lstat(path).await.is_ok()
    }

    pub async fn mkdir(&self, dir: &str, name: &str) -> Result<()> {
        validate_file_name(name)?;
        self.ensure_alive()?;
        let path = join(dir, name);
        match self.raw.mkdir(path.as_str(), FileAttributes::default()).await {
            Ok(_) => Ok(()),
            // OpenSSH answers a generic Failure for an existing name.
            Err(SftpError::Status(s)) if s.status_code == StatusCode::Failure && self.exists(&path).await => {
                Err(CoreError::fs(FsKind::AlreadyExists, &path, s.error_message))
            }
            Err(e) => self.check(Err(e), &path),
        }
    }

    /// SFTP v3 `rename` refuses an existing target; the `lstat` before gives a clear `AlreadyExists`.
    pub async fn rename(&self, dir: &str, from: &str, to: &str) -> Result<()> {
        validate_file_name(from)?;
        validate_file_name(to)?;
        self.ensure_alive()?;
        let (old, new) = (join(dir, from), join(dir, to));
        if self.exists(&new).await {
            return Err(CoreError::fs(FsKind::AlreadyExists, &new, "target exists"));
        }
        match self.raw.rename(old.as_str(), new.as_str()).await {
            Ok(_) => Ok(()),
            Err(SftpError::Status(s)) if s.status_code == StatusCode::Failure && self.exists(&new).await => {
                Err(CoreError::fs(FsKind::AlreadyExists, &new, s.error_message))
            }
            Err(e) => self.check(Err(e), &old),
        }
    }

    /// Permanent, recursive, bottom-up. Types come from `lstat`, so links are removed, never followed.
    pub async fn delete(&self, dir: &str, names: &[String]) -> Result<()> {
        for name in names {
            validate_file_name(name)?;
        }
        self.ensure_alive()?;
        // (path, Some(is_dir) once known, children pushed)
        let mut stack: Vec<(String, bool)> = names.iter().map(|n| (join(dir, n), false)).rev().collect();
        while let Some((path, expanded)) = stack.pop() {
            if expanded {
                self.check(self.raw.rmdir(path.as_str()).await, &path)?;
                continue;
            }
            let attrs = self.check(self.raw.lstat(path.as_str()).await, &path)?.attrs;
            if attrs.is_dir() && !attrs.is_symlink() {
                let children = self.list_names(&path).await?;
                stack.push((path.clone(), true));
                stack.extend(children.into_iter().map(|c| (join(&path, &c), false)));
            } else {
                self.check(self.raw.remove(path.as_str()).await, &path)?;
            }
        }
        Ok(())
    }

    /// Raw child names of `path` (no canonicalisation, no link resolution).
    async fn list_names(&self, path: &str) -> Result<Vec<String>> {
        let handle = self.check(self.raw.opendir(path).await, path)?.handle;
        let mut names = Vec::new();
        loop {
            match self.raw.readdir(handle.as_str()).await {
                Ok(name) => names.extend(name.files.into_iter().map(|f| f.filename).filter(|n| n != "." && n != "..")),
                Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break,
                Err(e) => {
                    let _ = self.raw.close(handle.as_str()).await;
                    return self.check(Err(e), path);
                }
            }
        }
        let _ = self.raw.close(handle.as_str()).await;
        Ok(names)
    }

    /// Ends the session without a `Lost` event.
    pub async fn close(&self) {
        self.lost.fired.store(true, Ordering::SeqCst);
        self.watcher.abort();
        let _ = self.raw.close_session();
        let _ = self.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        if let Some(jump) = &self.jump {
            let _ = jump.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        }
    }
}

impl Drop for SftpSession {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}
```

`connect/mod.rs`: `pub use sftp_session::{LostFn, SftpSession};` and

```rust
impl Pipeline {
    /// Steps Subsystem -> Directory; closes the connection again on failure.
    async fn open_sftp(&self, req: &ConnectRequest, est: Established, on_lost: LostFn) -> Result<(SftpSession, Listing)> {
        let host = &req.target.host;
        let Established { handle, jump, info } = est;
        let opened = self
            .step(Step::Subsystem, async {
                self.log("debug1: Requesting subsystem sftp");
                let channel = handle.channel_open_session().await.map_err(map_russh_error)?;
                channel.request_subsystem(false, "sftp").await.map_err(map_russh_error)?;
                let raw = RawSftpSession::new(channel.into_stream());
                if host.timeout_s > 0 {
                    raw.set_timeout(u64::from(host.timeout_s));
                }
                let version = raw.init().await.map_err(|e| sftp_session::map_sftp_error(e, ""))?;
                self.log(format!("debug1: SFTP protocol version {}", version.version));
                Ok((raw, version.version))
            })
            .await;
        let (raw, version) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                disconnect(Established { handle, jump, info }).await;
                return Err(error);
            }
        };
        let session = SftpSession::start(raw, version, handle, jump, info, on_lost);
        let listing = self
            .step(Step::Directory, async {
                let start = match host.sftp.remote_start.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    Some(start) if start == "~" => session.home().await?,
                    Some(start) => match start.strip_prefix("~/") {
                        Some(rest) => join(&session.home().await?, rest),
                        None => start.to_string(),
                    },
                    None => session.home().await?,
                };
                self.log(format!("debug1: Listing {start}"));
                session.list(&start).await
            })
            .await;
        match listing {
            Ok(listing) => Ok((session, listing)),
            Err(error) => {
                session.close().await;
                Err(error)
            }
        }
    }
}

/// Run the pipeline up to an SFTP session in the start directory.
/// The caller registers the session and emits `ConnectEvent::Connected`.
pub async fn connect_sftp(
    req: ConnectRequest,
    deps: ConnectDeps,
    broker: Arc<PromptBroker>,
    emit: EmitFn,
    on_lost: LostFn,
    cancel: CancellationToken,
) -> Result<(SftpSession, Listing)> {
    emit(ConnectEvent::Steps { steps: plan_sftp_steps(&req) });
    let pipeline = Pipeline::new(deps, broker, emit.clone(), cancel);
    let result = async {
        let est = pipeline.establish(&req).await?;
        pipeline.open_sftp(&req, est, on_lost).await
    }
    .await;
    if let Err(error) = &result {
        emit(ConnectEvent::Failed { error: error.clone() });
    }
    result
}
```

Imports in `connect/mod.rs`: `use russh_sftp::client::RawSftpSession; use crate::sftp::{Listing, join};`. If `EmitFn` is not the alias name in `connect/mod.rs`, use the type `Arc<dyn Fn(ConnectEvent) + Send + Sync>` as in `connect_ssh`. Remove any temporary `allow(dead_code)` from Task 3.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1 2>&1 | grep -E "test result|FAILED|panicked"`
Expected: 22 passed (17 old + 5 new). Then `cargo test -p linascp-core` and clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): SFTP session with list/mkdir/rename/delete and connect_sftp

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Backend commands and bindings

**Files:**
- Create: `src-tauri/src/commands/sftp.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/connect.rs` (make `channel_emit`, `fail`, `connect_deps`, `find_host`, `build_request`, `register_connect` reachable: `pub(crate)`), `src-tauri/src/state.rs`, `src-tauri/src/lib.rs`, `ui/src/ipc/bindings.ts` (generated)

**Interfaces:**
- Consumes: Task 2 local functions, Task 4 `connect_sftp`, `SftpSession`, `LostFn`.
- Produces (Tauri commands, camelCase in TS):
  - `connect_sftp(connect_id: Uuid, host_id: Id, events: Channel<ConnectEvent>, sftp_events: Channel<SftpEvent>) -> Result<SftpConnected>`
  - `sftp_list(session_id, path) -> Listing`, `sftp_mkdir(session_id, dir, name)`, `sftp_rename(session_id, dir, from, to)`, `sftp_delete(session_id, dir, names: Vec<String>)`, `sftp_close(session_id)`
  - `local_list(path) -> Listing`, `local_home(host_id: Option<Id>) -> String`, `local_mkdir(dir, name)`, `local_rename(dir, from, to)`, `local_delete(dir, names)`, `local_trash(dir, names)`
  - `SftpConnected { session_id, info: SessionInfo, version: u32, listing: Listing }`, `SftpEvent::Lost { error }` (`{"kind":"lost","error":…}`)
  - `AppState.sftp: Arc<Mutex<HashMap<SessionId, Arc<SftpSession>>>>`

- [ ] **Step 1: Write the failing tests** (in `commands/sftp.rs` `mod tests`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn setup() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        (dir, state)
    }

    #[tokio::test]
    async fn unknown_sftp_session_is_an_error() {
        let (_d, state) = setup();
        let result = sftp_list_inner(&state, Uuid::new_v4(), "/".into()).await;
        assert!(matches!(result, Err(CoreError::Internal { .. })));
    }

    #[tokio::test]
    async fn local_commands_work_on_a_temp_dir() {
        let (d, state) = setup();
        let root = d.path().canonicalize().unwrap().to_string_lossy().into_owned();
        local_mkdir_inner(root.clone(), "x".into()).await.unwrap();
        local_rename_inner(root.clone(), "x".into(), "y".into()).await.unwrap();
        local_trash_inner(&state, root.clone(), vec!["y".into()]).await.unwrap();
        assert!(state.paths.trash_dir().join("files/y").is_dir());
        let listing = local_list_inner(root.clone()).await.unwrap();
        assert!(!listing.entries.iter().any(|e| e.name == "y"));
    }

    #[tokio::test]
    async fn local_home_uses_the_hosts_local_start() {
        let (_d, state) = setup();
        let mut host = linascp_core::model::Host::new("h", "h.example");
        host.sftp.local_start = Some("/srv".into());
        let host = crate::commands::hosts::save_host(&state, host).unwrap();
        assert_eq!(local_home_inner(&state, Some(host.id)).unwrap(), "/srv");
        assert_eq!(local_home_inner(&state, None).unwrap(), state.paths.home.to_string_lossy());
    }

    #[tokio::test]
    async fn connect_sftp_for_a_missing_host_emits_exactly_one_failed() {
        let (_d, state) = setup();
        let events: Arc<StdMutex<Vec<ConnectEvent>>> = Arc::default();
        let emit: EmitFn = {
            let events = events.clone();
            Arc::new(move |e| events.lock().unwrap().push(e))
        };
        let result = connect_sftp_inner(&state, Uuid::new_v4(), Uuid::new_v4(), emit, Arc::new(|_| {})).await;
        assert!(result.is_err());
        let events = events.lock().unwrap();
        assert_eq!(events.iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count(), 1);
    }
}
```

If `hosts::save_host` has another signature, use what `connect.rs` tests use (`crate::commands::hosts::save_host`) and ledger a ruling.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p linascp sftp:: 2>&1 | tail -5`
Expected: compile errors.

- [ ] **Step 3: Implement**

`state.rs`: field `pub sftp: Arc<Mutex<HashMap<SessionId, Arc<SftpSession>>>>,` (doc: "Open SFTP sessions; separate from terminal sessions."), initialised with `Arc::new(Mutex::new(HashMap::new()))`; import `linascp_core::connect::SftpSession`.

`commands/sftp.rs`:

```rust
//! SFTP commander: remote operations on `SftpSession`s and local operations (left pane).

use std::sync::Arc;

use linascp_core::connect::{ConnectEvent, EmitFn, LostFn, SessionInfo, SftpSession, connect_sftp as run_pipeline};
use linascp_core::error::{CoreError, Result};
use linascp_core::model::Id;
use linascp_core::session::SessionId;
use linascp_core::sftp::{Listing, local};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use super::connect::{build_request, channel_emit, connect_deps, fail, find_host, register_connect};
use crate::state::AppState;
use crate::util::lock;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SftpConnected {
    pub session_id: SessionId,
    pub info: SessionInfo,
    pub version: u32,
    pub listing: Listing,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SftpEvent {
    Lost { error: CoreError },
}

pub(crate) async fn connect_sftp_inner(
    state: &AppState,
    connect_id: Uuid,
    host_id: Id,
    emit: EmitFn,
    on_lost: LostFn,
) -> Result<SftpConnected> {
    let guard = register_connect(state, connect_id);
    let host = match find_host(state, host_id) {
        Ok(host) => host,
        Err(error) => return fail(&emit, error),
    };
    let request = match build_request(state, host, 80, 24) {
        Ok(request) => request,
        Err(error) => return fail(&emit, error),
    };
    if guard.is_cancelled() {
        return fail(&emit, CoreError::Cancelled);
    }
    let cancel = guard.token();
    let result =
        run_pipeline(request, connect_deps(state), state.broker.clone(), emit.clone(), on_lost, cancel.clone()).await;
    drop(guard);
    match result {
        Ok((session, _)) if cancel.is_cancelled() => {
            session.close().await;
            fail(&emit, CoreError::Cancelled)
        }
        Ok((session, listing)) => {
            let session_id = Uuid::new_v4();
            let connected =
                SftpConnected { session_id, info: session.info().clone(), version: session.version(), listing };
            lock(&state.sftp).insert(session_id, Arc::new(session));
            emit(ConnectEvent::Connected { session_id });
            Ok(connected)
        }
        Err(error) => Err(error),
    }
}

fn session(state: &AppState, id: SessionId) -> Result<Arc<SftpSession>> {
    lock(&state.sftp).get(&id).cloned().ok_or_else(|| CoreError::Internal { message: format!("unknown sftp session {id}") })
}

pub(crate) async fn sftp_list_inner(state: &AppState, id: SessionId, path: String) -> Result<Listing> {
    session(state, id)?.list(&path).await
}

/// Runs a blocking local file operation off the async runtime.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| CoreError::internal(e.to_string()))?
}

pub(crate) async fn local_list_inner(path: String) -> Result<Listing> {
    blocking(move || local::list(&path)).await
}
pub(crate) async fn local_mkdir_inner(dir: String, name: String) -> Result<()> {
    blocking(move || local::mkdir(&dir, &name)).await
}
pub(crate) async fn local_rename_inner(dir: String, from: String, to: String) -> Result<()> {
    blocking(move || local::rename(&dir, &from, &to)).await
}
pub(crate) async fn local_trash_inner(state: &AppState, dir: String, names: Vec<String>) -> Result<()> {
    let root = state.paths.trash_dir();
    blocking(move || local::trash(&root, &dir, &names)).await
}
pub(crate) fn local_home_inner(state: &AppState, host_id: Option<Id>) -> Result<String> {
    let start = match host_id {
        Some(id) => find_host(state, id)?.sftp.local_start,
        None => None,
    };
    Ok(local::home(start.as_deref(), &state.paths.home))
}

#[tauri::command]
#[specta::specta]
pub async fn connect_sftp(
    state: State<'_, AppState>,
    connect_id: Uuid,
    host_id: Id,
    events: Channel<ConnectEvent>,
    sftp_events: Channel<SftpEvent>,
) -> Result<SftpConnected> {
    let on_lost: LostFn = Arc::new(move |error| {
        if let Err(e) = sftp_events.send(SftpEvent::Lost { error }) {
            tracing::debug!(error = %e, "sftp event dropped: channel closed");
        }
    });
    connect_sftp_inner(&state, connect_id, host_id, channel_emit(events), on_lost).await
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_list(state: State<'_, AppState>, session_id: SessionId, path: String) -> Result<Listing> {
    sftp_list_inner(&state, session_id, path).await
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_mkdir(state: State<'_, AppState>, session_id: SessionId, dir: String, name: String) -> Result<()> {
    session(&state, session_id)?.mkdir(&dir, &name).await
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    session_id: SessionId,
    dir: String,
    from: String,
    to: String,
) -> Result<()> {
    session(&state, session_id)?.rename(&dir, &from, &to).await
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_delete(
    state: State<'_, AppState>,
    session_id: SessionId,
    dir: String,
    names: Vec<String>,
) -> Result<()> {
    session(&state, session_id)?.delete(&dir, &names).await
}

/// Disconnects and forgets the session; unknown ids are fine (already closed).
#[tauri::command]
#[specta::specta]
pub async fn sftp_close(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    let removed = lock(&state.sftp).remove(&session_id);
    if let Some(session) = removed {
        session.close().await;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn local_list(path: String) -> Result<Listing> {
    local_list_inner(path).await
}

#[tauri::command]
#[specta::specta]
pub async fn local_home(state: State<'_, AppState>, host_id: Option<Id>) -> Result<String> {
    local_home_inner(&state, host_id)
}

#[tauri::command]
#[specta::specta]
pub async fn local_mkdir(dir: String, name: String) -> Result<()> {
    local_mkdir_inner(dir, name).await
}

#[tauri::command]
#[specta::specta]
pub async fn local_rename(dir: String, from: String, to: String) -> Result<()> {
    local_rename_inner(dir, from, to).await
}

#[tauri::command]
#[specta::specta]
pub async fn local_delete(dir: String, names: Vec<String>) -> Result<()> {
    blocking(move || local::delete(&dir, &names)).await
}

#[tauri::command]
#[specta::specta]
pub async fn local_trash(state: State<'_, AppState>, dir: String, names: Vec<String>) -> Result<()> {
    local_trash_inner(&state, dir, names).await
}
```

Make the used helpers in `commands/connect.rs` `pub(crate)` (`channel_emit`, `fail`, `connect_deps`); `find_host`, `build_request`, `register_connect` already are. If `SessionInfo`/`EmitFn` are not re-exported from `linascp_core::connect`, import them from where `connect.rs` does.

`commands/mod.rs`: `pub mod sftp;`. `lib.rs`: add `sftp` to the `use commands::{…}` list and after `connect::connect_test,`:

```rust
            sftp::connect_sftp,
            sftp::sftp_list,
            sftp::sftp_mkdir,
            sftp::sftp_rename,
            sftp::sftp_delete,
            sftp::sftp_close,
            sftp::local_list,
            sftp::local_home,
            sftp::local_mkdir,
            sftp::local_rename,
            sftp::local_delete,
            sftp::local_trash,
```

- [ ] **Step 4: Run tests, regenerate bindings**

Run: `cargo test -p linascp sftp:: 2>&1 | grep -E "test result|FAILED"` → PASS.
Run: `cargo test -p linascp exports_bindings` → `ui/src/ipc/bindings.ts` now contains `connectSftp`, `SftpConnected`, `Listing`, `Entry`, `FsKind`, `"subsystem"`, `"directory"`.
Run: `bun run typecheck 2>&1 | tail -5` → Expected FAIL: `steps.ts` switch not exhaustive (fixed in Task 6). That is fine for this commit only if Task 6 follows directly; otherwise add the two cases now (see Task 6 Step 3).

- [ ] **Step 5: Commit**

```bash
git add src-tauri ui/src/ipc/bindings.ts
git commit -m "feat(tauri): SFTP and local file commands

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: UI IPC wrappers, strings, step labels, entry helpers

**Files:**
- Modify: `ui/src/ipc/api.ts`, `ui/src/ipc/errors.ts`, `ui/src/screens/connect/steps.ts`, `ui/src/i18n/de.ts`
- Create: `ui/src/screens/sftp/entries.ts`, `ui/src/screens/sftp/entries.test.ts`

**Interfaces:**
- Consumes: generated bindings from Task 5.
- Produces:
  - api: `connectSftp(connectId, hostId, events: Channel<ConnectEvent>, sftpEvents: Channel<SftpEvent>): Promise<SftpConnected>`, `sftpList(sessionId, path): Promise<Listing>`, `sftpMkdir(sessionId, dir, name)`, `sftpRename(sessionId, dir, from, to)`, `sftpDelete(sessionId, dir, names)`, `sftpClose(sessionId)`, `localList(path)`, `localHome(hostId: Id | null)`, `localMkdir(dir, name)`, `localRename(dir, from, to)`, `localDelete(dir, names)`, `localTrash(dir, names)`; type exports `Entry, EntryKind, FsKind, LinkTarget, Listing, SftpConnected, SftpEvent`.
  - `entries.ts`: `type SortKey = "name" | "size" | "mtime" | "mode" | "owner"`, `interface Sort { key: SortKey; dir: "asc" | "desc" }`, `isDirLike(e: Entry): boolean`, `isHidden(e: Entry): boolean`, `sortEntries(entries: Entry[], sort: Sort): Entry[]`, `visibleEntries(entries: Entry[], showHidden: boolean, sort: Sort): Entry[]`, `formatSize(bytes: number): string`, `formatMtime(seconds: number | null): string`, `formatMode(mode: number | null): string`, `parentPath(path: string): string`, `joinPath(dir: string, name: string): string`, `crumbs(path: string): { label: string; path: string }[]`
  - `t.sftp` strings (see Step 3), `t.pipeline.steps.subsystem`, `.directory`, `t.sftp.fsErrors`.

- [ ] **Step 1: Write the failing tests** — `ui/src/screens/sftp/entries.test.ts`

```ts
import { describe, expect, it } from "vitest";
import type { Entry } from "../../ipc/api";
import { crumbs, formatMode, formatMtime, formatSize, joinPath, parentPath, sortEntries, visibleEntries } from "./entries";

const e = (name: string, kind: Entry["kind"] = { type: "file" }, size = 0, mtime = 0): Entry => ({
  name, kind, size, mtime, mode: 0o100644, owner: "u", nameLossy: false,
});

describe("entries", () => {
  it("sorts folders and folder links first, names naturally and case-insensitively", () => {
    const list = [e("file10"), e("File2"), e("b-dir", { type: "dir" }), e("a-link", { type: "symlink", target: "dir" }), e("z")];
    expect(sortEntries(list, { key: "name", dir: "asc" }).map((x) => x.name)).toEqual(["a-link", "b-dir", "File2", "file10", "z"]);
    expect(sortEntries(list, { key: "name", dir: "desc" }).map((x) => x.name)).toEqual(["b-dir", "a-link", "z", "file10", "File2"]);
  });

  it("sorts by size with folders still first", () => {
    const list = [e("big", { type: "file" }, 10), e("d", { type: "dir" }, 4096), e("small", { type: "file" }, 1)];
    expect(sortEntries(list, { key: "size", dir: "asc" }).map((x) => x.name)).toEqual(["d", "small", "big"]);
  });

  it("hides dot files unless asked", () => {
    const list = [e(".env"), e("a")];
    expect(visibleEntries(list, false, { key: "name", dir: "asc" }).map((x) => x.name)).toEqual(["a"]);
    expect(visibleEntries(list, true, { key: "name", dir: "asc" })).toHaveLength(2);
  });

  it("formats sizes, dates and modes the German way", () => {
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(1536)).toBe("1,5 KB");
    expect(formatSize(19293798)).toBe("18,4 MB");
    expect(formatMtime(null)).toBe("");
    expect(formatMtime(new Date(2026, 9, 8, 10, 41).getTime() / 1000)).toBe("08.10.2026 10:41");
    expect(formatMode(0o100755)).toBe("rwxr-xr-x");
    expect(formatMode(0o104750)).toBe("rwsr-x---");
    expect(formatMode(null)).toBe("");
  });

  it("builds paths and breadcrumbs", () => {
    expect(parentPath("/var/www")).toBe("/var");
    expect(parentPath("/var")).toBe("/");
    expect(parentPath("/")).toBe("/");
    expect(joinPath("/", "etc")).toBe("/etc");
    expect(joinPath("/var", "www")).toBe("/var/www");
    expect(crumbs("/var/www")).toEqual([
      { label: "/", path: "/" },
      { label: "var", path: "/var" },
      { label: "www", path: "/var/www" },
    ]);
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `bun run test -- entries 2>&1 | tail -5` → FAIL (module not found).

- [ ] **Step 3: Implement**

`ui/src/screens/sftp/entries.ts`:

```ts
import type { Entry } from "../../ipc/api";

export type SortKey = "name" | "size" | "mtime" | "mode" | "owner";
export interface Sort {
  key: SortKey;
  dir: "asc" | "desc";
}

const collator = new Intl.Collator("de", { numeric: true, sensitivity: "base" });

export function isDirLike(entry: Entry): boolean {
  return entry.kind.type === "dir" || (entry.kind.type === "symlink" && entry.kind.target === "dir");
}

export function isHidden(entry: Entry): boolean {
  return entry.name.startsWith(".");
}

function compare(a: Entry, b: Entry, key: SortKey): number {
  switch (key) {
    case "name":
      return collator.compare(a.name, b.name);
    case "size":
      return a.size - b.size;
    case "mtime":
      return (a.mtime ?? 0) - (b.mtime ?? 0);
    case "mode":
      return (a.mode ?? 0) - (b.mode ?? 0);
    case "owner":
      return collator.compare(a.owner ?? "", b.owner ?? "");
  }
}

/** Folders (and links to folders) first; then by key, ties by name. */
export function sortEntries(entries: readonly Entry[], sort: Sort): Entry[] {
  const sign = sort.dir === "asc" ? 1 : -1;
  return [...entries].sort((a, b) => {
    const dirs = Number(isDirLike(b)) - Number(isDirLike(a));
    if (dirs !== 0) return dirs;
    return sign * (compare(a, b, sort.key) || collator.compare(a.name, b.name));
  });
}

export function visibleEntries(entries: readonly Entry[], showHidden: boolean, sort: Sort): Entry[] {
  return sortEntries(showHidden ? entries : entries.filter((entry) => !isHidden(entry)), sort);
}

const number1 = new Intl.NumberFormat("de", { maximumFractionDigits: 1 });
const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatSize(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${unit === 0 ? value : number1.format(value)} ${UNITS[unit]}`;
}

const pad = (n: number) => String(n).padStart(2, "0");

export function formatMtime(seconds: number | null): string {
  if (seconds === null) return "";
  const d = new Date(seconds * 1000);
  return `${pad(d.getDate())}.${pad(d.getMonth() + 1)}.${d.getFullYear()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatMode(mode: number | null): string {
  if (mode === null) return "";
  const triple = (bits: number, special: boolean, specialChar: string) =>
    `${bits & 4 ? "r" : "-"}${bits & 2 ? "w" : "-"}${special ? (bits & 1 ? specialChar : specialChar.toUpperCase()) : bits & 1 ? "x" : "-"}`;
  return (
    triple((mode >> 6) & 7, (mode & 0o4000) !== 0, "s") +
    triple((mode >> 3) & 7, (mode & 0o2000) !== 0, "s") +
    triple(mode & 7, (mode & 0o1000) !== 0, "t")
  );
}

export function parentPath(path: string): string {
  const cut = path.replace(/\/+$/, "").lastIndexOf("/");
  return cut <= 0 ? "/" : path.slice(0, cut);
}

export function joinPath(dir: string, name: string): string {
  return `${dir.replace(/\/+$/, "")}/${name}`;
}

export function crumbs(path: string): { label: string; path: string }[] {
  const parts = path.split("/").filter(Boolean);
  return [{ label: "/", path: "/" }, ...parts.map((label, i) => ({ label, path: `/${parts.slice(0, i + 1).join("/")}` }))];
}
```

`api.ts` — add to the `bindings` type import and the `export type { … }` list: `Entry, EntryKind, FsKind, LinkTarget, Listing, SftpConnected, SftpEvent`. Wrappers after `connectTest`:

```ts
// SFTP commander
export function connectSftp(
  connectId: Id,
  hostId: Id,
  events: Channel<ConnectEvent>,
  sftpEvents: Channel<SftpEvent>,
): Promise<SftpConnected> {
  return unwrap(commands.connectSftp(connectId, hostId, events, sftpEvents));
}
export function sftpList(sessionId: SessionId, path: string): Promise<Listing> {
  return unwrap(commands.sftpList(sessionId, path));
}
export function sftpMkdir(sessionId: SessionId, dir: string, name: string): Promise<void> {
  return unwrapVoid(commands.sftpMkdir(sessionId, dir, name));
}
export function sftpRename(sessionId: SessionId, dir: string, from: string, to: string): Promise<void> {
  return unwrapVoid(commands.sftpRename(sessionId, dir, from, to));
}
export function sftpDelete(sessionId: SessionId, dir: string, names: string[]): Promise<void> {
  return unwrapVoid(commands.sftpDelete(sessionId, dir, names));
}
export function sftpClose(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sftpClose(sessionId));
}
export function localList(path: string): Promise<Listing> {
  return unwrap(commands.localList(path));
}
export function localHome(hostId: Id | null): Promise<string> {
  return unwrap(commands.localHome(hostId));
}
export function localMkdir(dir: string, name: string): Promise<void> {
  return unwrapVoid(commands.localMkdir(dir, name));
}
export function localRename(dir: string, from: string, to: string): Promise<void> {
  return unwrapVoid(commands.localRename(dir, from, to));
}
export function localDelete(dir: string, names: string[]): Promise<void> {
  return unwrapVoid(commands.localDelete(dir, names));
}
export function localTrash(dir: string, names: string[]): Promise<void> {
  return unwrapVoid(commands.localTrash(dir, names));
}
```

If the generated `commands.*` parameter order differs, follow the generated signature. If tests mock `ipc/api` with a manual `__mocks__`, add the new functions there.

`errors.ts`: add `"fs"` to `CORE_ERROR_TYPES`.

`steps.ts` `stepLabel`: add

```ts
    case "subsystem":
      return t.pipeline.steps.subsystem;
    case "directory":
      return t.pipeline.steps.directory;
```

`de.ts`: in `pipeline.steps`: `subsystem: "SFTP-Subsystem starten", directory: "Verzeichnis laden",`. Remove `stubs.sftp`/`stubs.sftpText` only in Task 9. New block `sftp`:

```ts
  sftp: {
    title: "SFTP-Dateimanager",
    tabs: "SFTP-Sitzungen",
    newTab: "Neue SFTP-Sitzung",
    closeTab: (name: string) => `${name} schließen`,
    pickHost: "Host für SFTP wählen",
    noSftpHosts: "Kein Host mit SFTP vorhanden.",
    local: "Lokal",
    remote: "Remote",
    toolbar: "Dateiaktionen",
    newFolder: "Neuer Ordner",
    rename: "Umbenennen",
    delete: "Löschen",
    reload: "Neu laden",
    hidden: "Versteckte",
    terminal: "Terminal",
    open: "Öffnen",
    up: "..",
    columns: { name: "Name", size: "Größe", mtime: "Geändert", mode: "Rechte", owner: "Besitzer" },
    path: "Pfad",
    loading: "Lade …",
    empty: "Ordner ist leer",
    lossyName: "Name ist kein gültiges UTF-8 – Aktionen nicht möglich",
    brokenLink: "Ziel des Links fehlt",
    lost: "Verbindung verloren",
    reconnect: "Neu verbinden",
    status: (userHost: string, version: number, cipher: string) => `${userHost} · SFTP v${version} · ${cipher}`,
    nameDialog: { newTitle: "Neuer Ordner", renameTitle: "Umbenennen", label: "Name", create: "Anlegen", save: "Umbenennen" },
    nameErrors: {
      empty: "Bitte einen Namen eingeben.",
      slash: "Der Name darf kein / enthalten.",
      dots: "„.“ und „..“ sind keine gültigen Namen.",
      exists: "Dieser Name existiert bereits.",
    },
    deleteDialog: {
      permanentTitle: "Endgültig löschen?",
      trashTitle: "In den Papierkorb verschieben?",
      one: (name: string) => `„${name}“ wird gelöscht.`,
      many: (count: number) => `${count} Einträge werden gelöscht.`,
      trashOne: (name: string) => `„${name}“ wird in den Papierkorb verschoben.`,
      trashMany: (count: number) => `${count} Einträge werden in den Papierkorb verschoben.`,
      folderHint: "Der Ordner wird mitsamt Inhalt entfernt.",
      confirm: "Löschen",
      confirmTrash: "In den Papierkorb",
    },
    fsErrors: {
      permissionDenied: "Keine Berechtigung",
      notFound: "Nicht gefunden",
      alreadyExists: "Existiert bereits",
      notEmpty: "Ordner ist nicht leer",
      other: "Dateifehler",
    },
    failed: "Aktion fehlgeschlagen",
    details: "Details",
  },
```

- [ ] **Step 4: Run tests**

Run: `bun run test -- entries 2>&1 | tail -3` → PASS. `bun run typecheck` → clean.

- [ ] **Step 5: Commit**

```bash
git add ui/src
git commit -m "feat(ui): SFTP IPC wrappers, strings, step labels and entry helpers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: `useSftpStore` and connect integration

**Files:**
- Create: `ui/src/stores/sftp.ts`, `ui/src/stores/sftp.test.ts`
- Modify: `ui/src/stores/connect.ts`, `ui/src/stores/connect.test.ts`

**Interfaces:**
- Consumes: Task 6 api + `entries.ts`.
- Produces:
  ```ts
  export type Side = "local" | "remote";
  export interface PaneState {
    path: string; entries: Entry[]; loading: boolean; error?: CoreError;
    selection: string[]; cursor: string | null; anchor: string | null;
    sort: Sort; showHidden: boolean; editingPath: boolean;
    /** Increments per navigate; a response for an older token is dropped. */
    token: number;
  }
  export interface SftpTab {
    id: string; hostId: Id; sessionId: string; title: string;
    status: "connected" | "lost"; lostError?: CoreError; info: SessionInfo; version: number;
    panes: Record<Side, PaneState>; active: Side;
  }
  export interface SftpState {
    tabs: SftpTab[]; activeTabId: string | null;
    openTab(tabId: string, hostId: Id, connected: SftpConnected): void;
    attachSession(tabId: string, connected: SftpConnected): boolean;
    closeTab(tabId: string): void;
    setActiveTab(tabId: string): void;
    setActivePane(tabId: string, side: Side): void;
    markLost(tabId: string, sessionId: string, error: CoreError): void;
    reconnect(tabId: string): void;
    navigate(tabId: string, side: Side, path: string, focus?: string): Promise<void>;
    reload(tabId: string, side: Side, focus?: string): Promise<void>;
    setSort(tabId: string, side: Side, key: SortKey): void;
    toggleHidden(tabId: string, side: Side): void;
    setEditingPath(tabId: string, side: Side, on: boolean): void;
    select(tabId: string, side: Side, patch: Pick<PaneState, "selection" | "cursor" | "anchor">): void;
    mkdir(tabId: string, side: Side, name: string): Promise<void>;          // rejects with CoreError
    rename(tabId: string, side: Side, from: string, to: string): Promise<void>; // rejects with CoreError
    remove(tabId: string, side: Side, names: string[], mode: "trash" | "permanent"): Promise<void>; // never rejects; error -> pane.error
  }
  export const useSftpStore;
  ```
  - `ConnectFlow.sftpTabId?: string`; `start(hostId, protocol, target?: { paneId: string } | { sftpTabId: string })`.

- [ ] **Step 1: Write the failing tests** — `ui/src/stores/sftp.test.ts`

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import type { Entry, Listing, SftpConnected } from "../ipc/api";
import { makeHost, makeSessionInfo } from "../test/fixtures";
import { useConnectStore } from "./connect";
import { useHostsStore } from "./hosts";
import { useSftpStore } from "./sftp";

vi.mock("../ipc/api");

const file = (name: string): Entry => ({ name, kind: { type: "file" }, size: 1, mtime: 0, mode: 0o100644, owner: "u", nameLossy: false });
const listing = (path: string, names: string[] = []): Listing => ({ path, entries: names.map(file) });
const connected = (sessionId = "s1", path = "/home/u", names = ["a", "b", "c"]): SftpConnected => ({
  sessionId, info: makeSessionInfo(), version: 3, listing: listing(path, names),
});
const tab = () => useSftpStore.getState().tabs[0]!;

beforeEach(() => {
  vi.mocked(api.localHome).mockResolvedValue("/home/me");
  vi.mocked(api.localList).mockImplementation(async (path) => listing(path, ["l1"]));
  vi.mocked(api.sftpList).mockImplementation(async (_s, path) => listing(path, ["x"]));
  vi.mocked(api.sftpClose).mockResolvedValue();
  useSftpStore.setState(useSftpStore.getInitialState(), true);
  useHostsStore.setState({ ...useHostsStore.getInitialState(), hosts: [makeHost({ id: "h1", name: "web" })] }, true);
});

describe("useSftpStore", () => {
  it("opens a tab with the remote listing and loads the local home", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    expect(tab().panes.remote.path).toBe("/home/u");
    expect(tab().title).toBe("web");
    await vi.waitFor(() => expect(tab().panes.local.path).toBe("/home/me"));
    expect(api.localHome).toHaveBeenCalledWith("h1");
    expect(useSftpStore.getState().activeTabId).toBe("t1");
  });

  it("numbers tabs of the same host", () => {
    useSftpStore.getState().openTab("t1", "h1", connected("s1"));
    useSftpStore.getState().openTab("t2", "h1", connected("s2"));
    expect(useSftpStore.getState().tabs.map((x) => x.title)).toEqual(["web", "web 2"]);
  });

  it("keeps the old path and shows the error when navigating fails", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    const denied = { type: "fs", kind: "permissionDenied", path: "/root", message: "denied" } as const;
    vi.mocked(api.sftpList).mockRejectedValueOnce(denied);
    await useSftpStore.getState().navigate("t1", "remote", "/root");
    expect(tab().panes.remote.path).toBe("/home/u");
    expect(tab().panes.remote.error).toEqual(denied);
    expect(tab().panes.remote.entries.map((e) => e.name)).toEqual(["a", "b", "c"]);
  });

  it("a stale listing does not overwrite a newer navigation", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    let releaseSlow!: (l: Listing) => void;
    vi.mocked(api.sftpList)
      .mockImplementationOnce(() => new Promise((resolve) => { releaseSlow = resolve; }))
      .mockImplementationOnce(async (_s, path) => listing(path, ["fast"]));
    const slow = useSftpStore.getState().navigate("t1", "remote", "/slow");
    await useSftpStore.getState().navigate("t1", "remote", "/fast");
    releaseSlow(listing("/slow", ["slow"]));
    await slow;
    expect(tab().panes.remote.path).toBe("/fast");
  });

  it("puts the cursor on the neighbour after deleting", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    vi.mocked(api.sftpDelete).mockResolvedValue();
    vi.mocked(api.sftpList).mockResolvedValueOnce(listing("/home/u", ["a", "c"]));
    useSftpStore.getState().select("t1", "remote", { selection: [], cursor: "b", anchor: "b" });
    await useSftpStore.getState().remove("t1", "remote", ["b"], "permanent");
    expect(api.sftpDelete).toHaveBeenCalledWith("s1", "/home/u", ["b"]);
    expect(tab().panes.remote.cursor).toBe("c");
  });

  it("uses the trash for local deletes in trash mode", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    await vi.waitFor(() => expect(tab().panes.local.path).toBe("/home/me"));
    vi.mocked(api.localTrash).mockResolvedValue();
    await useSftpStore.getState().remove("t1", "local", ["l1"], "trash");
    expect(api.localTrash).toHaveBeenCalledWith("/home/me", ["l1"]);
  });

  it("rename rejects with the core error so the dialog can show it", async () => {
    useSftpStore.getState().openTab("t1", "h1", connected());
    const exists = { type: "fs", kind: "alreadyExists", path: "/home/u/b", message: "x" } as const;
    vi.mocked(api.sftpRename).mockRejectedValueOnce(exists);
    await expect(useSftpStore.getState().rename("t1", "remote", "a", "b")).rejects.toEqual(exists);
  });

  it("marks only the bound session as lost and reconnects into the same tab", async () => {
    const start = vi.fn(async () => undefined);
    useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
    useSftpStore.getState().openTab("t1", "h1", connected("s1"));
    useSftpStore.getState().markLost("t1", "other", { type: "cancelled" });
    expect(tab().status).toBe("connected");
    useSftpStore.getState().markLost("t1", "s1", { type: "network", kind: "other", message: "lost" });
    expect(tab().status).toBe("lost");
    useSftpStore.getState().reconnect("t1");
    expect(start).toHaveBeenCalledWith("h1", "sftp", { sftpTabId: "t1" });
    await useSftpStore.getState().navigate("t1", "remote", "/home/u/deep");
    expect(useSftpStore.getState().attachSession("t1", connected("s2", "/home/u"))).toBe(true);
    expect(tab().status).toBe("connected");
    expect(tab().sessionId).toBe("s2");
    await vi.waitFor(() => expect(api.sftpList).toHaveBeenLastCalledWith("s2", "/home/u/deep"));
  });

  it("reconnect into a closed tab closes the new session", () => {
    expect(useSftpStore.getState().attachSession("gone", connected("s9"))).toBe(false);
  });

  it("closing a tab closes its session", () => {
    useSftpStore.getState().openTab("t1", "h1", connected("s1"));
    useSftpStore.getState().closeTab("t1");
    expect(api.sftpClose).toHaveBeenCalledWith("s1");
    expect(useSftpStore.getState().tabs).toEqual([]);
  });
});
```

In `connect.test.ts` add (follow that file's existing setup/helpers for hosts and api mocks):

```ts
  it("opens an SFTP tab after an sftp connect", async () => {
    vi.mocked(api.connectSftp).mockResolvedValue({ sessionId: "s1", info: makeSessionInfo(), version: 3, listing: { path: "/home/u", entries: [] } });
    vi.mocked(api.localHome).mockResolvedValue("/home/me");
    vi.mocked(api.localList).mockResolvedValue({ path: "/home/me", entries: [] });
    await useConnectStore.getState().start(hostId, "sftp");
    expect(useConnectStore.getState().active).toBeNull();
    expect(useUiStore.getState().view).toBe("sftp");
    expect(useSftpStore.getState().tabs).toHaveLength(1);
    expect(api.connectSsh).not.toHaveBeenCalled();
  });

  it("cancelling an sftp reconnect stays in the sftp view", async () => {
    vi.mocked(api.connectSftp).mockReturnValue(new Promise(() => {}));
    void useConnectStore.getState().start(hostId, "sftp", { sftpTabId: "t1" });
    await useConnectStore.getState().cancel();
    expect(useUiStore.getState().view).toBe("sftp");
  });
```

(`hostId` = the id of a host the file's fixtures already register; import `useSftpStore`.)

- [ ] **Step 2: Run to verify they fail**

Run: `bun run test -- stores/sftp stores/connect 2>&1 | tail -8` → FAIL (module missing, sftp branch missing).

- [ ] **Step 3: Implement** — `ui/src/stores/sftp.ts`

```ts
import { create } from "zustand";
import {
  type CoreError,
  type Entry,
  type Id,
  localDelete,
  localHome,
  localList,
  localMkdir,
  localRename,
  localTrash,
  type Listing,
  type SessionInfo,
  type SftpConnected,
  sftpClose,
  sftpDelete,
  sftpList,
  sftpMkdir,
  sftpRename,
} from "../ipc/api";
import { isCoreError } from "../ipc/errors";
import { type Sort, type SortKey, visibleEntries } from "../screens/sftp/entries";
import { fireAndForget } from "./connectProgress";
import { useConnectStore } from "./connect";
import { useHostsStore } from "./hosts";
import { useUiStore } from "./ui";

export type Side = "local" | "remote";

export interface PaneState {
  path: string;
  entries: Entry[];
  loading: boolean;
  error?: CoreError;
  selection: string[];
  cursor: string | null;
  anchor: string | null;
  sort: Sort;
  showHidden: boolean;
  editingPath: boolean;
  /** Increments per navigate; a response for an older token is dropped. */
  token: number;
}

export interface SftpTab {
  id: string;
  hostId: Id;
  sessionId: string;
  title: string;
  status: "connected" | "lost";
  lostError?: CoreError;
  info: SessionInfo;
  version: number;
  panes: Record<Side, PaneState>;
  active: Side;
}

export interface SftpState {
  tabs: SftpTab[];
  activeTabId: string | null;
  openTab(tabId: string, hostId: Id, connected: SftpConnected): void;
  attachSession(tabId: string, connected: SftpConnected): boolean;
  closeTab(tabId: string): void;
  setActiveTab(tabId: string): void;
  setActivePane(tabId: string, side: Side): void;
  markLost(tabId: string, sessionId: string, error: CoreError): void;
  reconnect(tabId: string): void;
  navigate(tabId: string, side: Side, path: string, focus?: string): Promise<void>;
  reload(tabId: string, side: Side, focus?: string): Promise<void>;
  setSort(tabId: string, side: Side, key: SortKey): void;
  toggleHidden(tabId: string, side: Side): void;
  setEditingPath(tabId: string, side: Side, on: boolean): void;
  select(tabId: string, side: Side, patch: Pick<PaneState, "selection" | "cursor" | "anchor">): void;
  mkdir(tabId: string, side: Side, name: string): Promise<void>;
  rename(tabId: string, side: Side, from: string, to: string): Promise<void>;
  remove(tabId: string, side: Side, names: string[], mode: "trash" | "permanent"): Promise<void>;
}

const asError = (error: unknown): CoreError =>
  isCoreError(error) ? error : { type: "internal", message: String(error) };

function pane(listing: Listing | null): PaneState {
  return {
    path: listing?.path ?? "",
    entries: listing?.entries ?? [],
    loading: listing === null,
    selection: [],
    cursor: null,
    anchor: null,
    sort: { key: "name", dir: "asc" },
    showHidden: false,
    editingPath: false,
    token: 0,
  };
}

export const useSftpStore = create<SftpState>()((set, get) => {
  const findTab = (tabId: string) => get().tabs.find((tab) => tab.id === tabId);
  const patchTab = (tabId: string, change: (tab: SftpTab) => Partial<SftpTab>) =>
    set((state) => ({ tabs: state.tabs.map((tab) => (tab.id === tabId ? { ...tab, ...change(tab) } : tab)) }));
  const patchPane = (tabId: string, side: Side, change: (p: PaneState) => Partial<PaneState>) =>
    patchTab(tabId, (tab) => ({ panes: { ...tab.panes, [side]: { ...tab.panes[side], ...change(tab.panes[side]) } } }));

  const list = (tab: SftpTab, side: Side, path: string) =>
    side === "remote" ? sftpList(tab.sessionId, path) : localList(path);

  /** Cursor after a reload: `focus` if present, else the old cursor, else its next visible neighbour. */
  const cursorAfter = (before: PaneState, entries: Entry[], focus?: string): string | null => {
    const names = new Set(entries.map((e) => e.name));
    if (focus && names.has(focus)) return focus;
    if (before.cursor && names.has(before.cursor)) return before.cursor;
    const old = visibleEntries(before.entries, before.showHidden, before.sort).map((e) => e.name);
    const index = before.cursor ? old.indexOf(before.cursor) : -1;
    for (const name of [...old.slice(index + 1), ...old.slice(0, Math.max(index, 0)).reverse()]) {
      if (names.has(name)) return name;
    }
    return null;
  };

  const titleFor = (hostId: Id) => {
    const name = useHostsStore.getState().hosts.find((h) => h.id === hostId)?.name ?? hostId;
    const taken = new Set(get().tabs.map((tab) => tab.title));
    let n = 1;
    const label = (i: number) => (i === 1 ? name : `${name} ${i}`);
    while (taken.has(label(n))) n++;
    return label(n);
  };

  const loadLocalHome = async (tabId: string, hostId: Id) => {
    try {
      await get().navigate(tabId, "local", await localHome(hostId));
    } catch (error) {
      patchPane(tabId, "local", () => ({ loading: false, error: asError(error) }));
    }
  };

  return {
    tabs: [],
    activeTabId: null,

    openTab: (tabId, hostId, connected) => {
      const tab: SftpTab = {
        id: tabId,
        hostId,
        sessionId: connected.sessionId,
        title: titleFor(hostId),
        status: "connected",
        info: connected.info,
        version: connected.version,
        panes: { local: pane(null), remote: pane(connected.listing) },
        active: "remote",
      };
      set((state) => ({ tabs: [...state.tabs, tab], activeTabId: tabId }));
      void loadLocalHome(tabId, hostId);
    },

    attachSession: (tabId, connected) => {
      const tab = findTab(tabId);
      if (!tab) return false;
      const lastPath = tab.panes.remote.path;
      patchTab(tabId, () => ({ sessionId: connected.sessionId, status: "connected", lostError: undefined, info: connected.info, version: connected.version }));
      patchPane(tabId, "remote", () => ({ path: connected.listing.path, entries: connected.listing.entries, error: undefined }));
      // Back to where the user was; if that is gone, the start directory stays.
      if (lastPath && lastPath !== connected.listing.path) void get().navigate(tabId, "remote", lastPath);
      return true;
    },

    closeTab: (tabId) => {
      const tab = findTab(tabId);
      if (!tab) return;
      fireAndForget(() => sftpClose(tab.sessionId));
      set((state) => {
        const tabs = state.tabs.filter((x) => x.id !== tabId);
        const activeTabId = state.activeTabId === tabId ? (tabs.at(-1)?.id ?? null) : state.activeTabId;
        return { tabs, activeTabId };
      });
    },

    setActiveTab: (tabId) => set({ activeTabId: tabId }),
    setActivePane: (tabId, side) => patchTab(tabId, () => ({ active: side })),

    markLost: (tabId, sessionId, error) => {
      if (findTab(tabId)?.sessionId !== sessionId) return;
      patchTab(tabId, () => ({ status: "lost", lostError: error }));
    },

    reconnect: (tabId) => {
      const tab = findTab(tabId);
      if (!tab) return;
      fireAndForget(() => sftpClose(tab.sessionId));
      void useConnectStore.getState().start(tab.hostId, "sftp", { sftpTabId: tabId });
    },

    navigate: async (tabId, side, path, focus) => {
      const tab = findTab(tabId);
      if (!tab) return;
      const token = tab.panes[side].token + 1;
      patchPane(tabId, side, () => ({ loading: true, token, editingPath: false }));
      try {
        const listing = await list(tab, side, path);
        const now = findTab(tabId)?.panes[side];
        if (!now || now.token !== token) return;
        const moved = listing.path !== now.path;
        patchPane(tabId, side, (p) => ({
          path: listing.path,
          entries: listing.entries,
          loading: false,
          error: undefined,
          selection: moved ? [] : p.selection.filter((n) => listing.entries.some((e) => e.name === n)),
          cursor: moved ? (focus ?? null) : cursorAfter(p, listing.entries, focus),
          anchor: moved ? null : p.anchor,
        }));
      } catch (error) {
        if (findTab(tabId)?.panes[side].token !== token) return;
        patchPane(tabId, side, () => ({ loading: false, error: asError(error) }));
      }
    },

    reload: (tabId, side, focus) => {
      const tab = findTab(tabId);
      return tab ? get().navigate(tabId, side, tab.panes[side].path, focus) : Promise.resolve();
    },

    setSort: (tabId, side, key) =>
      patchPane(tabId, side, (p) => ({ sort: { key, dir: p.sort.key === key && p.sort.dir === "asc" ? "desc" : "asc" } })),
    toggleHidden: (tabId, side) => patchPane(tabId, side, (p) => ({ showHidden: !p.showHidden })),
    setEditingPath: (tabId, side, on) => patchPane(tabId, side, () => ({ editingPath: on })),
    select: (tabId, side, patch) => patchPane(tabId, side, () => patch),

    mkdir: async (tabId, side, name) => {
      const tab = findTab(tabId);
      if (!tab) return;
      const dir = tab.panes[side].path;
      await (side === "remote" ? sftpMkdir(tab.sessionId, dir, name) : localMkdir(dir, name));
      await get().reload(tabId, side, name);
    },

    rename: async (tabId, side, from, to) => {
      const tab = findTab(tabId);
      if (!tab) return;
      const dir = tab.panes[side].path;
      await (side === "remote" ? sftpRename(tab.sessionId, dir, from, to) : localRename(dir, from, to));
      await get().reload(tabId, side, to);
    },

    remove: async (tabId, side, names, mode) => {
      const tab = findTab(tabId);
      if (!tab) return;
      const dir = tab.panes[side].path;
      try {
        if (side === "remote") await sftpDelete(tab.sessionId, dir, names);
        else if (mode === "trash") await localTrash(dir, names);
        else await localDelete(dir, names);
        patchPane(tabId, side, () => ({ selection: [] }));
        await get().reload(tabId, side);
      } catch (error) {
        await get().reload(tabId, side);
        patchPane(tabId, side, () => ({ error: asError(error) }));
      }
    },
  };
});

/** View helper used by the screen and the connect store. */
export function showSftp(): void {
  useUiStore.getState().setView("sftp");
}
```

Note: `remove` sets `cursor` via `cursorAfter` because the reload's listing has the same path (not `moved`). Drop `showSftp` if unused at the end of Task 9.

`ui/src/stores/connect.ts`:

1. `ConnectFlow`: add `/** SFTP reconnect: the tab that receives the new session. */ sftpTabId?: string;`
2. `start` signature: `start(hostId: Id, protocol: ConnectProtocol, target?: { paneId: string } | { sftpTabId: string }): Promise<void>;`; in the flow literal: `paneId: target && "paneId" in target ? target.paneId : undefined, sftpTabId: target && "sftpTabId" in target ? target.sftpTabId : undefined,`.
3. Replace the three home computations (`onFailure`'s `home`, `cancel`'s `close(...)`) with a helper:

```ts
/** Where a cancelled/rejected flow returns to. */
function homeView(flow: ConnectFlow | null): View {
  if (flow?.paneId) return "terminal";
  if (flow?.sftpTabId) return "sftp";
  return "hosts";
}
```

4. In `run`, first line inside `try` before `tabId = newId()`: `if (flow.protocol === "sftp") return await runSftp(flow);` where (inside the store factory, next to `run`):

```ts
  const runSftp = async (flow: ConnectFlow) => {
    const { connectId } = flow;
    const tabId = flow.sftpTabId ?? newId();
    let bound: string | null = null;
    const events = createEventChannel<ConnectEvent>((event) => onEvent(connectId, event));
    const sftpEvents = createEventChannel<SftpEvent>((event) => {
      if (bound !== null) useSftpStore.getState().markLost(tabId, bound, event.error);
    });
    const connected = await connectSftp(connectId, flow.hostId, events, sftpEvents);
    if (!isCurrent(connectId)) {
      fireAndForget(() => sftpClose(connected.sessionId));
      return;
    }
    bound = connected.sessionId;
    const sftp = useSftpStore.getState();
    if (!flow.sftpTabId) sftp.openTab(tabId, flow.hostId, connected);
    else if (!sftp.attachSession(flow.sftpTabId, connected)) fireAndForget(() => sftpClose(connected.sessionId));
    close("sftp");
  };
```

(`runSftp` throws on rejection; the surrounding `try/catch` in `run` routes it to `onFailure`. Keep the `catch`'s `discardEarlySessionEvent` guarded by `tabId !== null`, which stays `null` on the SFTP path.) Imports: `connectSftp, sftpClose, type SftpEvent` from api; `useSftpStore` from `./sftp`. `sftp.ts` imports `useConnectStore` too: the cycle is fine because both only use each other inside functions.

- [ ] **Step 4: Run tests**

Run: `bun run test -- stores 2>&1 | tail -5` → PASS, no warnings. `bun run typecheck` clean.

- [ ] **Step 5: Commit**

```bash
git add ui/src/stores
git commit -m "feat(ui): SFTP store and sftp connect flow with reconnect into the same tab

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: `FilePane` (table, breadcrumbs, path field, keyboard, context menu)

**Files:**
- Create: `ui/src/screens/sftp/FilePane.tsx`, `ui/src/screens/sftp/FilePane.module.css`, `ui/src/screens/sftp/FilePane.test.tsx`, `ui/src/screens/sftp/paneKeys.ts`, `ui/src/screens/sftp/paneKeys.test.ts`

**Interfaces:**
- Consumes: `useSftpStore` (Task 7), `entries.ts` (Task 6).
- Produces:
  - `export type PaneCommand = "newFolder" | "rename" | "delete" | "deletePermanent";`
  - `export function FilePane(props: { tabId: string; side: Side; disabled?: boolean; onCommand: (command: PaneCommand, names: string[]) => void }): JSX.Element`
  - `paneKeys.ts`: `export function moveCursor(names: string[], cursor: string | null, key: string, page: number): string | null`, `export function rangeBetween(names: string[], a: string, b: string): string[]`, `export function typeAhead(names: string[], prefix: string, from: string | null): string | null`, `export function targets(pane: PaneState): string[]` (selection, else cursor, never `..`, never lossy).

- [ ] **Step 1: Write the failing tests**

`paneKeys.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { moveCursor, rangeBetween, typeAhead } from "./paneKeys";

const names = ["..", "a", "b", "c", "d"];

describe("paneKeys", () => {
  it("moves the cursor with arrows, Home/End and pages", () => {
    expect(moveCursor(names, null, "ArrowDown", 2)).toBe("..");
    expect(moveCursor(names, "a", "ArrowDown", 2)).toBe("b");
    expect(moveCursor(names, "a", "ArrowUp", 2)).toBe("..");
    expect(moveCursor(names, "..", "ArrowUp", 2)).toBe("..");
    expect(moveCursor(names, "b", "End", 2)).toBe("d");
    expect(moveCursor(names, "b", "Home", 2)).toBe("..");
    expect(moveCursor(names, "a", "PageDown", 2)).toBe("c");
    expect(moveCursor(names, "d", "PageDown", 2)).toBe("d");
  });

  it("selects ranges in both directions without ..", () => {
    expect(rangeBetween(names, "d", "b")).toEqual(["b", "c", "d"]);
    expect(rangeBetween(names, "..", "b")).toEqual(["a", "b"]);
  });

  it("jumps to the first name with the typed prefix after the cursor, wrapping", () => {
    expect(typeAhead(["..", "apple", "banana", "berry"], "b", null)).toBe("banana");
    expect(typeAhead(["..", "apple", "banana", "berry"], "be", "banana")).toBe("berry");
    expect(typeAhead(["..", "apple", "Banana"], "b", "apple")).toBe("Banana");
    expect(typeAhead(["..", "apple"], "z", "apple")).toBeNull();
  });
});
```

`FilePane.test.tsx`:

```tsx
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { Entry } from "../../ipc/api";
import { useSftpStore } from "../../stores/sftp";
import { makeSessionInfo } from "../../test/fixtures";
import { FilePane } from "./FilePane";

vi.mock("../../ipc/api");

const f = (name: string, type: "file" | "dir" = "file", extra: Partial<Entry> = {}): Entry => ({
  name, kind: { type }, size: 1, mtime: 0, mode: 0o100644, owner: "u", nameLossy: false, ...extra,
});

const onCommand = vi.fn();

beforeEach(() => {
  onCommand.mockClear();
  vi.mocked(api.localHome).mockResolvedValue("/l");
  vi.mocked(api.localList).mockResolvedValue({ path: "/l", entries: [] });
  vi.mocked(api.sftpList).mockImplementation(async (_s, path) => ({ path, entries: [f("inner")] }));
  useSftpStore.setState(useSftpStore.getInitialState(), true);
  useSftpStore.getState().openTab("t1", "h1", {
    sessionId: "s1", info: makeSessionInfo(), version: 3,
    listing: { path: "/srv/www", entries: [f("b.txt"), f("A-dir", "dir"), f(".env"), f("bad?", "file", { nameLossy: true })] },
  });
});

const pane = () => useSftpStore.getState().tabs[0]!.panes.remote;
const rowNames = () => screen.getAllByRole("row").slice(1).map((r) => within(r).getAllByRole("gridcell")[0]!.textContent);

describe("FilePane", () => {
  it("lists .. and folders first, hides dot files", () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    expect(rowNames()).toEqual(["..", "A-dir", "b.txt", "bad?"]);
  });

  it("opens folders with Enter and goes up with Backspace", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    const grid = screen.getByRole("grid");
    grid.focus();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(api.sftpList).toHaveBeenCalledWith("s1", "/srv/www/A-dir");
    await userEvent.keyboard("{Backspace}");
    expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/srv");
  });

  it("selects with Shift+arrows and Ctrl+A, and sends commands for the targets", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{Shift>}{ArrowDown}{/Shift}");
    expect(pane().selection).toEqual(["A-dir", "b.txt"]);
    await userEvent.keyboard("{Delete}");
    expect(onCommand).toHaveBeenLastCalledWith("delete", ["A-dir", "b.txt"]);
    await userEvent.keyboard("{Control>}a{/Control}");
    expect(pane().selection).toEqual(["A-dir", "b.txt"]);
    await userEvent.keyboard("{F7}");
    expect(onCommand).toHaveBeenLastCalledWith("newFolder", []);
  });

  it("F2 renames exactly one entry and ignores lossy names", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{End}{F2}");
    expect(onCommand).not.toHaveBeenCalled();
    await userEvent.keyboard("{ArrowUp}{F2}");
    expect(onCommand).toHaveBeenLastCalledWith("rename", ["b.txt"]);
  });

  it("toggles hidden files with Ctrl+H and jumps by typing", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{Control>}h{/Control}");
    expect(rowNames()).toContain(".env");
    await userEvent.keyboard("b");
    expect(pane().cursor).toBe("b.txt");
  });

  it("Tab moves focus to the other pane's grid", async () => {
    render(
      <>
        <FilePane tabId="t1" side="local" onCommand={onCommand} />
        <FilePane tabId="t1" side="remote" onCommand={onCommand} />
      </>,
    );
    screen.getByRole("grid", { name: "Remote" }).focus();
    await userEvent.keyboard("{Tab}");
    expect(screen.getByRole("grid", { name: "Lokal" })).toHaveFocus();
    expect(useSftpStore.getState().tabs[0]!.active).toBe("local");
  });

  it("sorts by clicking a column header", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    await userEvent.click(screen.getByRole("columnheader", { name: /Name/ }));
    expect(pane().sort).toEqual({ key: "name", dir: "desc" });
  });

  it("navigates via breadcrumbs and the path field", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    await userEvent.click(screen.getByRole("button", { name: "srv" }));
    expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/srv");
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{Control>}l{/Control}");
    const field = screen.getByRole("textbox", { name: "Pfad" });
    await userEvent.clear(field);
    await userEvent.type(field, "/etc{Enter}");
    expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/etc");
  });

  it("keys inside the path field stay with the field", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    screen.getByRole("grid").focus();
    await userEvent.keyboard("{Control>}l{/Control}");
    const field = screen.getByRole("textbox", { name: "Pfad" });
    vi.mocked(api.sftpList).mockClear();
    await userEvent.type(field, "{Backspace}{F2}{Delete}b");
    expect(onCommand).not.toHaveBeenCalled();
    expect(api.sftpList).not.toHaveBeenCalled();
  });

  it("shows a navigation error above the table", async () => {
    vi.mocked(api.sftpList).mockRejectedValueOnce({ type: "fs", kind: "permissionDenied", path: "/root", message: "denied" });
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    await useSftpStore.getState().navigate("t1", "remote", "/root");
    expect(await screen.findByRole("alert")).toHaveTextContent("Keine Berechtigung");
  });

  it("right-click selects the row and opens a context menu", async () => {
    render(<FilePane tabId="t1" side="remote" onCommand={onCommand} />);
    await userEvent.pointer({ keys: "[MouseRight]", target: screen.getByText("b.txt") });
    expect(pane().selection).toEqual(["b.txt"]);
    await userEvent.click(screen.getByRole("menuitem", { name: "Löschen" }));
    expect(onCommand).toHaveBeenLastCalledWith("delete", ["b.txt"]);
  });
});
```

- [ ] **Step 2: Run to verify they fail**

Run: `bun run test -- sftp/ 2>&1 | tail -5` → FAIL (modules missing).

- [ ] **Step 3: Implement**

`paneKeys.ts`:

```ts
import type { PaneState } from "../../stores/sftp";

export function moveCursor(names: string[], cursor: string | null, key: string, page: number): string | null {
  if (names.length === 0) return null;
  const last = names.length - 1;
  const at = cursor === null ? -1 : names.indexOf(cursor);
  const index = (() => {
    switch (key) {
      case "ArrowDown":
        return Math.min(at + 1, last);
      case "ArrowUp":
        return Math.max(at - 1, 0);
      case "Home":
        return 0;
      case "End":
        return last;
      case "PageDown":
        return Math.min(Math.max(at, 0) + page, last);
      case "PageUp":
        return Math.max(at - page, 0);
      default:
        return at;
    }
  })();
  return names[index] ?? null;
}

export function rangeBetween(names: string[], a: string, b: string): string[] {
  const [i, j] = [names.indexOf(a), names.indexOf(b)].sort((x, y) => x - y) as [number, number];
  return names.slice(i, j + 1).filter((n) => n !== "..");
}

export function typeAhead(names: string[], prefix: string, from: string | null): string | null {
  const p = prefix.toLocaleLowerCase("de");
  const start = from === null ? 0 : names.indexOf(from);
  // A continued prefix may stay on the current entry; a fresh letter moves on.
  const order = [...names.slice(start), ...names.slice(0, start)];
  const firstMatch = (list: string[]) => list.find((n) => n !== ".." && n.toLocaleLowerCase("de").startsWith(p)) ?? null;
  return prefix.length > 1 ? firstMatch(order) : firstMatch([...order.slice(1), ...order.slice(0, 1)]);
}

/** Entries an action applies to: the selection, else the cursor; never `..` or lossy names. */
export function targets(pane: PaneState): string[] {
  const usable = (name: string) => name !== ".." && pane.entries.some((e) => e.name === name && !e.nameLossy);
  const chosen = pane.selection.length > 0 ? pane.selection : pane.cursor ? [pane.cursor] : [];
  return chosen.filter(usable);
}
```

`FilePane.tsx` (CSS module: grid layout with sticky header, `.row[aria-selected=true]` highlight, `.cursor` outline, `.dim` for the disabled pane, `.error` line, `.menu` absolutely positioned; use tokens only, e.g. `var(--amber)`, `var(--surface-2)`, `var(--text-muted)` as in other modules):

```tsx
import { AlertTriangle, File, Folder, Link2 } from "lucide-react";
import { type KeyboardEvent, type MouseEvent, useEffect, useMemo, useRef, useState } from "react";
import { cx } from "../../components";
import { t } from "../../i18n/de";
import type { Entry } from "../../ipc/api";
import { type Side, useSftpStore } from "../../stores/sftp";
import { crumbs, formatMode, formatMtime, formatSize, isDirLike, joinPath, parentPath, type SortKey, visibleEntries } from "./entries";
import styles from "./FilePane.module.css";
import { moveCursor, rangeBetween, targets, typeAhead } from "./paneKeys";

export type PaneCommand = "newFolder" | "rename" | "delete" | "deletePermanent";

const COLUMNS: SortKey[] = ["name", "size", "mtime", "mode", "owner"];
const PAGE = 15;
const TYPE_AHEAD_MS = 1000;

function errorText(error: { type: string; kind?: string; message?: string }): string {
  if (error.type === "fs" && error.kind) return t.sftp.fsErrors[error.kind as keyof typeof t.sftp.fsErrors];
  return error.message ?? t.sftp.failed;
}

function icon(entry: Entry) {
  if (entry.kind.type === "symlink") return entry.kind.target === null ? AlertTriangle : Link2;
  return isDirLike(entry) ? Folder : File;
}

export function FilePane({ tabId, side, disabled = false, onCommand }: {
  tabId: string;
  side: Side;
  disabled?: boolean;
  onCommand: (command: PaneCommand, names: string[]) => void;
}) {
  const tab = useSftpStore((s) => s.tabs.find((x) => x.id === tabId));
  const store = useSftpStore.getState;
  const gridRef = useRef<HTMLDivElement>(null);
  const typed = useRef({ prefix: "", at: 0 });
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [draft, setDraft] = useState("");
  const p = tab?.panes[side];
  const rows = useMemo(() => (p ? visibleEntries(p.entries, p.showHidden, p.sort) : []), [p]);
  const names = useMemo(() => [...(p && p.path !== "/" ? [".."] : []), ...rows.map((e) => e.name)], [p, rows]);

  useEffect(() => {
    if (p?.editingPath) setDraft(p.path);
  }, [p?.editingPath, p?.path]);

  if (!tab || !p) return null;
  const label = side === "local" ? t.sftp.local : t.sftp.remote;

  const open = (name: string) => {
    if (name === "..") return void store().navigate(tabId, side, parentPath(p.path), p.path.split("/").pop());
    const entry = p.entries.find((e) => e.name === name);
    if (entry && isDirLike(entry) && !entry.nameLossy) void store().navigate(tabId, side, joinPath(p.path, name));
  };
  const setCursor = (cursor: string | null, extend: boolean) => {
    if (cursor === null) return;
    if (extend) {
      const anchor = p.anchor ?? p.cursor ?? cursor;
      store().select(tabId, side, { selection: rangeBetween(names, anchor, cursor), cursor, anchor });
    } else {
      store().select(tabId, side, { selection: p.selection, cursor, anchor: cursor });
    }
  };
  const toggle = (name: string) => {
    if (name === "..") return;
    const selection = p.selection.includes(name) ? p.selection.filter((n) => n !== name) : [...p.selection, name];
    store().select(tabId, side, { selection, cursor: name, anchor: name });
  };
  const command = (c: PaneCommand) => {
    const chosen = targets(p);
    if (c === "newFolder") return onCommand(c, []);
    if (c === "rename" && chosen.length !== 1) return;
    if (chosen.length > 0) onCommand(c, chosen);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (disabled || event.target !== event.currentTarget) return;
    const { key, ctrlKey, shiftKey, altKey } = event;
    const handled = () => event.preventDefault();
    if (["ArrowDown", "ArrowUp", "Home", "End", "PageDown", "PageUp"].includes(key) && !ctrlKey) {
      handled();
      return setCursor(moveCursor(names, p.cursor, key, PAGE), shiftKey);
    }
    if (ctrlKey && !altKey) {
      const k = key.toLowerCase();
      if (k === "a") return handled(), store().select(tabId, side, { selection: names.filter((n) => n !== ".."), cursor: p.cursor, anchor: p.anchor });
      if (k === "h") return handled(), store().toggleHidden(tabId, side);
      if (k === "r") return handled(), void store().reload(tabId, side);
      if (k === "l") return handled(), store().setEditingPath(tabId, side, true);
      return;
    }
    switch (key) {
      case "Tab": {
        handled();
        const other: Side = side === "local" ? "remote" : "local";
        store().setActivePane(tabId, other);
        return document.querySelector<HTMLElement>(`[data-sftp-grid="${other}"]`)?.focus();
      }
      case "Enter":
        return handled(), p.cursor && open(p.cursor);
      case "Backspace":
        return handled(), open("..");
      case " ":
        return handled(), p.cursor && toggle(p.cursor);
      case "F2":
        return handled(), command("rename");
      case "F7":
        return handled(), command("newFolder");
      case "F8":
      case "Delete":
        return handled(), command(shiftKey && side === "local" ? "deletePermanent" : "delete");
    }
    if (key.length === 1 && !altKey && key !== " ") {
      const now = Date.now();
      typed.current = { prefix: now - typed.current.at < TYPE_AHEAD_MS ? typed.current.prefix + key : key, at: now };
      const hit = typeAhead(names, typed.current.prefix, p.cursor);
      if (hit) setCursor(hit, false);
    }
  };

  const onRowClick = (event: MouseEvent, name: string) => {
    gridRef.current?.focus();
    store().setActivePane(tabId, side);
    if (event.ctrlKey) return toggle(name);
    if (event.shiftKey && p.anchor) return setCursor(name, true);
    store().select(tabId, side, { selection: name === ".." ? [] : [name], cursor: name, anchor: name });
  };
  const onContextMenu = (event: MouseEvent, name: string | null) => {
    event.preventDefault();
    if (name && name !== ".." && !p.selection.includes(name)) store().select(tabId, side, { selection: [name], cursor: name, anchor: name });
    setMenu({ x: event.clientX, y: event.clientY });
  };
  const menuItem = (text: string, run: () => void) => (
    <button type="button" role="menuitem" className={styles.menuItem} onClick={() => { setMenu(null); run(); }}>
      {text}
    </button>
  );

  return (
    <section className={cx(styles.pane, disabled && styles.dim, tab.active === side && styles.active)} aria-label={label}>
      <header className={styles.head}>
        <span className={styles.side}>{label}</span>
        {p.editingPath ? (
          <input
            aria-label={t.sftp.path}
            className={styles.pathField}
            value={draft}
            autoFocus
            spellCheck={false}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter" && !e.nativeEvent.isComposing) void store().navigate(tabId, side, draft.trim() || "/");
              if (e.key === "Escape") store().setEditingPath(tabId, side, false);
            }}
            onBlur={() => store().setEditingPath(tabId, side, false)}
          />
        ) : (
          <nav className={styles.crumbs} aria-label={t.sftp.path} onClick={(e) => e.target === e.currentTarget && store().setEditingPath(tabId, side, true)}>
            {crumbs(p.path).map((c) => (
              <button key={c.path} type="button" className={styles.crumb} onClick={() => void store().navigate(tabId, side, c.path)}>
                {c.label}
              </button>
            ))}
          </nav>
        )}
      </header>
      {p.error ? (
        <details role="alert" className={styles.error}>
          <summary>{errorText(p.error)}</summary>
          <code>{"message" in p.error ? p.error.message : p.error.type}</code>
        </details>
      ) : null}
      <div
        ref={gridRef}
        role="grid"
        data-sftp-grid={side}
        aria-label={label}
        aria-multiselectable
        aria-busy={p.loading}
        tabIndex={0}
        className={styles.grid}
        onKeyDown={onKeyDown}
        onFocus={() => store().setActivePane(tabId, side)}
        onContextMenu={(e) => onContextMenu(e, null)}
      >
        <div role="row" className={styles.header}>
          {COLUMNS.map((key) => (
            <div
              key={key}
              role="columnheader"
              aria-sort={p.sort.key === key ? (p.sort.dir === "asc" ? "ascending" : "descending") : "none"}
              className={styles.colHead}
              onClick={() => store().setSort(tabId, side, key)}
            >
              {t.sftp.columns[key]}
            </div>
          ))}
        </div>
        {names.map((name) => {
          const entry = rows.find((e) => e.name === name);
          const Icon = entry ? icon(entry) : Folder;
          return (
            <div
              key={name}
              role="row"
              aria-selected={p.selection.includes(name)}
              title={entry?.nameLossy ? t.sftp.lossyName : entry?.kind.type === "symlink" && entry.kind.target === null ? t.sftp.brokenLink : undefined}
              className={cx(styles.row, p.cursor === name && styles.cursor, entry?.nameLossy && styles.lossy)}
              onClick={(e) => onRowClick(e, name)}
              onDoubleClick={() => open(name)}
              onContextMenu={(e) => { e.stopPropagation(); onContextMenu(e, name); }}
            >
              <div role="gridcell" className={styles.name}>
                <Icon size={14} aria-hidden /> <span>{name}</span>
              </div>
              <div role="gridcell" className="mono">{entry && !isDirLike(entry) ? formatSize(entry.size) : ""}</div>
              <div role="gridcell" className="mono">{entry ? formatMtime(entry.mtime) : ""}</div>
              <div role="gridcell" className="mono">{entry ? formatMode(entry.mode) : ""}</div>
              <div role="gridcell">{entry?.owner ?? ""}</div>
            </div>
          );
        })}
      </div>
      {menu ? (
        <div role="menu" className={styles.menu} style={{ left: menu.x, top: menu.y }} onMouseLeave={() => setMenu(null)}>
          {menuItem(t.sftp.open, () => p.cursor && open(p.cursor))}
          {menuItem(t.sftp.rename, () => command("rename"))}
          {menuItem(t.sftp.delete, () => command("delete"))}
          {menuItem(t.sftp.newFolder, () => command("newFolder"))}
          {menuItem(t.sftp.reload, () => void store().reload(tabId, side))}
        </div>
      ) : null}
    </section>
  );
}
```

Keep it lint-clean: if the repo's lint config rejects `return handled(), …` comma expressions, write them as two statements. If `aria-multiselectable` needs `={true}` per lint, use that. The menu closes on `Escape` too: add `onKeyDown={(e) => e.key === "Escape" && setMenu(null)}` to the menu container.

- [ ] **Step 4: Run tests**

Run: `bun run test -- sftp/ 2>&1 | tail -5` → PASS, no act() warnings (await every store-triggering interaction; use `vi.waitFor` where a navigate resolves asynchronously).

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/sftp
git commit -m "feat(ui): commander file pane with breadcrumbs, sorting and keyboard control

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: `SftpScreen`, dialogs, host picker, wiring

**Files:**
- Create: `ui/src/screens/sftp/SftpScreen.tsx`, `ui/src/screens/sftp/SftpScreen.module.css`, `ui/src/screens/sftp/SftpScreen.test.tsx`, `ui/src/screens/sftp/NameDialog.tsx`, `ui/src/screens/sftp/DeleteDialog.tsx`, `ui/src/screens/sftp/dialogs.test.tsx`
- Modify: `ui/src/shell/AppShell.tsx`, `ui/src/screens/hosts/HostDetail.tsx`, `ui/src/screens/hosts/HostCard.tsx`, `ui/src/screens/hosts/useHostActions.ts`, `ui/src/i18n/de.ts` (`stubs` sftp entries removed, `editor.sftpHint`, `hosts.filesLater` removed if unused), host screen tests that asserted the disabled "Dateien" button
- Delete: `ui/src/screens/sftp/SftpPlaceholder.tsx`

**Interfaces:**
- Consumes: Tasks 6–8.
- Produces: `SftpScreen` (default view content), `NameDialog`, `DeleteDialog`, `HostActions.openFiles(host: Host): void`, `validateName(name: string): string | null` (in `NameDialog.tsx`, exported).

- [ ] **Step 1: Write the failing tests**

`dialogs.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DeleteDialog } from "./DeleteDialog";
import { NameDialog, validateName } from "./NameDialog";

describe("NameDialog", () => {
  it("validates like the core", () => {
    expect(validateName("")).toBe("Bitte einen Namen eingeben.");
    expect(validateName("a/b")).toBe("Der Name darf kein / enthalten.");
    expect(validateName("..")).toBe("„.“ und „..“ sind keine gültigen Namen.");
    expect(validateName("ok")).toBeNull();
  });

  it("shows AlreadyExists from the core at the field", async () => {
    const submit = vi.fn(async () => {
      throw { type: "fs", kind: "alreadyExists", path: "/x/b", message: "exists" };
    });
    render(<NameDialog mode="rename" initial="a.txt" onSubmit={submit} onClose={() => {}} />);
    const field = screen.getByRole("textbox", { name: "Name" });
    await userEvent.clear(field);
    await userEvent.type(field, "b{Enter}");
    expect(await screen.findByText("Dieser Name existiert bereits.")).toBeInTheDocument();
  });

  it("preselects the name without extension when renaming", () => {
    render(<NameDialog mode="rename" initial="report.final.pdf" onSubmit={async () => {}} onClose={() => {}} />);
    const field = screen.getByRole("textbox", { name: "Name" }) as HTMLInputElement;
    expect([field.selectionStart, field.selectionEnd]).toEqual([0, "report.final".length]);
  });
});

describe("DeleteDialog", () => {
  it("is a red permanent confirmation with focus on cancel", () => {
    render(<DeleteDialog mode="permanent" names={["dir"]} folder onConfirm={() => {}} onClose={() => {}} />);
    expect(screen.getByRole("alertdialog", { name: "Endgültig löschen?" })).toBeInTheDocument();
    expect(screen.getByText("Der Ordner wird mitsamt Inhalt entfernt.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Abbrechen" })).toHaveFocus();
  });

  it("offers the trash for local deletes", async () => {
    const confirm = vi.fn();
    render(<DeleteDialog mode="trash" names={["a", "b"]} folder={false} onConfirm={confirm} onClose={() => {}} />);
    expect(screen.getByText("2 Einträge werden in den Papierkorb verschoben.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "In den Papierkorb" }));
    expect(confirm).toHaveBeenCalled();
  });
});
```

`SftpScreen.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { useSftpStore } from "../../stores/sftp";
import { makeHost, makeSessionInfo } from "../../test/fixtures";
import { SftpScreen } from "./SftpScreen";

vi.mock("../../ipc/api");

const start = vi.fn(async () => undefined);

beforeEach(() => {
  start.mockClear();
  vi.mocked(api.localHome).mockResolvedValue("/l");
  vi.mocked(api.localList).mockResolvedValue({ path: "/l", entries: [] });
  vi.mocked(api.sftpClose).mockResolvedValue();
  useConnectStore.setState({ ...useConnectStore.getInitialState(), start }, true);
  useSftpStore.setState(useSftpStore.getInitialState(), true);
  useHostsStore.setState({
    ...useHostsStore.getInitialState(),
    hosts: [
      makeHost({ id: "h1", name: "web", user: "deploy", address: "web.example" }),
      makeHost({ id: "h2", name: "ssh-only", protocolSftp: false }),
    ],
  }, true);
});

const openTab = () =>
  useSftpStore.getState().openTab("t1", "h1", {
    sessionId: "s1", info: makeSessionInfo({ cipher: "chacha20-poly1305@openssh.com" }), version: 3,
    listing: { path: "/srv", entries: [] },
  });

describe("SftpScreen", () => {
  it("offers only SFTP hosts when no session is open", async () => {
    render(<SftpScreen />);
    expect(screen.queryByText("ssh-only")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /web/ }));
    expect(start).toHaveBeenCalledWith("h1", "sftp");
  });

  it("shows both panes and the status line", () => {
    openTab();
    render(<SftpScreen />);
    expect(screen.getByRole("grid", { name: "Lokal" })).toBeInTheDocument();
    expect(screen.getByRole("grid", { name: "Remote" })).toBeInTheDocument();
    expect(screen.getByText("deploy@web.example · SFTP v3 · chacha20-poly1305@openssh.com")).toBeInTheDocument();
  });

  it("shows the lost banner and reconnects", async () => {
    openTab();
    useSftpStore.getState().markLost("t1", "s1", { type: "network", kind: "other", message: "connection lost" });
    render(<SftpScreen />);
    expect(screen.getByText("Verbindung verloren")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Neu verbinden" }));
    expect(start).toHaveBeenCalledWith("h1", "sftp", { sftpTabId: "t1" });
  });

  it("opens the new-folder dialog from the toolbar and creates the folder", async () => {
    openTab();
    vi.mocked(api.sftpMkdir).mockResolvedValue();
    vi.mocked(api.sftpList).mockResolvedValue({ path: "/srv", entries: [] });
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Neuer Ordner" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Name" }), "neu{Enter}");
    expect(api.sftpMkdir).toHaveBeenCalledWith("s1", "/srv", "neu");
  });

  it("opens a terminal to the same host", async () => {
    openTab();
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
    expect(start).toHaveBeenCalledWith("h1", "ssh");
  });

  it("closes a tab", async () => {
    openTab();
    render(<SftpScreen />);
    await userEvent.click(screen.getByRole("button", { name: "web schließen" }));
    expect(api.sftpClose).toHaveBeenCalledWith("s1");
  });
});
```

Host screens: change the existing tests that expect the "Dateien" button to be `aria-disabled` into: "Dateien" is enabled for an SFTP host and calls `start(host.id, "sftp")`; disabled (`disabled` attribute) for a host with `protocolSftp: false`.

- [ ] **Step 2: Run to verify they fail**

Run: `bun run test -- sftp/ hosts/ 2>&1 | tail -8` → FAIL.

- [ ] **Step 3: Implement**

`NameDialog.tsx`:

```tsx
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { Button, Dialog, TextField } from "../../components";
import { t } from "../../i18n/de";
import { isCoreError } from "../../ipc/errors";

/** Same rules as core `validate_file_name` (minus U+FFFD, which a user cannot type by accident). */
export function validateName(name: string): string | null {
  const texts = t.sftp.nameErrors;
  if (name.trim() === "") return texts.empty;
  if (name.includes("/")) return texts.slash;
  if (name === "." || name === "..") return texts.dots;
  return null;
}

export function NameDialog({ mode, initial = "", onSubmit, onClose }: {
  mode: "new" | "rename";
  initial?: string;
  onSubmit: (name: string) => Promise<void>;
  onClose: () => void;
}) {
  const texts = t.sftp.nameDialog;
  const ref = useRef<HTMLInputElement>(null);
  const [value, setValue] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const dot = initial.lastIndexOf(".");
    ref.current?.setSelectionRange(0, dot > 0 ? dot : initial.length);
  }, [initial]);

  const submit = async () => {
    const invalid = validateName(value);
    if (invalid) return setError(invalid);
    if (mode === "rename" && value === initial) return onClose();
    setBusy(true);
    try {
      await onSubmit(value);
      onClose();
    } catch (e) {
      setError(isCoreError(e) && e.type === "fs" && e.kind === "alreadyExists" ? t.sftp.nameErrors.exists : isCoreError(e) && "message" in e ? e.message : t.sftp.failed);
    } finally {
      setBusy(false);
    }
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
    event.preventDefault();
    void submit();
  };

  return (
    <Dialog
      open
      size="sm"
      title={mode === "new" ? texts.newTitle : texts.renameTitle}
      onClose={onClose}
      initialFocusRef={ref}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>{t.common.cancel}</Button>
          <Button variant="primary" disabled={busy} onClick={() => void submit()}>
            {mode === "new" ? texts.create : texts.save}
          </Button>
        </>
      }
    >
      <div onKeyDown={onKeyDown}>
        <TextField ref={ref} label={texts.label} mono spellCheck={false} value={value} error={error ?? undefined}
          onChange={(v) => { setValue(v); setError(null); }} />
      </div>
    </Dialog>
  );
}
```

`DeleteDialog.tsx`:

```tsx
import { useRef } from "react";
import { Button, Dialog } from "../../components";
import { t } from "../../i18n/de";

export function DeleteDialog({ mode, names, folder, onConfirm, onClose }: {
  mode: "trash" | "permanent";
  names: string[];
  folder: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const texts = t.sftp.deleteDialog;
  const cancelRef = useRef<HTMLButtonElement>(null);
  const trash = mode === "trash";
  const one = names.length === 1 ? names[0]! : null;
  const text = trash
    ? one ? texts.trashOne(one) : texts.trashMany(names.length)
    : one ? texts.one(one) : texts.many(names.length);
  return (
    <Dialog
      open
      size="sm"
      role="alertdialog"
      title={trash ? texts.trashTitle : texts.permanentTitle}
      onClose={onClose}
      initialFocusRef={cancelRef}
      footer={
        <>
          <Button ref={cancelRef} variant="ghost" onClick={onClose}>{t.common.cancel}</Button>
          <Button variant={trash ? "primary" : "danger"} onClick={() => { onConfirm(); onClose(); }}>
            {trash ? texts.confirmTrash : texts.confirm}
          </Button>
        </>
      }
    >
      <p>{text}</p>
      {folder ? <p>{texts.folderHint}</p> : null}
    </Dialog>
  );
}
```

(Check `Button` forwards refs and has a `"danger"` variant; if the destructive variant has another name, use it; if `Button` has no ref forwarding, put the ref on a wrapper via `initialFocusRef` to the first footer button as `PlaceholderDialog`-style code does elsewhere, and ledger the ruling.)

`SftpScreen.tsx`:

```tsx
import { FolderPlus, Pencil, RefreshCw, SquareTerminal, Trash2, Eye, X } from "lucide-react";
import { useState } from "react";
import { Button, cx, IconButton } from "../../components";
import { t } from "../../i18n/de";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { type Side, useSftpStore } from "../../stores/sftp";
import { hostProfile } from "../../model/hostFormat";
import { DeleteDialog } from "./DeleteDialog";
import { isDirLike } from "./entries";
import { FilePane, type PaneCommand } from "./FilePane";
import { NameDialog } from "./NameDialog";
import { targets } from "./paneKeys";
import styles from "./SftpScreen.module.css";

type DialogState =
  | { type: "new"; side: Side }
  | { type: "rename"; side: Side; name: string }
  | { type: "delete"; side: Side; names: string[]; mode: "trash" | "permanent"; folder: boolean };

function HostPicker() {
  const hosts = useHostsStore((s) => s.hosts).filter((h) => h.protocolSftp);
  const start = useConnectStore((s) => s.start);
  return (
    <div className={styles.picker}>
      <h2>{t.sftp.pickHost}</h2>
      {hosts.length === 0 ? <p>{t.sftp.noSftpHosts}</p> : null}
      <ul>
        {hosts.map((h) => (
          <li key={h.id}>
            <Button onClick={() => void start(h.id, "sftp")}>{h.name}</Button>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function SftpScreen() {
  const tabs = useSftpStore((s) => s.tabs);
  const activeTabId = useSftpStore((s) => s.activeTabId);
  const hosts = useHostsStore((s) => s.hosts);
  const profiles = useHostsStore((s) => s.profiles);
  const start = useConnectStore((s) => s.start);
  const [picking, setPicking] = useState(false);
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const tab = tabs.find((x) => x.id === activeTabId) ?? null;
  const store = useSftpStore.getState;

  if (!tab || picking) return <HostPicker />;
  const host = hosts.find((h) => h.id === tab.hostId);
  const user = host?.user?.trim() || (host ? hostProfile(host, profiles)?.user : "") || "";
  const lost = tab.status === "lost";
  const side = tab.active;
  const pane = tab.panes[side];
  const sideDisabled = (s: Side) => s === "remote" && lost;

  const onCommand = (s: Side) => (command: PaneCommand, names: string[]) => {
    if (sideDisabled(s)) return;
    if (command === "newFolder") return setDialog({ type: "new", side: s });
    if (command === "rename") return setDialog({ type: "rename", side: s, name: names[0]! });
    const entries = tab.panes[s].entries.filter((e) => names.includes(e.name));
    const folder = entries.some((e) => isDirLike(e) && e.kind.type !== "symlink");
    const mode = s === "local" && command === "delete" ? "trash" : "permanent";
    setDialog({ type: "delete", side: s, names, mode, folder });
  };
  const toolbar = (command: PaneCommand) => onCommand(side)(command, targets(pane));

  return (
    <div className={styles.screen}>
      <div role="tablist" aria-label={t.sftp.tabs} className={styles.tabs}>
        {tabs.map((x) => (
          <div key={x.id} className={cx(styles.tab, x.id === tab.id && styles.activeTab)}>
            <button type="button" role="tab" aria-selected={x.id === tab.id} onClick={() => store().setActiveTab(x.id)}>
              {x.title}
            </button>
            <IconButton size="sm" icon={X} label={t.sftp.closeTab(x.title)} onClick={() => store().closeTab(x.id)} />
          </div>
        ))}
        <IconButton size="sm" icon={FolderPlus} label={t.sftp.newTab} onClick={() => setPicking(true)} />
      </div>
      <div role="toolbar" aria-label={t.sftp.toolbar} className={styles.toolbar}>
        <Button icon={FolderPlus} disabled={sideDisabled(side)} onClick={() => toolbar("newFolder")}>{t.sftp.newFolder}</Button>
        <Button icon={Pencil} disabled={sideDisabled(side)} onClick={() => toolbar("rename")}>{t.sftp.rename}</Button>
        <Button icon={Trash2} disabled={sideDisabled(side)} onClick={() => toolbar("delete")}>{t.sftp.delete}</Button>
        <Button icon={RefreshCw} disabled={sideDisabled(side)} onClick={() => void store().reload(tab.id, side)}>{t.sftp.reload}</Button>
        <Button icon={Eye} aria-pressed={pane.showHidden} onClick={() => store().toggleHidden(tab.id, side)}>{t.sftp.hidden}</Button>
        <Button icon={SquareTerminal} onClick={() => void start(tab.hostId, "ssh")}>{t.sftp.terminal}</Button>
      </div>
      <div className={styles.panes}>
        <FilePane tabId={tab.id} side="local" onCommand={onCommand("local")} />
        <div className={styles.remote}>
          {lost ? (
            <div role="status" className={styles.lost}>
              <span>{t.sftp.lost}</span>
              <Button variant="primary" onClick={() => store().reconnect(tab.id)}>{t.sftp.reconnect}</Button>
            </div>
          ) : null}
          <FilePane tabId={tab.id} side="remote" disabled={lost} onCommand={onCommand("remote")} />
        </div>
      </div>
      <footer className={styles.status}>
        {t.sftp.status(`${user}@${host?.address ?? ""}`, tab.version, tab.info.cipher)}
      </footer>
      {dialog?.type === "new" ? (
        <NameDialog mode="new" onSubmit={(name) => store().mkdir(tab.id, dialog.side, name)} onClose={() => setDialog(null)} />
      ) : null}
      {dialog?.type === "rename" ? (
        <NameDialog mode="rename" initial={dialog.name} onSubmit={(name) => store().rename(tab.id, dialog.side, dialog.name, name)} onClose={() => setDialog(null)} />
      ) : null}
      {dialog?.type === "delete" ? (
        <DeleteDialog mode={dialog.mode} names={dialog.names} folder={dialog.folder}
          onConfirm={() => void store().remove(tab.id, dialog.side, dialog.names, dialog.mode)} onClose={() => setDialog(null)} />
      ) : null}
    </div>
  );
}
```

`HostPicker` closes itself after `start`: call `setPicking(false)` in SftpScreen when `tabs.length` grows — simplest: pass `onPicked={() => setPicking(false)}` to `HostPicker` and call it after `start(...)`. Make the picker's buttons keyboard reachable (they are buttons). Styles: CSS module with tokens; panes side by side (`display: grid; grid-template-columns: 1fr 1fr`), amber accent for the active pane border (SFTP colour semantics).

Wiring:
- `AppShell.tsx`: replace `SftpPlaceholder` with `SftpScreen`; delete `SftpPlaceholder.tsx`; remove `t.stubs.sftp`/`sftpText` (and the whole `stubs` block if empty) — keep `ScreenPlaceholder` only if something else still uses it, else delete it too.
- `useHostActions.ts`: `openFiles: (host) => { void useConnectStore.getState().start(host.id, "sftp"); }` + interface entry.
- `HostDetail.tsx`: the files button becomes `<Button size="lg" icon={FolderOpen} disabled={!host.protocolSftp} onClick={() => actions.openFiles(host)}>`; drop the `title`/`later` class.
- `HostCard.tsx`: the files `IconButton` gets `onClick={() => actions.openFiles(host)}`, no `aria-disabled`/`title`/`later`.
- `de.ts`: remove `hosts.filesLater` if no longer referenced; `editor.sftpHint: "Startverzeichnisse wirken ab sofort. Modus, Parallelität, Rechte und Bandbreite wirken ab Teilprojekt 3b bzw. 4."`.
- Connect screen amber accent: in `ConnectProgress.tsx` pass `tone={flow.protocol === "sftp" ? "amber" : "accent"}` to `ProgressBar` (it accepts `tone`).

- [ ] **Step 4: Run tests**

Run: `bun run test 2>&1 | tail -5` → all PASS, no warnings. `bun run typecheck` clean.

- [ ] **Step 5: Commit**

```bash
git add -A ui/src
git commit -m "feat(ui): SFTP screen with tabs, toolbar, dialogs and lost banner; enable Dateien

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Acceptance checklist, docs, full verification

**Files:**
- Create: `docs/acceptance-tp3a.md`
- Modify: `CLAUDE.md` (acceptance list mentions `acceptance-tp3a.md`; roadmap line "TP3: SFTP" → "TP3: SFTP (3a browse done; 3b transfers; 3c bookmarks + edit)")

- [ ] **Step 1: Write `docs/acceptance-tp3a.md`** (German, same style as `docs/acceptance-tp2d.md`), sections with checkboxes:

```markdown
# Abnahme TP3a: SFTP – Verbinden, Navigieren, Dateioperationen

## Verbinden
- [ ] „Dateien“ in Host-Karte und Detail-Panel öffnet den SFTP-Ladebildschirm (amber), Schritte bis „Verzeichnis laden“.
- [ ] Host ohne SFTP: „Dateien“ ist deaktiviert.
- [ ] Unbekannter Host-Key: Dialog wie bei SSH; Ablehnen bricht ab.
- [ ] `remoteStart` gesetzt → Remote-Panel startet dort; nicht lesbar → Fehlerseite bei „Verzeichnis laden“.
- [ ] `localStart` gesetzt → lokales Panel startet dort, sonst im Home.
- [ ] Jump-Host-Host funktioniert.
- [ ] SFTP-View ohne Sitzung zeigt die Host-Auswahl (nur SFTP-Hosts).

## Navigieren
- [ ] Doppelklick/Enter öffnet Ordner und Links auf Ordner; „..“ und Backspace gehen hoch.
- [ ] Breadcrumbs klickbar; Klick auf freie Fläche oder Strg+L → Pfad-Feld; Enter navigiert, Esc bricht ab.
- [ ] Ordner ohne Berechtigung: Fehlerzeile, alter Pfad bleibt.
- [ ] Sortierung per Spaltenkopf, Ordner immer oben, `file2` vor `file10`.
- [ ] Strg+H / „Versteckte“ blendet Punktdateien ein und aus.
- [ ] Größe, Datum (TT.MM.JJJJ hh:mm), Rechte (`rwxr-xr-x`), Besitzer stimmen mit `ls -l` überein.
- [ ] Kaputter Symlink hat Warnsymbol.

## Tastatur
- [ ] Tab wechselt das Panel; ↑/↓, Pos1/Ende, Bild↑/↓; Shift erweitert; Leertaste/Strg+Klick toggelt; Strg+A.
- [ ] Tippen springt zum Namen; F2, F7, Entf/F8, Shift+Entf (lokal), Strg+R.
- [ ] Tasten im Pfad-Feld und in Dialogen lösen keine Panel-Aktionen aus.
- [ ] Rechtsklick-Menü: Öffnen, Umbenennen, Löschen, Neuer Ordner, Neu laden.

## Dateioperationen
- [ ] Neuer Ordner lokal und remote; existierender Name → Fehler am Feld.
- [ ] Umbenennen lokal und remote; Ziel existiert → Fehler am Feld, nichts überschrieben.
- [ ] Remote löschen (Datei, Ordner mit Inhalt, Symlink auf Ordner: Ziel bleibt erhalten) mit rotem Dialog.
- [ ] Lokal Entf → Papierkorb (in Nautilus sichtbar und wiederherstellbar); Shift+Entf → endgültig.
- [ ] Lokal auf USB-Stick Entf → verständliche Meldung, Shift+Entf funktioniert.

## Verbindungsverlust
- [ ] WLAN aus / sshd neu starten → Banner „Verbindung verloren“, Remote-Panel ausgegraut, lokal bedienbar.
- [ ] „Neu verbinden“ → derselbe Tab, letzter Remote-Pfad wieder geladen.
- [ ] Tab schließen trennt die Verbindung (auf dem Server keine sftp-server-Prozesse mehr).
- [ ] „Terminal“ in der Toolbar öffnet eine SSH-Sitzung zum selben Host.
```

- [ ] **Step 2: Full verification**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bun run typecheck && bun run test`
Expected: all green, no warnings.
Run: `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`
Expected: 22 passed.
Run: `cargo test -p linascp exports_bindings && git status --short ui/src/ipc/bindings.ts`
Expected: no diff (bindings up to date).

- [ ] **Step 3: Commit**

```bash
git add docs/acceptance-tp3a.md CLAUDE.md
git commit -m "docs: TP3a acceptance checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
