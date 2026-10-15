# LinaSCP TP3b SFTP Transfers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upload/download between the commander panes (F5, context menu, toolbar, drag & drop, Nautilus drop) through a per-tab queue with conflict dialog, upload permissions, `.filepart` safety and automatic SFTP reconnect that resumes interrupted files.

**Architecture:** A transfer engine (`TransferQueue`) in `crates/core/src/transfer/` works on two `Side` trait objects (local file system, `SftpSession`). A generic `Reconnecting<T>` in `crates/core/src/connect/sftp_reconnect.rs` keeps the SFTP connection alive like TP2d and tells the queue when the remote side comes and goes. Tauri glues both into one `SftpEntry` per session and forwards everything over the existing per-tab `SftpEvent` channel; the UI only renders (`stores/transfers.ts`, `TransferPanel`, `ConflictDialog`).

**Tech Stack:** Rust (tokio, russh-sftp 3.0.1 `RawSftpSession`, futures `buffered`, async-trait), Tauri 2 + tauri-specta, React 19 + zustand + Vitest.

**Spec:** `docs/superpowers/specs/2026-10-15-linascp-tp3b-transfers-design.md`

## Global Constraints

- Chat German; code, comments, identifiers, commits English. UI strings German, only in `ui/src/i18n/de.ts` (new keys under `t.sftp.transfers` and `t.sftp.conflict`).
- TDD: every behaviour starts with a failing test. Test output free of warnings (incl. act()/jsdom noise).
- Work on branch `feat/tp3b-transfers`. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Toolchain: `export PATH=$HOME/.cargo/bin:$PATH` first. Bindings: `cargo test -p linascp exports_bindings` (never edit `ui/src/ipc/bindings.ts` by hand).
- Full check: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bun run typecheck && bun run test`.
- Docker suite: `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`.
- Secrets only in the keyring; reconnect attempts never prompt (`inputRequired`); host keys fail closed.
- Under the target name there is never a half-written file; an existing file stays untouched until the transfer succeeded.
- Recursion never follows directory symlinks. No lossy file name ever reaches the file system.
- Download permissions are never wider than the remote ones, except the minimum `u+rw` (dirs `u+rwx`).
- Always binary. No text mode, no bandwidth limit, no `sftpServerCmd` (TP4).

## Review Focus

1. A file that shrinks or vanishes while it is copied: the job must end `failed` (vanished) or `done` with the bytes that were there, never hang. → Task 2 test `copy_stops_at_a_short_read`.
2. Cancelling a job while it waits for a conflict decision: the next conflict of the session must open, not stay blocked. → Task 4 test `cancelling_the_open_conflict_opens_the_next`.
3. A source path that is `/`, relative, or contains `..` from a crafted IPC call: enqueue must refuse with `Config`, nothing touched. → Task 3 test `enqueue_rejects_bad_paths`.
4. The connection drops while a directory job is expanding: no duplicate child jobs after the resume. → Task 5 test `a_dir_job_paused_mid_listing_adds_its_children_once`.
5. Closing a tab during a running upload: no `.filepart` left behind on either side, no event after close. → Task 4 test `close_cancels_and_removes_parts_without_events`.

---

## File Structure

Core (`crates/core/src/`):
- `transfer/mod.rs` – public types (`Direction`, `Job`, `JobState`, `JobKind`, `JobNote`, `FileStamp`, `Decision`, `CancelTarget`, `TransferEvent`, `TransferOptions`, ids) and re-exports.
- `transfer/side.rs` – `Side`, `Reader`, `Writer` traits, `Node`, `NodeKind`, `Child`.
- `transfer/local_side.rs` – `LocalSide` (std fs via `spawn_blocking`).
- `transfer/copy.rs` – pipelined `copy_range` with contiguous watermark.
- `transfer/queue.rs` – `TransferQueue` engine.
- `transfer/test_support.rs` – `#[cfg(test)]` `FaultySide` + event recorder.
- `connect/sftp_session.rs` – `raw` becomes `Arc<RawSftpSession>`, `posix_rename` flag, `impl Side for SftpSession`, `impl Connection for SftpSession`.
- `connect/sftp_reconnect.rs` – `Connection` trait, `Reconnecting<T>`.
- `connect/mod.rs` – passes the `posix-rename@openssh.com` flag, exports.
- `tests/ssh_integration.rs` – Docker tests for transfers and reconnect.

Tauri (`src-tauri/src/`):
- `commands/sftp.rs` – `SftpEntry`, new `SftpEvent`, connect with optional reattach, panel ops via `current()`.
- `commands/sftp_transfers.rs` – transfer and reconnect commands (the name `transfer.rs` is taken by export/import).
- `commands/connect.rs` – `find_host_in`, `build_request_in`, `connect_deps_in` become `pub(crate)`.
- `state.rs` – `sftp` map holds `Arc<SftpEntry>`.
- `dto.rs`, `commands/settings.rs` – `terminal_auto_reconnect` → `auto_reconnect` (DB key unchanged).
- `lib.rs` – command registration, bindings assertions.

UI (`ui/src/`):
- `ipc/api.ts` – wrappers.
- `stores/transfers.ts` (+ test) – jobs per tab, conflicts, speed.
- `stores/sftp.ts` (+ test) – `reconnecting` status, `onEvent`, reconnect via reattach.
- `stores/connect.ts` (+ test) – `runSftp` routes all `SftpEvent`s, passes the existing session id.
- `screens/sftp/transferTargets.ts` (+ test) – F5/drop → enqueue arguments.
- `screens/sftp/FilePane.tsx` – F5, context menu entry, pointer drag source/target, drop highlight.
- `screens/sftp/useExternalDrop.ts` – Tauri `onDragDropEvent` → remote pane.
- `screens/sftp/TransferPanel.tsx` (+ css, test), `ConflictDialog.tsx` (+ test), `CloseTabDialog.tsx`.
- `screens/sftp/SftpScreen.tsx` – toolbar button, panel, dialogs, reconnect bar, tab dot, close confirm.
- `screens/terminal/ReconnectBar.tsx` – takes plain props so SFTP can reuse it.
- `screens/settings/SettingsDialog.tsx`, `stores/settings.ts` – `autoReconnect`.
- `i18n/de.ts` – strings.

Docs: `docs/acceptance-tp3b.md`, `CLAUDE.md` (spec/acceptance references, roadmap).

## Rulings already taken against the spec (the executor keeps them)

- **Chunking:** 64 KiB blocks, 32 in flight per file (spec: 256 KiB × 8). OpenSSH's sftp-server caps a read at 261 120 bytes, so 256 KiB reads come back short; the bytes in flight stay the same (2 MiB).
- **In-app drag & drop uses pointer events, not HTML5 DnD.** The spec's §6 spike needs a person at the GUI in the middle of the run; pointer events work regardless of how WebKitGTK and Tauri's file-drop handler interact, and they are testable in jsdom. The spec names this as the fallback.
- **`SftpEvent::Transfer { event }`** wraps the core `TransferEvent` instead of four flat variants: one core type, one specta type, same information.
- **"Neu verbinden" after `lost`** runs the normal interactive connect flow (prompts allowed) and **re-attaches** the new connection to the existing session (`connect_sftp(…, sessionId)`), so the queue and the paused jobs survive even when a password has to be typed. The ReconnectBar's "Jetzt versuchen" calls `sftp_reconnect_now`.
- **Ids** (`JobId`, `BatchId`) are `u64` counters (bindings cast to `number`).

---

### Task 1: Transfer types, `Side` trait and `LocalSide`

**Files:**
- Create: `crates/core/src/transfer/mod.rs`, `crates/core/src/transfer/side.rs`, `crates/core/src/transfer/local_side.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod transfer;`)

**Interfaces:**
- Produces:
  - `pub type JobId = u64; pub type BatchId = u64;`
  - `enum Direction { Upload, Download }`, `enum JobKind { File, Dir }`, `enum JobNote { DirLink, BrokenLink, Special }`, `enum JobState { Queued, Running, WaitingDecision, Paused, Done, Skipped, Failed, Cancelled }` with `fn is_final(self) -> bool`.
  - `struct Job { id, batch_id, direction, source: String, target: String, rel: String, kind: JobKind, size: u64, done: u64, state: JobState, note: Option<JobNote>, error: Option<CoreError> }`
  - `struct FileStamp { size: u64, mtime: Option<i64> }`, `enum Decision { Overwrite, Skip, Newer, CancelBatch }`, `enum CancelTarget { Job { id: JobId }, Batch { id: BatchId } }`
  - `enum TransferEvent { Job { job: Job }, Progress { job_id: JobId, done: u64 }, Conflict { job_id: JobId, source: FileStamp, target: FileStamp }, BatchDone { batch_id: BatchId, target_dir: String, direction: Direction } }` (tag `kind`, camelCase)
  - `struct TransferOptions { parallel: usize, upload_mode: Option<u32> }` with `fn from_settings(&SftpSettings) -> Self`, `fn file_mode(&self, Direction, source_mode: u32) -> u32`, `fn dir_mode(&self, Direction, source_mode: u32) -> u32`
  - `trait Side` / `Reader` / `Writer`, `struct Node { kind: NodeKind, size: u64, mtime: Option<i64>, mode: u32 }`, `enum NodeKind { File, Dir, DirLink, BrokenLink, Other }`, `struct Child { name: String, lossy: bool }`
  - `struct LocalSide;` implementing `Side`.

- [ ] **Step 1: Create the branch**

```bash
git checkout -b feat/tp3b-transfers
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/transfer/mod.rs` (tests module at the bottom):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SftpSettings;

    #[test]
    fn upload_mode_applies_to_files_and_adds_x_for_dirs() {
        let o = TransferOptions { parallel: 4, upload_mode: Some(0o644) };
        assert_eq!(o.file_mode(Direction::Upload, 0o100600), 0o644);
        assert_eq!(o.dir_mode(Direction::Upload, 0o40700), 0o755);
        let o = TransferOptions { parallel: 4, upload_mode: Some(0o640) };
        assert_eq!(o.dir_mode(Direction::Upload, 0o40700), 0o750);
    }

    #[test]
    fn without_upload_mode_the_local_mode_is_kept() {
        let o = TransferOptions { parallel: 4, upload_mode: None };
        assert_eq!(o.file_mode(Direction::Upload, 0o100751), 0o751);
        assert_eq!(o.dir_mode(Direction::Upload, 0o40711), 0o711);
    }

    #[test]
    fn downloads_keep_the_remote_mode_but_stay_usable() {
        let o = TransferOptions { parallel: 4, upload_mode: Some(0o666) };
        assert_eq!(o.file_mode(Direction::Download, 0o100600), 0o600);
        assert_eq!(o.file_mode(Direction::Download, 0o100444), 0o644);
        assert_eq!(o.dir_mode(Direction::Download, 0o40500), 0o700);
    }

    #[test]
    fn options_from_settings_fall_back_on_invalid_values() {
        let mut s = SftpSettings { parallel: 3, upload_perms: Some("0640".into()), ..SftpSettings::default() };
        assert_eq!(TransferOptions::from_settings(&s), TransferOptions { parallel: 3, upload_mode: Some(0o640) });
        s.parallel = 0;
        s.upload_perms = Some("rwx".into());
        assert_eq!(TransferOptions::from_settings(&s), TransferOptions { parallel: 1, upload_mode: None });
        s.upload_perms = Some("  ".into());
        assert_eq!(TransferOptions::from_settings(&s).upload_mode, None);
    }

    #[test]
    fn events_serialize_with_kind_tag() {
        let event = TransferEvent::Progress { job_id: 7, done: 10 };
        assert_eq!(serde_json::to_value(&event).unwrap(), serde_json::json!({ "kind": "progress", "jobId": 7, "done": 10 }));
        let target = CancelTarget::Batch { id: 2 };
        assert_eq!(serde_json::to_value(&target).unwrap(), serde_json::json!({ "type": "batch", "id": 2 }));
    }

    #[test]
    fn final_states() {
        use JobState::*;
        for s in [Done, Skipped, Failed, Cancelled] {
            assert!(s.is_final());
        }
        for s in [Queued, Running, WaitingDecision, Paused] {
            assert!(!s.is_final());
        }
    }
}
```

`crates/core/src/transfer/local_side.rs` (tests module at the bottom):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FsKind;
    use std::os::unix::fs::PermissionsExt;

    fn p(path: &std::path::Path) -> String {
        path.to_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn stat_classifies_files_dirs_and_links() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::write(root.join("f"), b"abc").unwrap();
        std::fs::create_dir(root.join("dir")).unwrap();
        std::os::unix::fs::symlink(root.join("f"), root.join("lf")).unwrap();
        std::os::unix::fs::symlink(root.join("dir"), root.join("ld")).unwrap();
        std::os::unix::fs::symlink(root.join("gone"), root.join("lb")).unwrap();
        let side = LocalSide;
        let f = side.stat(&p(&root.join("f"))).await.unwrap().unwrap();
        assert_eq!((f.kind, f.size), (NodeKind::File, 3));
        assert!(f.mtime.is_some());
        assert_eq!(side.stat(&p(&root.join("dir"))).await.unwrap().unwrap().kind, NodeKind::Dir);
        assert_eq!(side.stat(&p(&root.join("lf"))).await.unwrap().unwrap().kind, NodeKind::File);
        assert_eq!(side.stat(&p(&root.join("ld"))).await.unwrap().unwrap().kind, NodeKind::DirLink);
        assert_eq!(side.stat(&p(&root.join("lb"))).await.unwrap().unwrap().kind, NodeKind::BrokenLink);
        assert!(side.stat(&p(&root.join("none"))).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn write_read_mode_mtime_and_rename_over() {
        let d = tempfile::tempdir().unwrap();
        let part = p(&d.path().join("x.filepart"));
        let target = p(&d.path().join("x"));
        std::fs::write(&target, b"old").unwrap();
        let side = LocalSide;
        let w = side.open_write(&part, false).await.unwrap();
        w.write_at(3, b"def".to_vec()).await.unwrap();
        w.write_at(0, b"abc".to_vec()).await.unwrap();
        w.close().await.unwrap();
        assert_eq!(std::fs::metadata(&part).unwrap().permissions().mode() & 0o777, 0o600, "parts start private");
        let w = side.open_write(&part, true).await.unwrap();
        w.write_at(6, b"g".to_vec()).await.unwrap();
        w.close().await.unwrap();
        let r = side.open_read(&part).await.unwrap();
        assert_eq!(r.read_at(2, 10).await.unwrap(), b"cdefg");
        assert_eq!(r.read_at(7, 10).await.unwrap(), b"");
        r.close().await;
        side.set_mode(&part, 0o640).await.unwrap();
        side.set_mtime(&part, 1_000_000).await.unwrap();
        side.rename_over(&part, &target).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"abcdefg");
        let node = side.stat(&target).await.unwrap().unwrap();
        assert_eq!((node.mode & 0o777, node.mtime), (0o640, Some(1_000_000)));
        assert!(side.stat(&part).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn mkdir_children_remove_and_errors() {
        let d = tempfile::tempdir().unwrap();
        let side = LocalSide;
        let dir = p(&d.path().join("a"));
        side.mkdir(&dir).await.unwrap();
        assert!(matches!(side.mkdir(&dir).await, Err(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
        std::fs::write(d.path().join("a/f"), b"").unwrap();
        let bad = std::ffi::OsStr::from_bytes(b"bad\xff");
        std::fs::write(d.path().join("a").join(bad), b"").unwrap();
        let mut children = side.children(&dir).await.unwrap();
        children.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(children.len(), 2);
        assert!(children.iter().any(|c| c.name == "f" && !c.lossy));
        assert!(children.iter().any(|c| c.lossy));
        side.remove(&p(&d.path().join("a/f"))).await.unwrap();
        assert!(matches!(side.open_read(&p(&d.path().join("a/f"))).await, Err(CoreError::Fs { kind: FsKind::NotFound, .. })));
    }

    use std::os::unix::ffi::OsStrExt;
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p linascp-core transfer::`
Expected: FAIL to compile (`transfer` module and types missing).

- [ ] **Step 4: Implement**

`crates/core/src/lib.rs`: add `pub mod transfer;` after `pub mod store;`.

`crates/core/src/transfer/mod.rs`:

```rust
//! File transfers between the local file system and an SFTP session (TP3b).

mod copy;
mod local_side;
mod queue;
mod side;
#[cfg(test)]
mod test_support;

pub use local_side::LocalSide;
pub use queue::{TransferEmit, TransferQueue};
pub use side::{Child, Node, NodeKind, Reader, Side, Writer};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::model::SftpSettings;

pub type JobId = u64;
pub type BatchId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    File,
    Dir,
}

/// Why a job was skipped without a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum JobNote {
    DirLink,
    BrokenLink,
    Special,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Queued,
    Running,
    WaitingDecision,
    /// The connection is gone; resumes after a reconnect.
    Paused,
    Done,
    Skipped,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn is_final(self) -> bool {
        matches!(self, Self::Done | Self::Skipped | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: JobId,
    pub batch_id: BatchId,
    pub direction: Direction,
    /// Absolute path on the source side.
    pub source: String,
    /// Absolute path on the target side.
    pub target: String,
    /// Path relative to the batch, for display.
    pub rel: String,
    pub kind: JobKind,
    pub size: u64,
    /// Bytes confirmed written (contiguous from 0).
    pub done: u64,
    pub state: JobState,
    pub note: Option<JobNote>,
    pub error: Option<CoreError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    pub size: u64,
    pub mtime: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Overwrite,
    Skip,
    /// Overwrite only when the source has a newer mtime.
    Newer,
    CancelBatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CancelTarget {
    Job { id: JobId },
    Batch { id: BatchId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TransferEvent {
    /// Full job on creation and on every state change.
    Job { job: Job },
    /// Throttled byte progress of a running file.
    Progress { job_id: JobId, done: u64 },
    /// The target exists; answer with `TransferQueue::resolve`.
    Conflict { job_id: JobId, source: FileStamp, target: FileStamp },
    /// Every job of the batch reached a final state.
    BatchDone { batch_id: BatchId, target_dir: String, direction: Direction },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferOptions {
    /// Files transferred at the same time (directories do not count).
    pub parallel: usize,
    /// Host setting "Rechte beim Upload".
    pub upload_mode: Option<u32>,
}

impl TransferOptions {
    /// Values are validated on save; fall back anyway (1 file, no mode) so a bad row never breaks transfers.
    pub fn from_settings(settings: &SftpSettings) -> Self {
        let upload_mode = settings
            .upload_perms
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty() && m.len() <= 4 && m.chars().all(|c| ('0'..='7').contains(&c)))
            .and_then(|m| u32::from_str_radix(m, 8).ok());
        Self { parallel: (settings.parallel as usize).max(1), upload_mode }
    }

    pub fn file_mode(&self, direction: Direction, source_mode: u32) -> u32 {
        match direction {
            Direction::Upload => self.upload_mode.unwrap_or(source_mode & 0o777),
            Direction::Download => (source_mode & 0o777) | 0o600,
        }
    }

    pub fn dir_mode(&self, direction: Direction, source_mode: u32) -> u32 {
        match direction {
            // x wherever r is set: 0644 -> 0755.
            Direction::Upload => self.upload_mode.map(|m| m | ((m & 0o444) >> 2)).unwrap_or(source_mode & 0o777),
            Direction::Download => (source_mode & 0o777) | 0o700,
        }
    }
}
```

`crates/core/src/transfer/side.rs`:

```rust
//! What the transfer engine needs from either end of a transfer.

use async_trait::async_trait;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A regular file, or a symlink to one (then size, mtime and mode are the target's).
    File,
    Dir,
    /// A symlink to a directory: never followed.
    DirLink,
    BrokenLink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub size: u64,
    pub mtime: Option<i64>,
    /// `st_mode` (type and permission bits).
    pub mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Child {
    pub name: String,
    /// The real name is not valid UTF-8; `name` must not be used for operations.
    pub lossy: bool,
}

#[async_trait]
pub trait Side: Send + Sync {
    /// `None` when nothing exists at `path`.
    async fn stat(&self, path: &str) -> Result<Option<Node>>;
    /// Raw names without `.` and `..`.
    async fn children(&self, path: &str) -> Result<Vec<Child>>;
    /// `Fs { AlreadyExists }` when something exists at `path`.
    async fn mkdir(&self, path: &str) -> Result<()>;
    async fn open_read(&self, path: &str) -> Result<Box<dyn Reader>>;
    /// `resume`: keep the existing content (no truncate). New files start with mode 0600.
    async fn open_write(&self, path: &str, resume: bool) -> Result<Box<dyn Writer>>;
    async fn set_mode(&self, path: &str, mode: u32) -> Result<()>;
    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()>;
    /// Atomically replaces `to` if it exists.
    async fn rename_over(&self, from: &str, to: &str) -> Result<()>;
    async fn remove(&self, path: &str) -> Result<()>;
}

#[async_trait]
pub trait Reader: Send + Sync {
    /// Up to `len` bytes at `offset`; fewer only at the end of the file.
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>>;
    async fn close(&self);
}

#[async_trait]
pub trait Writer: Send + Sync {
    async fn write_at(&self, offset: u64, data: Vec<u8>) -> Result<()>;
    async fn close(&self) -> Result<()>;
}
```

`crates/core/src/transfer/local_side.rs`:

```rust
//! The local file system as a transfer side.

use std::fs::{File, OpenOptions, Permissions};
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use async_trait::async_trait;

use super::side::{Child, Node, NodeKind, Reader, Side, Writer};
use crate::error::{CoreError, Result};
use crate::sftp::local::io_error;

pub struct LocalSide;

async fn run<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| CoreError::internal(e.to_string()))?
}

fn node(meta: &std::fs::Metadata, kind: NodeKind) -> Node {
    Node { kind, size: meta.len(), mtime: Some(meta.mtime()), mode: meta.mode() }
}

fn stat_sync(path: &str) -> Result<Option<Node>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(e, path)),
    };
    let ft = meta.file_type();
    Ok(Some(if ft.is_symlink() {
        match std::fs::metadata(path) {
            Ok(target) if target.is_dir() => node(&target, NodeKind::DirLink),
            Ok(target) if target.is_file() => node(&target, NodeKind::File),
            Ok(target) => node(&target, NodeKind::Other),
            Err(_) => node(&meta, NodeKind::BrokenLink),
        }
    } else if ft.is_dir() {
        node(&meta, NodeKind::Dir)
    } else if ft.is_file() {
        node(&meta, NodeKind::File)
    } else {
        node(&meta, NodeKind::Other)
    }))
}

struct LocalFile {
    file: Arc<File>,
    path: String,
}

#[async_trait]
impl Reader for LocalFile {
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let (file, path) = (self.file.clone(), self.path.clone());
        run(move || {
            let mut buf = vec![0; len];
            let mut filled = 0;
            while filled < len {
                match file.read_at(&mut buf[filled..], offset + filled as u64) {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(io_error(e, &path)),
                }
            }
            buf.truncate(filled);
            Ok(buf)
        })
        .await
    }

    async fn close(&self) {}
}

#[async_trait]
impl Writer for LocalFile {
    async fn write_at(&self, offset: u64, data: Vec<u8>) -> Result<()> {
        let (file, path) = (self.file.clone(), self.path.clone());
        run(move || file.write_all_at(&data, offset).map_err(|e| io_error(e, &path))).await
    }

    async fn close(&self) -> Result<()> {
        let (file, path) = (self.file.clone(), self.path.clone());
        run(move || file.sync_all().map_err(|e| io_error(e, &path))).await
    }
}

#[async_trait]
impl Side for LocalSide {
    async fn stat(&self, path: &str) -> Result<Option<Node>> {
        let path = path.to_string();
        run(move || stat_sync(&path)).await
    }

    async fn children(&self, path: &str) -> Result<Vec<Child>> {
        let path = path.to_string();
        run(move || {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(&path).map_err(|e| io_error(e, &path))? {
                let name = entry.map_err(|e| io_error(e, &path))?.file_name();
                out.push(match name.to_str() {
                    Some(s) => Child { name: s.to_string(), lossy: false },
                    None => Child { name: name.to_string_lossy().into_owned(), lossy: true },
                });
            }
            Ok(out)
        })
        .await
    }

    async fn mkdir(&self, path: &str) -> Result<()> {
        let path = path.to_string();
        run(move || std::fs::create_dir(&path).map_err(|e| io_error(e, &path))).await
    }

    async fn open_read(&self, path: &str) -> Result<Box<dyn Reader>> {
        let path = path.to_string();
        run(move || {
            let file = File::open(&path).map_err(|e| io_error(e, &path))?;
            Ok(Box::new(LocalFile { file: Arc::new(file), path }) as Box<dyn Reader>)
        })
        .await
    }

    async fn open_write(&self, path: &str, resume: bool) -> Result<Box<dyn Writer>> {
        let path = path.to_string();
        run(move || {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(!resume)
                .mode(0o600)
                .open(&path)
                .map_err(|e| io_error(e, &path))?;
            Ok(Box::new(LocalFile { file: Arc::new(file), path }) as Box<dyn Writer>)
        })
        .await
    }

    async fn set_mode(&self, path: &str, mode: u32) -> Result<()> {
        let path = path.to_string();
        run(move || std::fs::set_permissions(&path, Permissions::from_mode(mode)).map_err(|e| io_error(e, &path))).await
    }

    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()> {
        let path = path.to_string();
        run(move || {
            let time = UNIX_EPOCH + Duration::from_secs(mtime.max(0) as u64);
            let file = OpenOptions::new().write(true).open(&path).map_err(|e| io_error(e, &path))?;
            file.set_modified(time).map_err(|e| io_error(e, &path))
        })
        .await
    }

    async fn rename_over(&self, from: &str, to: &str) -> Result<()> {
        let (from, to) = (from.to_string(), to.to_string());
        run(move || std::fs::rename(Path::new(&from), Path::new(&to)).map_err(|e| io_error(e, &to))).await
    }

    async fn remove(&self, path: &str) -> Result<()> {
        let path = path.to_string();
        run(move || std::fs::remove_file(&path).map_err(|e| io_error(e, &path))).await
    }
}
```

Create empty stubs so the module compiles (filled in by Tasks 2–4):
- `crates/core/src/transfer/copy.rs`: `//! Pipelined copy (Task 2).`
- `crates/core/src/transfer/queue.rs`: `pub type TransferEmit = std::sync::Arc<dyn Fn(super::TransferEvent) + Send + Sync>;` and `pub struct TransferQueue;`
- `crates/core/src/transfer/test_support.rs`: `//! Test helpers (Task 3).`

If `io_error` does not map `NotFound`/`AlreadyExists` to the `FsKind`s the tests expect, check `crates/core/src/sftp/local.rs:17` – it already does for TP3a.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p linascp-core transfer::`
Expected: PASS (9 tests), no warnings except unused stubs — silence those by using them in later tasks; if clippy complains now, add `#[allow(dead_code)]` only on the stub items and remove it in Task 3.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/lib.rs crates/core/src/transfer
git commit -m "feat(core): transfer types, Side trait and the local side

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Pipelined copy with a contiguous watermark

**Files:**
- Modify: `crates/core/src/transfer/copy.rs`

**Interfaces:**
- Consumes: `Reader`, `Writer` (Task 1).
- Produces: `pub(crate) const CHUNK: usize = 64 * 1024; pub(crate) const IN_FLIGHT: usize = 32;` and
  `pub(crate) async fn copy_range(reader: &dyn Reader, writer: &dyn Writer, from: u64, size: u64, cancel: &CancellationToken, progress: &mut (dyn FnMut(u64) + Send)) -> Result<u64, (CoreError, u64)>` — `Ok(done)`; `Err((error, watermark))` where `watermark` is the contiguous prefix confirmed written. A cancel returns `Err((CoreError::Cancelled, watermark))`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::{LocalSide, Side};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn p(path: &std::path::Path) -> String {
        path.to_str().unwrap().to_string()
    }

    async fn copy_file(data: &[u8], from: u64) -> (Vec<u8>, Vec<u64>, std::result::Result<u64, (CoreError, u64)>) {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (p(&d.path().join("s")), p(&d.path().join("d")));
        std::fs::write(&src, data).unwrap();
        let side = LocalSide;
        let r = side.open_read(&src).await.unwrap();
        let w = side.open_write(&dst, from > 0).await.unwrap();
        let mut seen = Vec::new();
        let result = copy_range(&*r, &*w, from, data.len() as u64, &CancellationToken::new(), &mut |d| seen.push(d)).await;
        w.close().await.unwrap();
        (std::fs::read(&dst).unwrap(), seen, result)
    }

    #[tokio::test]
    async fn copies_multi_chunk_files_and_reports_a_rising_watermark() {
        let data: Vec<u8> = (0..(CHUNK * 3 + 17)).map(|i| (i % 251) as u8).collect();
        let (out, seen, result) = copy_file(&data, 0).await;
        assert_eq!(out, data);
        assert_eq!(result.unwrap(), data.len() as u64);
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        assert_eq!(*seen.last().unwrap(), data.len() as u64);
    }

    #[tokio::test]
    async fn empty_files_copy_nothing() {
        let (out, seen, result) = copy_file(b"", 0).await;
        assert!(out.is_empty() && seen.is_empty());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn resumes_from_an_offset_reading_only_the_rest() {
        struct Counting<'a>(&'a dyn Reader, AtomicU64);
        #[async_trait::async_trait]
        impl Reader for Counting<'_> {
            async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
                let bytes = self.0.read_at(offset, len).await?;
                self.1.fetch_add(bytes.len() as u64, Ordering::SeqCst);
                Ok(bytes)
            }
            async fn close(&self) {}
        }
        let d = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..(CHUNK * 2 + 5)).map(|i| (i % 13) as u8).collect();
        let (src, dst) = (p(&d.path().join("s")), p(&d.path().join("d")));
        std::fs::write(&src, &data).unwrap();
        std::fs::write(&dst, &data[..CHUNK]).unwrap();
        let side = LocalSide;
        let inner = side.open_read(&src).await.unwrap();
        let r = Counting(&*inner, AtomicU64::new(0));
        let w = side.open_write(&dst, true).await.unwrap();
        let done = copy_range(&r, &*w, CHUNK as u64, data.len() as u64, &CancellationToken::new(), &mut |_| {}).await;
        assert_eq!(done.unwrap(), data.len() as u64);
        assert_eq!(r.1.load(Ordering::SeqCst), (data.len() - CHUNK) as u64);
        assert_eq!(std::fs::read(&dst).unwrap(), data);
    }

    #[tokio::test]
    async fn copy_stops_at_a_short_read() {
        // The source shrank after its size was taken: copy what is there, report it, do not hang.
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (p(&d.path().join("s")), p(&d.path().join("d")));
        std::fs::write(&src, vec![7u8; CHUNK + 10]).unwrap();
        let side = LocalSide;
        let r = side.open_read(&src).await.unwrap();
        let w = side.open_write(&dst, false).await.unwrap();
        let done = copy_range(&*r, &*w, 0, (CHUNK * 4) as u64, &CancellationToken::new(), &mut |_| {}).await;
        assert_eq!(done.unwrap(), (CHUNK + 10) as u64);
    }

    #[tokio::test]
    async fn a_cancel_returns_cancelled_with_the_watermark() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (p(&d.path().join("s")), p(&d.path().join("d")));
        std::fs::write(&src, vec![1u8; CHUNK * 8]).unwrap();
        let side = LocalSide;
        let r = side.open_read(&src).await.unwrap();
        let w = side.open_write(&dst, false).await.unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = copy_range(&*r, &*w, 0, (CHUNK * 8) as u64, &cancel, &mut |_| {}).await.unwrap_err();
        assert_eq!(err, (CoreError::Cancelled, 0));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core transfer::copy`
Expected: FAIL to compile (`copy_range`, `CHUNK` missing).

- [ ] **Step 3: Implement**

```rust
//! Pipelined copy of one file: up to `IN_FLIGHT` read+write pairs run at once.

use futures::{StreamExt, stream};
use tokio_util::sync::CancellationToken;

use super::side::{Reader, Writer};
use crate::error::{CoreError, Result};

/// OpenSSH's sftp-server answers at most 261 120 bytes per read; 64 KiB stays well below.
pub(crate) const CHUNK: usize = 64 * 1024;
/// 2 MiB in flight per file: enough to fill long links without hogging the channel window.
pub(crate) const IN_FLIGHT: usize = 32;

/// Copies `[from, size)` and returns the end offset. `progress` sees the contiguous prefix that is
/// confirmed written; on error that prefix comes back with the error (resume point).
pub(crate) async fn copy_range(
    reader: &dyn Reader,
    writer: &dyn Writer,
    from: u64,
    size: u64,
    cancel: &CancellationToken,
    progress: &mut (dyn FnMut(u64) + Send),
) -> std::result::Result<u64, (CoreError, u64)> {
    let mut done = from;
    if cancel.is_cancelled() {
        return Err((CoreError::Cancelled, done));
    }
    // `buffered` runs the pairs concurrently but yields them in order: each yielded pair extends
    // the contiguous prefix.
    let mut pairs = stream::iter((from..size).step_by(CHUNK))
        .map(|offset| async move {
            let len = (size - offset).min(CHUNK as u64) as usize;
            let data = reader.read_at(offset, len).await?;
            let read = data.len();
            if read > 0 {
                writer.write_at(offset, data).await?;
            }
            Ok::<_, CoreError>((offset, read, len))
        })
        .buffered(IN_FLIGHT);
    loop {
        let next = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err((CoreError::Cancelled, done)),
            next = pairs.next() => next,
        };
        match next {
            None => return Ok(done),
            Some(Ok((offset, read, len))) => {
                done = offset + read as u64;
                progress(done);
                if read < len {
                    // The source ended early (it shrank): stop here.
                    return Ok(done);
                }
            }
            Some(Err(error)) => return Err((error, done)),
        }
    }
}
```

`Result` alias import: if `Result` is unused outside tests, drop it from the `use` line.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p linascp-core transfer::copy`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/transfer/copy.rs
git commit -m "feat(core): pipelined file copy with a resumable watermark

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `TransferQueue` – enqueue, expansion, parallel limit, parts, modes, events

**Files:**
- Modify: `crates/core/src/transfer/queue.rs`, `crates/core/src/transfer/test_support.rs`

**Interfaces:**
- Consumes: Task 1 types, `copy_range` (Task 2), `validate_file_name` (`crate::validate`), `join` (`crate::sftp`).
- Produces:
  - `pub type TransferEmit = Arc<dyn Fn(TransferEvent) + Send + Sync>;`
  - `#[derive(Clone)] pub struct TransferQueue` with
    - `pub fn new(local: Arc<dyn Side>, options: TransferOptions, emit: TransferEmit) -> Self`
    - `pub fn set_remote(&self, remote: Option<Arc<dyn Side>>)`
    - `pub async fn enqueue(&self, direction: Direction, sources: Vec<String>, target_dir: String) -> Result<BatchId>`
    - `pub fn resolve(&self, job: JobId, decision: Decision, apply_to_all: bool) -> Result<()>` (Task 4)
    - `pub fn cancel(&self, target: CancelTarget) -> Result<()>` (Task 4)
    - `pub fn retry(&self, job: JobId) -> Result<()>` (Task 4)
    - `pub fn clear(&self)` (Task 4)
    - `pub async fn close(&self)` (Task 4)
  - test_support: `pub(crate) struct Recorder` (`emit()`, `wait(pred, what)`, `jobs()`, `events()`), `pub(crate) struct FaultySide` (Task 5 adds faults).

- [ ] **Step 1: Write the test helpers**

`crates/core/src/transfer/test_support.rs`:

```rust
//! Helpers for engine tests: an event recorder and a side that fails on demand.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Job, JobId, TransferEmit, TransferEvent};

#[derive(Clone, Default)]
pub(crate) struct Recorder {
    events: Arc<Mutex<Vec<TransferEvent>>>,
}

impl Recorder {
    pub(crate) fn emit(&self) -> TransferEmit {
        let events = self.events.clone();
        Arc::new(move |e| events.lock().unwrap().push(e))
    }

    pub(crate) fn events(&self) -> Vec<TransferEvent> {
        self.events.lock().unwrap().clone()
    }

    /// Latest state of every job, by id.
    pub(crate) fn jobs(&self) -> HashMap<JobId, Job> {
        let mut jobs = HashMap::new();
        for event in self.events() {
            if let TransferEvent::Job { job } = event {
                jobs.insert(job.id, job);
            }
        }
        jobs
    }

    pub(crate) fn job_by_rel(&self, rel: &str) -> Job {
        self.jobs().into_values().find(|j| j.rel == rel).unwrap_or_else(|| panic!("no job {rel}: {:?}", self.events()))
    }

    pub(crate) async fn wait(&self, what: &str, pred: impl Fn(&Recorder) -> bool) {
        for _ in 0..500 {
            if pred(self) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {what}: {:#?}", self.events());
    }

    pub(crate) async fn wait_all_final(&self) {
        self.wait("all jobs final", |r| {
            let jobs = r.jobs();
            !jobs.is_empty() && jobs.values().all(|j| j.state.is_final())
        })
        .await;
    }
}

pub(crate) fn p(path: &std::path::Path) -> String {
    path.to_str().unwrap().to_string()
}
```

- [ ] **Step 2: Write the failing tests**

At the bottom of `crates/core/src/transfer/queue.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FsKind;
    use crate::transfer::test_support::{Recorder, p};
    use crate::transfer::{JobKind, JobNote, JobState, LocalSide};
    use std::os::unix::fs::PermissionsExt;

    struct Env {
        _dir: tempfile::TempDir,
        local: std::path::PathBuf,
        remote: std::path::PathBuf,
        rec: Recorder,
        queue: TransferQueue,
    }

    fn env(options: TransferOptions) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let (local, remote) = (dir.path().join("local"), dir.path().join("remote"));
        std::fs::create_dir_all(&local).unwrap();
        std::fs::create_dir_all(&remote).unwrap();
        let rec = Recorder::default();
        let queue = TransferQueue::new(Arc::new(LocalSide), options, rec.emit());
        queue.set_remote(Some(Arc::new(LocalSide)));
        Env { _dir: dir, local, remote, rec, queue }
    }

    fn opts() -> TransferOptions {
        TransferOptions { parallel: 4, upload_mode: None }
    }

    #[tokio::test]
    async fn uploads_a_file_with_content_mtime_and_mode() {
        let e = env(opts());
        let src = e.local.join("a.txt");
        std::fs::write(&src, b"hello").unwrap();
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o640)).unwrap();
        let mtime = std::fs::metadata(&src).unwrap().modified().unwrap();
        let batch = e.queue.enqueue(Direction::Upload, vec![p(&src)], p(&e.remote)).await.unwrap();
        e.rec.wait_all_final().await;
        let target = e.remote.join("a.txt");
        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
        let meta = std::fs::metadata(&target).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o640);
        assert_eq!(meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            mtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
        assert!(!e.remote.join("a.txt.filepart").exists());
        let job = e.rec.job_by_rel("a.txt");
        assert_eq!((job.state, job.size, job.done, job.batch_id), (JobState::Done, 5, 5, batch));
        assert!(e.rec.events().iter().any(|ev| matches!(ev, TransferEvent::BatchDone { batch_id, .. } if *batch_id == batch)));
    }

    #[tokio::test]
    async fn downloads_a_tree_and_merges_into_an_existing_dir() {
        let e = env(TransferOptions { parallel: 2, upload_mode: Some(0o600) });
        std::fs::create_dir_all(e.remote.join("site/css")).unwrap();
        std::fs::create_dir_all(e.remote.join("site/empty")).unwrap();
        std::fs::write(e.remote.join("site/index.html"), b"<h1>").unwrap();
        std::fs::write(e.remote.join("site/css/a.css"), b"a{}").unwrap();
        std::fs::create_dir_all(e.local.join("site")).unwrap();
        std::fs::write(e.local.join("site/keep"), b"k").unwrap();
        e.queue.enqueue(Direction::Download, vec![p(&e.remote.join("site"))], p(&e.local)).await.unwrap();
        e.rec.wait_all_final().await;
        assert_eq!(std::fs::read(e.local.join("site/index.html")).unwrap(), b"<h1>");
        assert_eq!(std::fs::read(e.local.join("site/css/a.css")).unwrap(), b"a{}");
        assert!(e.local.join("site/empty").is_dir());
        assert!(e.local.join("site/keep").exists());
        assert_eq!(e.rec.job_by_rel("site/css/a.css").state, JobState::Done);
        assert_eq!(e.rec.job_by_rel("site").kind, JobKind::Dir);
    }

    #[tokio::test]
    async fn upload_mode_sets_files_and_created_dirs() {
        let e = env(TransferOptions { parallel: 4, upload_mode: Some(0o644) });
        std::fs::create_dir_all(e.local.join("d")).unwrap();
        std::fs::write(e.local.join("d/f"), b"x").unwrap();
        std::fs::set_permissions(e.local.join("d/f"), std::fs::Permissions::from_mode(0o600)).unwrap();
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("d"))], p(&e.remote)).await.unwrap();
        e.rec.wait_all_final().await;
        assert_eq!(std::fs::metadata(e.remote.join("d/f")).unwrap().permissions().mode() & 0o777, 0o644);
        assert_eq!(std::fs::metadata(e.remote.join("d")).unwrap().permissions().mode() & 0o777, 0o755);
    }

    #[tokio::test]
    async fn links_specials_and_lossy_names_are_never_followed_or_used() {
        let e = env(opts());
        let d = e.local.join("d");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(e.local.join("real"), b"content").unwrap();
        std::os::unix::fs::symlink(e.local.join("real"), d.join("filelink")).unwrap();
        std::os::unix::fs::symlink(&e.local, d.join("dirlink")).unwrap();
        std::os::unix::fs::symlink(e.local.join("gone"), d.join("broken")).unwrap();
        use std::os::unix::ffi::OsStrExt;
        std::fs::write(d.join(std::ffi::OsStr::from_bytes(b"bad\xff")), b"").unwrap();
        e.queue.enqueue(Direction::Upload, vec![p(&d)], p(&e.remote)).await.unwrap();
        e.rec.wait_all_final().await;
        assert_eq!(std::fs::read(e.remote.join("d/filelink")).unwrap(), b"content");
        assert!(!e.remote.join("d/dirlink").exists());
        let dirlink = e.rec.job_by_rel("d/dirlink");
        assert_eq!((dirlink.state, dirlink.note), (JobState::Skipped, Some(JobNote::DirLink)));
        assert_eq!(e.rec.job_by_rel("d/broken").note, Some(JobNote::BrokenLink));
        let lossy = e.rec.jobs().into_values().find(|j| j.rel.starts_with("d/bad")).unwrap();
        assert_eq!(lossy.state, JobState::Failed);
        assert!(matches!(lossy.error, Some(CoreError::Config { .. })));
        assert_eq!(std::fs::read_dir(e.remote.join("d")).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn at_most_parallel_files_run_at_once() {
        let e = env(TransferOptions { parallel: 2, upload_mode: None });
        let sources: Vec<String> = (0..6)
            .map(|i| {
                let f = e.local.join(format!("f{i}"));
                std::fs::write(&f, vec![i as u8; 3 * 1024 * 1024]).unwrap();
                p(&f)
            })
            .collect();
        e.queue.enqueue(Direction::Upload, sources, p(&e.remote)).await.unwrap();
        e.rec.wait_all_final().await;
        // Replay the event stream: never more than 2 files in Running at the same time.
        let mut running = std::collections::HashSet::new();
        let mut peak = 0;
        for ev in e.rec.events() {
            if let TransferEvent::Job { job } = ev {
                if job.state == JobState::Running { running.insert(job.id); } else { running.remove(&job.id); }
                peak = peak.max(running.len());
            }
        }
        assert_eq!(peak, 2);
    }

    #[tokio::test]
    async fn a_missing_source_fails_only_its_job() {
        let e = env(opts());
        std::fs::write(e.local.join("ok"), b"1").unwrap();
        e.queue
            .enqueue(Direction::Upload, vec![p(&e.local.join("nope")), p(&e.local.join("ok"))], p(&e.remote))
            .await
            .unwrap();
        e.rec.wait_all_final().await;
        let nope = e.rec.job_by_rel("nope");
        assert_eq!(nope.state, JobState::Failed);
        assert!(matches!(nope.error, Some(CoreError::Fs { kind: FsKind::NotFound, .. })));
        assert_eq!(e.rec.job_by_rel("ok").state, JobState::Done);
    }

    #[tokio::test]
    async fn enqueue_rejects_bad_paths() {
        let e = env(opts());
        for (sources, target) in [
            (vec!["/".to_string()], p(&e.remote)),
            (vec!["relative".to_string()], p(&e.remote)),
            (vec![format!("{}/../x", p(&e.local))], p(&e.remote)),
            (vec![p(&e.local)], "remote".to_string()),
            (vec![p(&e.local)], format!("{}/..", p(&e.remote))),
            (vec![], p(&e.remote)),
        ] {
            assert!(
                matches!(e.queue.enqueue(Direction::Upload, sources.clone(), target).await, Err(CoreError::Config { .. })),
                "{sources:?}"
            );
        }
        assert!(e.rec.events().is_empty());
    }

    #[tokio::test]
    async fn without_a_remote_nothing_starts() {
        let e = env(opts());
        e.queue.set_remote(None);
        std::fs::write(e.local.join("a"), b"1").unwrap();
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("a"))], p(&e.remote)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(e.rec.job_by_rel("a").state, JobState::Queued);
        e.queue.set_remote(Some(Arc::new(LocalSide)));
        e.rec.wait_all_final().await;
        assert_eq!(e.rec.job_by_rel("a").state, JobState::Done);
    }

    #[tokio::test]
    async fn downloads_need_a_remote_to_enqueue() {
        let e = env(opts());
        e.queue.set_remote(None);
        let result = e.queue.enqueue(Direction::Download, vec![p(&e.remote.join("x"))], p(&e.local)).await;
        assert!(matches!(result, Err(CoreError::Network { .. })));
    }

    #[tokio::test]
    async fn progress_is_throttled_but_state_changes_are_not() {
        let e = env(opts());
        let f = e.local.join("big");
        std::fs::write(&f, vec![0u8; 8 * 1024 * 1024]).unwrap();
        e.queue.enqueue(Direction::Upload, vec![p(&f)], p(&e.remote)).await.unwrap();
        e.rec.wait_all_final().await;
        let progress = e.rec.events().iter().filter(|ev| matches!(ev, TransferEvent::Progress { .. })).count();
        // 128 chunks of 64 KiB on a local disk take far less than 250 ms per event.
        assert!(progress < 20, "{progress} progress events");
        let states: Vec<_> = e.rec.events().into_iter().filter_map(|ev| match ev {
            TransferEvent::Job { job } => Some(job.state),
            _ => None,
        }).collect();
        assert_eq!(states, vec![JobState::Queued, JobState::Running, JobState::Running, JobState::Done]);
    }
}
```

The last assertion expects: `Queued` on creation, `Running` on start, `Running` again when the size is known (the job carries `size` from the source stat), `Done`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p linascp-core transfer::queue`
Expected: FAIL to compile (`TransferQueue::new` etc. missing).

- [ ] **Step 4: Implement the engine (enqueue, run, finish)**

`crates/core/src/transfer/queue.rs`:

```rust
//! The per-session transfer engine (TP3b spec §1): FIFO jobs, at most `parallel` files at once,
//! conflicts one at a time, pause on connection loss and resume after the reconnect.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use futures::{StreamExt, stream};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::copy::copy_range;
use super::side::{Node, NodeKind, Side};
use super::{
    BatchId, CancelTarget, Decision, Direction, FileStamp, Job, JobId, JobKind, JobNote, JobState, TransferEvent,
    TransferOptions,
};
use crate::connect::network_error;
use crate::error::{CoreError, FsKind, NetworkKind, Result};
use crate::sftp::join;
use crate::validate::validate_file_name;

pub type TransferEmit = Arc<dyn Fn(TransferEvent) + Send + Sync>;

const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// Concurrent `stat`s while a directory job expands.
const STAT_CONCURRENCY: usize = 32;

struct Entry {
    job: Job,
    /// Contiguous bytes in this session's `.filepart` (0 = start over).
    resume_at: u64,
    /// Source stamp when the part was written; a change means start over.
    stamp: Option<FileStamp>,
    /// Answer of the conflict dialog for this job.
    decided: Option<Decision>,
    /// (source, target) of the open or pending conflict.
    conflict: Option<(FileStamp, FileStamp)>,
    cancel: Option<CancellationToken>,
    last_progress: Option<Instant>,
}

struct Batch {
    direction: Direction,
    target_dir: String,
    /// "Für alle weiteren Konflikte übernehmen".
    default: Option<Decision>,
    done_sent: bool,
}

struct State {
    jobs: Vec<Entry>,
    batches: HashMap<BatchId, Batch>,
    conflict_open: Option<JobId>,
    remote: Option<Arc<dyn Side>>,
    next_id: u64,
    closed: bool,
}

struct Inner {
    state: Mutex<State>,
    local: Arc<dyn Side>,
    options: TransferOptions,
    emit: TransferEmit,
    /// Signalled whenever a worker finishes (`close` waits for running workers).
    idle: Notify,
}

#[derive(Clone)]
pub struct TransferQueue {
    inner: Arc<Inner>,
}

enum Outcome {
    Done,
    Skipped(Option<JobNote>),
    Conflict(FileStamp, FileStamp),
    Failed(CoreError),
    /// The connection broke mid-file (worker view); `run_job` turns it into `Paused`.
    Interrupted { resume_at: u64, stamp: Option<FileStamp> },
    /// The connection broke; `used` is the remote the job ran on.
    Paused { resume_at: u64, stamp: Option<FileStamp>, used: Arc<dyn Side> },
    Cancelled,
    /// A directory job listed its children: they are queued with the `Done`.
    Expanded(Vec<Job>),
}

/// Absolute, no `..`, no NUL: paths come from the UI and must not escape where the user pointed.
fn check_path(path: &str) -> Result<()> {
    if !path.starts_with('/') || path.contains('\0') || path.split('/').any(|c| c == "..") {
        return Err(CoreError::config(format!("invalid path: {path}")));
    }
    Ok(())
}

fn base_name(path: &str) -> Result<String> {
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
    validate_file_name(&name)?;
    Ok(name)
}

fn stamp(node: &Node) -> FileStamp {
    FileStamp { size: node.size, mtime: node.mtime }
}

fn is_newer(source: Option<i64>, target: Option<i64>) -> bool {
    match (source, target) {
        (Some(s), Some(t)) => s > t,
        (Some(_), None) => true,
        _ => false,
    }
}

fn not_connected() -> CoreError {
    network_error(NetworkKind::Other, "not connected")
}

fn same_side(a: &Arc<dyn Side>, b: &Arc<dyn Side>) -> bool {
    std::ptr::eq(Arc::as_ptr(a) as *const (), Arc::as_ptr(b) as *const ())
}

/// Job kind for a stat result; links to dirs, broken links and specials are skipped with a note.
fn classify(node: &Node) -> std::result::Result<JobKind, JobNote> {
    match node.kind {
        NodeKind::File => Ok(JobKind::File),
        NodeKind::Dir => Ok(JobKind::Dir),
        NodeKind::DirLink => Err(JobNote::DirLink),
        NodeKind::BrokenLink => Err(JobNote::BrokenLink),
        NodeKind::Other => Err(JobNote::Special),
    }
}

impl TransferQueue {
    pub fn new(local: Arc<dyn Side>, options: TransferOptions, emit: TransferEmit) -> Self {
        let state = State {
            jobs: Vec::new(),
            batches: HashMap::new(),
            conflict_open: None,
            remote: None,
            next_id: 1,
            closed: false,
        };
        Self { inner: Arc::new(Inner { state: Mutex::new(state), local, options, emit, idle: Notify::new() }) }
    }

    /// The SFTP connection came (back) or went. Paused jobs resume on a new connection.
    pub fn set_remote(&self, remote: Option<Arc<dyn Side>>) {
        let mut st = self.inner.lock();
        let connected = remote.is_some();
        st.remote = remote;
        if connected {
            for entry in st.jobs.iter_mut().filter(|e| e.job.state == JobState::Paused) {
                entry.job.state = JobState::Queued;
                (self.inner.emit)(TransferEvent::Job { job: entry.job.clone() });
            }
            self.inner.kick(&mut st);
        }
    }

    pub async fn enqueue(&self, direction: Direction, sources: Vec<String>, target_dir: String) -> Result<BatchId> {
        if sources.is_empty() {
            return Err(CoreError::config("nothing to transfer"));
        }
        check_path(&target_dir)?;
        let mut names = Vec::with_capacity(sources.len());
        for source in &sources {
            check_path(source)?;
            names.push(base_name(source)?);
        }
        let src = self.inner.source_side(direction)?;
        let nodes: Vec<Result<Option<Node>>> =
            stream::iter(&sources).map(|s| src.stat(s)).buffered(STAT_CONCURRENCY).collect().await;
        let mut st = self.inner.lock();
        let batch_id = st.next_id;
        st.next_id += 1;
        st.batches.insert(batch_id, Batch { direction, target_dir: target_dir.clone(), default: None, done_sent: false });
        let mut jobs = Vec::new();
        for ((source, name), node) in sources.into_iter().zip(names).zip(nodes) {
            let target = join(&target_dir, &name);
            jobs.push(self.inner.new_job(&mut st, batch_id, direction, source, target, name, node));
        }
        for job in jobs {
            self.inner.push(&mut st, job);
        }
        self.inner.check_batch(&mut st, batch_id);
        self.inner.kick(&mut st);
        Ok(batch_id)
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Source side for listing/stat at enqueue time.
    fn source_side(&self, direction: Direction) -> Result<Arc<dyn Side>> {
        match direction {
            Direction::Upload => Ok(self.local.clone()),
            Direction::Download => self.lock().remote.clone().ok_or_else(not_connected),
        }
    }

    fn sides(&self, direction: Direction, remote: &Arc<dyn Side>) -> (Arc<dyn Side>, Arc<dyn Side>) {
        match direction {
            Direction::Upload => (self.local.clone(), remote.clone()),
            Direction::Download => (remote.clone(), self.local.clone()),
        }
    }

    /// A job for one stat result: queued, skipped (links, specials) or failed (missing, error).
    #[allow(clippy::too_many_arguments)]
    fn new_job(
        &self,
        st: &mut State,
        batch_id: BatchId,
        direction: Direction,
        source: String,
        target: String,
        rel: String,
        node: Result<Option<Node>>,
    ) -> Job {
        let id = st.next_id;
        st.next_id += 1;
        let mut job = Job {
            id,
            batch_id,
            direction,
            source,
            target,
            rel,
            kind: JobKind::File,
            size: 0,
            done: 0,
            state: JobState::Queued,
            note: None,
            error: None,
        };
        match node {
            Ok(Some(node)) => match classify(&node) {
                Ok(kind) => {
                    job.kind = kind;
                    if kind == JobKind::File {
                        job.size = node.size;
                    }
                }
                Err(note) => {
                    job.state = JobState::Skipped;
                    job.note = Some(note);
                }
            },
            Ok(None) => {
                job.state = JobState::Failed;
                job.error = Some(CoreError::fs(FsKind::NotFound, &job.source, "not found"));
            }
            Err(error) => {
                job.state = JobState::Failed;
                job.error = Some(error);
            }
        }
        job
    }

    fn push(&self, st: &mut State, job: Job) {
        (self.emit)(TransferEvent::Job { job: job.clone() });
        st.jobs.push(Entry {
            job,
            resume_at: 0,
            stamp: None,
            decided: None,
            conflict: None,
            cancel: None,
            last_progress: None,
        });
    }

    fn entry(st: &mut State, id: JobId) -> Option<&mut Entry> {
        st.jobs.iter_mut().find(|e| e.job.id == id)
    }

    /// Starts queued jobs while slots are free. Directory jobs do not take a slot.
    fn kick(self: &Arc<Self>, st: &mut State) {
        let Some(remote) = st.remote.clone() else { return };
        if st.closed {
            return;
        }
        let mut running =
            st.jobs.iter().filter(|e| e.job.state == JobState::Running && e.job.kind == JobKind::File).count();
        for entry in st.jobs.iter_mut().filter(|e| e.job.state == JobState::Queued) {
            if entry.job.kind == JobKind::File {
                if running >= self.options.parallel {
                    continue;
                }
                running += 1;
            }
            entry.job.state = JobState::Running;
            entry.job.error = None;
            let token = CancellationToken::new();
            entry.cancel = Some(token.clone());
            (self.emit)(TransferEvent::Job { job: entry.job.clone() });
            tokio::spawn(run_job(self.clone(), entry.job.id, remote.clone(), token));
        }
    }

    /// Sends `BatchDone` once all jobs of the batch are final.
    fn check_batch(&self, st: &mut State, batch_id: BatchId) {
        let all_final = st.jobs.iter().filter(|e| e.job.batch_id == batch_id).all(|e| e.job.state.is_final());
        let Some(batch) = st.batches.get_mut(&batch_id) else { return };
        if all_final && !batch.done_sent {
            batch.done_sent = true;
            (self.emit)(TransferEvent::BatchDone {
                batch_id,
                target_dir: batch.target_dir.clone(),
                direction: batch.direction,
            });
        }
    }

    /// Opens the first waiting conflict when none is open.
    fn open_next_conflict(&self, st: &mut State) {
        if st.conflict_open.is_some() {
            return;
        }
        if let Some(entry) = st.jobs.iter().find(|e| e.job.state == JobState::WaitingDecision) {
            let (source, target) = entry.conflict.expect("waiting jobs have a conflict");
            st.conflict_open = Some(entry.job.id);
            (self.emit)(TransferEvent::Conflict { job_id: entry.job.id, source, target });
        }
    }

    fn progress(&self, id: JobId, done: u64) {
        let mut st = self.lock();
        if st.closed {
            return;
        }
        let Some(entry) = Self::entry(&mut st, id) else { return };
        if entry.job.state != JobState::Running {
            return;
        }
        entry.job.done = done;
        let due = entry.last_progress.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY);
        if due {
            entry.last_progress = Some(Instant::now());
            (self.emit)(TransferEvent::Progress { job_id: id, done });
        }
    }

    /// Records the known size of a running file (one `Job` event).
    fn set_size(&self, id: JobId, size: u64, stamp: FileStamp) {
        let mut st = self.lock();
        let closed = st.closed;
        let Some(entry) = Self::entry(&mut st, id) else { return };
        entry.job.size = size;
        entry.stamp = Some(stamp);
        if !closed && entry.job.state == JobState::Running {
            (self.emit)(TransferEvent::Job { job: entry.job.clone() });
        }
    }

    fn finish(self: &Arc<Self>, id: JobId, outcome: Outcome) {
        let mut st = self.lock();
        let closed = st.closed;
        let remote = st.remote.clone();
        let Some(entry) = Self::entry(&mut st, id) else { return };
        entry.cancel = None;
        if entry.job.state != JobState::Running {
            // Cancelled (or closed) meanwhile: the cancel already reported it.
            drop(st);
            self.idle.notify_waiters();
            return;
        }
        let mut children = Vec::new();
        match outcome {
            Outcome::Done => {
                entry.job.state = JobState::Done;
                entry.job.done = entry.job.size;
                entry.resume_at = 0;
            }
            Outcome::Skipped(note) => {
                entry.job.state = JobState::Skipped;
                entry.job.note = note;
            }
            Outcome::Conflict(source, target) => {
                entry.job.state = JobState::WaitingDecision;
                entry.conflict = Some((source, target));
            }
            Outcome::Failed(error) => {
                entry.job.state = JobState::Failed;
                entry.job.error = Some(error);
                entry.resume_at = 0;
            }
            Outcome::Paused { resume_at, stamp, used } => {
                entry.resume_at = resume_at;
                entry.stamp = stamp;
                entry.job.done = resume_at;
                // Already reconnected on a new connection (fast reconnect): go again right away.
                let reconnected = remote.as_ref().is_some_and(|r| !same_side(r, &used));
                entry.job.state = if reconnected { JobState::Queued } else { JobState::Paused };
            }
            Outcome::Cancelled => entry.job.state = JobState::Cancelled,
            Outcome::Expanded(jobs) => {
                entry.job.state = JobState::Done;
                children = jobs;
            }
        }
        let job = entry.job.clone();
        if !closed {
            (self.emit)(TransferEvent::Job { job: job.clone() });
            for child in children {
                self.push(&mut st, child);
            }
            self.open_next_conflict(&mut st);
            self.check_batch(&mut st, job.batch_id);
            self.kick(&mut st);
        }
        drop(st);
        self.idle.notify_waiters();
    }
}

/// Reads what a worker needs from its entry.
struct Snapshot {
    job: Job,
    resume_at: u64,
    stamp: Option<FileStamp>,
    decision: Option<Decision>,
}

async fn run_job(inner: Arc<Inner>, id: JobId, remote: Arc<dyn Side>, cancel: CancellationToken) {
    let snapshot = {
        let mut st = inner.lock();
        let default = |st: &State, batch: BatchId| st.batches.get(&batch).and_then(|b| b.default);
        let Some(entry) = st.jobs.iter().find(|e| e.job.id == id) else { return };
        let batch_default = default(&st, entry.job.batch_id);
        let snap = Snapshot {
            job: entry.job.clone(),
            resume_at: entry.resume_at,
            stamp: entry.stamp,
            decision: entry.decided.or(batch_default),
        };
        if let Some(entry) = Inner::entry(&mut st, id) {
            entry.decided = None;
        }
        snap
    };
    let (src, dst) = inner.sides(snapshot.job.direction, &remote);
    let outcome = match snapshot.job.kind {
        JobKind::Dir => run_dir(&inner, &snapshot.job, &*src, &*dst).await,
        JobKind::File => run_file(&inner, &snapshot, &*src, &*dst, &cancel).await,
    };
    let outcome = match outcome {
        Outcome::Failed(error) if matches!(error, CoreError::Network { .. }) => {
            Outcome::Paused { resume_at: snapshot.resume_at, stamp: snapshot.stamp, used: remote }
        }
        Outcome::Interrupted { resume_at, stamp } => Outcome::Paused { resume_at, stamp, used: remote },
        other => other,
    };
    inner.finish(id, outcome);
}

/// Creates the target directory (an existing one is merged) and lists the children.
async fn run_dir(inner: &Inner, job: &Job, src: &dyn Side, dst: &dyn Side) -> Outcome {
    let source = match src.stat(&job.source).await {
        Ok(Some(node)) if node.kind == NodeKind::Dir => node,
        Ok(Some(_)) => return Outcome::Failed(CoreError::fs(FsKind::Other, &job.source, "no longer a directory")),
        Ok(None) => return Outcome::Failed(CoreError::fs(FsKind::NotFound, &job.source, "not found")),
        Err(error) => return Outcome::Failed(error),
    };
    match dst.stat(&job.target).await {
        Ok(Some(node)) if node.kind == NodeKind::Dir || node.kind == NodeKind::DirLink => {}
        Ok(Some(_)) => return Outcome::Failed(CoreError::fs(FsKind::AlreadyExists, &job.target, "not a directory")),
        Ok(None) => {
            if let Err(error) = dst.mkdir(&job.target).await {
                return Outcome::Failed(error);
            }
            if let Err(error) = dst.set_mode(&job.target, inner.options.dir_mode(job.direction, source.mode)).await {
                return Outcome::Failed(error);
            }
        }
        Err(error) => return Outcome::Failed(error),
    }
    let children = match src.children(&job.source).await {
        Ok(children) => children,
        Err(error) => return Outcome::Failed(error),
    };
    // ponytail: one stat per child; the SFTP readdir attrs would save the round trips for big dirs.
    let stats: Vec<_> = stream::iter(children)
        .map(|child| async move {
            let node = if child.lossy { None } else { Some(src.stat(&join(&job.source, &child.name)).await) };
            (child, node)
        })
        .buffered(STAT_CONCURRENCY)
        .collect()
        .await;
    if let Some(error) = stats.iter().find_map(|(_, n)| match n {
        Some(Err(e @ CoreError::Network { .. })) => Some(e.clone()),
        _ => None,
    }) {
        // Nothing queued yet: the resumed job lists again without duplicates.
        return Outcome::Failed(error);
    }
    let mut st = inner.lock();
    let mut jobs = Vec::with_capacity(stats.len());
    for (child, node) in stats {
        let rel = format!("{}/{}", job.rel, child.name);
        let (source, target) = (join(&job.source, &child.name), join(&job.target, &child.name));
        let node = match node {
            Some(node) => node,
            None => Err(CoreError::config(format!("file name is not valid UTF-8: {}", child.name))),
        };
        jobs.push(inner.new_job(&mut st, job.batch_id, job.direction, source, target, rel, node));
    }
    Outcome::Expanded(jobs)
}

async fn run_file(inner: &Inner, snap: &Snapshot, src: &dyn Side, dst: &dyn Side, cancel: &CancellationToken) -> Outcome {
    let job = &snap.job;
    let node = match src.stat(&job.source).await {
        Ok(Some(node)) if node.kind == NodeKind::File => node,
        Ok(Some(_)) => return Outcome::Failed(CoreError::fs(FsKind::Other, &job.source, "no longer a file")),
        Ok(None) => return Outcome::Failed(CoreError::fs(FsKind::NotFound, &job.source, "not found")),
        Err(error) => return Outcome::Failed(error),
    };
    let now = stamp(&node);
    // A part from this session continues only if the source is unchanged.
    let resume = if snap.resume_at > 0 && snap.stamp == Some(now) { snap.resume_at } else { 0 };
    if resume == 0 {
        match dst.stat(&job.target).await {
            Ok(Some(t)) if matches!(t.kind, NodeKind::Dir | NodeKind::DirLink) => {
                return Outcome::Failed(CoreError::fs(FsKind::AlreadyExists, &job.target, "a directory exists"));
            }
            Ok(Some(t)) => match snap.decision {
                None | Some(Decision::CancelBatch) => return Outcome::Conflict(now, stamp(&t)),
                Some(Decision::Skip) => return Outcome::Skipped(None),
                Some(Decision::Newer) if !is_newer(node.mtime, t.mtime) => return Outcome::Skipped(None),
                Some(Decision::Newer | Decision::Overwrite) => {}
            },
            Ok(None) => {}
            Err(error) => return Outcome::Failed(error),
        }
    }
    inner.set_size(job.id, node.size, now);
    let part = format!("{}.filepart", job.target);
    let result = async {
        let reader = src.open_read(&job.source).await.map_err(|e| (e, resume))?;
        let writer = match dst.open_write(&part, resume > 0).await {
            Ok(writer) => writer,
            Err(e) => {
                reader.close().await;
                return Err((e, resume));
            }
        };
        let mut progress = |done| inner.progress(job.id, done);
        let copied = copy_range(&*reader, &*writer, resume, node.size, cancel, &mut progress).await;
        reader.close().await;
        let closed = writer.close().await;
        let done = copied?;
        closed.map_err(|e| (e, done))?;
        dst.set_mode(&part, inner.options.file_mode(job.direction, node.mode)).await.map_err(|e| (e, done))?;
        if let Some(mtime) = node.mtime {
            dst.set_mtime(&part, mtime).await.map_err(|e| (e, done))?;
        }
        dst.rename_over(&part, &job.target).await.map_err(|e| (e, done))
    }
    .await;
    match result {
        Ok(()) => Outcome::Done,
        Err((CoreError::Network { .. }, done)) => Outcome::Interrupted { resume_at: done, stamp: Some(now) },
        Err((error, _)) => {
            // Cancelled or failed: the part must not stay behind.
            let _ = dst.remove(&part).await;
            if error == CoreError::Cancelled { Outcome::Cancelled } else { Outcome::Failed(error) }
        }
    }
}
```

Workers never build `Paused` themselves: `run_file` returns `Interrupted`, and `run_job` adds the remote it ran on. In `finish`, treat a stray `Interrupted` like `Paused` with `reconnected = false` (add it to the `Paused` arm as `Outcome::Interrupted { resume_at, stamp }` → same fields, state `Paused`).

Also add `resolve`, `cancel`, `retry`, `clear`, `close` as `todo!()`-free stubs only if the compiler requires them for this task — it does not; Task 4 adds them.

`crate::connect::network_error` is already re-exported (`pub use classify::{…, network_error, …}`).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p linascp-core transfer::`
Expected: PASS (all transfer tests). If `progress_is_throttled_but_state_changes_are_not` sees an extra `Running`, check that `set_size` emits exactly once per run.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/transfer
git commit -m "feat(core): transfer queue with recursion, parallel limit, parts and modes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Conflicts, cancel, retry, clear, close

**Files:**
- Modify: `crates/core/src/transfer/queue.rs`

**Interfaces:**
- Consumes: Task 3 internals (`Inner`, `Entry`, `open_next_conflict`, `check_batch`, `kick`).
- Produces: `TransferQueue::{resolve, cancel, retry, clear, close}` with the signatures listed in Task 3.

- [ ] **Step 1: Write the failing tests** (append to the `tests` module of `queue.rs`)

```rust
    fn conflicts(rec: &Recorder) -> Vec<JobId> {
        rec.events().into_iter().filter_map(|e| match e {
            TransferEvent::Conflict { job_id, .. } => Some(job_id),
            _ => None,
        }).collect()
    }

    /// Two local files `a`, `b` that both exist on the remote with older content.
    async fn two_conflicts(e: &Env) {
        for name in ["a", "b"] {
            std::fs::write(e.remote.join(name), b"old").unwrap();
            std::fs::write(e.local.join(name), b"new!").unwrap();
        }
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        for name in ["a", "b"] {
            std::fs::File::options().write(true).open(e.remote.join(name)).unwrap().set_modified(old).unwrap();
        }
        e.queue
            .enqueue(Direction::Upload, vec![p(&e.local.join("a")), p(&e.local.join("b"))], p(&e.remote))
            .await
            .unwrap();
        e.rec.wait("both waiting", |r| r.jobs().values().filter(|j| j.state == JobState::WaitingDecision).count() == 2).await;
    }

    #[tokio::test]
    async fn conflicts_open_one_at_a_time_and_others_keep_running() {
        let e = env(opts());
        two_conflicts(&e).await;
        std::fs::write(e.local.join("c"), b"c").unwrap();
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("c"))], p(&e.remote)).await.unwrap();
        e.rec.wait("c done", |r| r.job_by_rel("c").state == JobState::Done).await;
        assert_eq!(conflicts(&e.rec).len(), 1);
        let a = e.rec.job_by_rel("a");
        assert_eq!(conflicts(&e.rec), vec![a.id]);
        let TransferEvent::Conflict { source, target, .. } =
            e.rec.events().into_iter().find(|ev| matches!(ev, TransferEvent::Conflict { .. })).unwrap()
        else { unreachable!() };
        assert_eq!((source.size, target.size, target.mtime), (4, 3, Some(1_000)));
        e.queue.resolve(a.id, Decision::Skip, false).unwrap();
        e.rec.wait("second conflict", |r| conflicts(r).len() == 2).await;
        assert_eq!(e.rec.job_by_rel("a").state, JobState::Skipped);
        assert_eq!(std::fs::read(e.remote.join("a")).unwrap(), b"old");
    }

    #[tokio::test]
    async fn overwrite_and_apply_to_all_resolve_the_waiting_ones_too() {
        let e = env(opts());
        two_conflicts(&e).await;
        let a = e.rec.job_by_rel("a");
        e.queue.resolve(a.id, Decision::Overwrite, true).unwrap();
        e.rec.wait_all_final().await;
        assert_eq!(std::fs::read(e.remote.join("a")).unwrap(), b"new!");
        assert_eq!(std::fs::read(e.remote.join("b")).unwrap(), b"new!");
        assert_eq!(conflicts(&e.rec).len(), 1, "b never asked");
    }

    #[tokio::test]
    async fn newer_overwrites_only_older_targets() {
        let e = env(opts());
        two_conflicts(&e).await;
        // b on the remote becomes newer than the local file.
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(3_600);
        std::fs::File::options().write(true).open(e.remote.join("b")).unwrap().set_modified(future).unwrap();
        let (a, b) = (e.rec.job_by_rel("a"), e.rec.job_by_rel("b"));
        e.queue.resolve(a.id, Decision::Newer, false).unwrap();
        e.rec.wait("b open", |r| conflicts(r).len() == 2).await;
        e.queue.resolve(b.id, Decision::Newer, false).unwrap();
        e.rec.wait_all_final().await;
        assert_eq!(std::fs::read(e.remote.join("a")).unwrap(), b"new!");
        assert_eq!(std::fs::read(e.remote.join("b")).unwrap(), b"old");
        assert_eq!(e.rec.job_by_rel("b").state, JobState::Skipped);
    }

    #[tokio::test]
    async fn cancel_batch_from_the_dialog_cancels_every_open_job_of_the_batch() {
        let e = env(opts());
        two_conflicts(&e).await;
        let a = e.rec.job_by_rel("a");
        e.queue.resolve(a.id, Decision::CancelBatch, false).unwrap();
        e.rec.wait_all_final().await;
        assert_eq!(e.rec.job_by_rel("a").state, JobState::Cancelled);
        assert_eq!(e.rec.job_by_rel("b").state, JobState::Cancelled);
        assert_eq!(conflicts(&e.rec).len(), 1);
        assert!(e.rec.events().iter().any(|ev| matches!(ev, TransferEvent::BatchDone { .. })));
    }

    #[tokio::test]
    async fn cancelling_the_open_conflict_opens_the_next() {
        let e = env(opts());
        two_conflicts(&e).await;
        let a = e.rec.job_by_rel("a");
        e.queue.cancel(CancelTarget::Job { id: a.id }).unwrap();
        e.rec.wait("b asked", |r| conflicts(r).len() == 2).await;
        assert_eq!(e.rec.job_by_rel("a").state, JobState::Cancelled);
    }

    #[tokio::test]
    async fn a_file_against_a_directory_fails() {
        let e = env(opts());
        std::fs::create_dir(e.remote.join("x")).unwrap();
        std::fs::write(e.local.join("x"), b"1").unwrap();
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("x"))], p(&e.remote)).await.unwrap();
        e.rec.wait_all_final().await;
        let x = e.rec.job_by_rel("x");
        assert_eq!(x.state, JobState::Failed);
        assert!(matches!(x.error, Some(CoreError::Fs { kind: FsKind::AlreadyExists, .. })));
    }

    #[tokio::test]
    async fn cancelling_a_running_job_removes_its_part_and_keeps_finished_files() {
        let e = env(TransferOptions { parallel: 1, upload_mode: None });
        std::fs::write(e.local.join("small"), b"s").unwrap();
        std::fs::write(e.local.join("big"), vec![0u8; 64 * 1024 * 1024]).unwrap();
        e.queue
            .enqueue(Direction::Upload, vec![p(&e.local.join("small")), p(&e.local.join("big"))], p(&e.remote))
            .await
            .unwrap();
        e.rec.wait("big running", |r| r.jobs().values().any(|j| j.rel == "big" && j.state == JobState::Running)).await;
        let big = e.rec.job_by_rel("big");
        e.queue.cancel(CancelTarget::Batch { id: big.batch_id }).unwrap();
        e.rec.wait_all_final().await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(e.rec.job_by_rel("big").state, JobState::Cancelled);
        assert!(!e.remote.join("big.filepart").exists());
        assert!(!e.remote.join("big").exists());
        assert_eq!(std::fs::read(e.remote.join("small")).unwrap(), b"s");
    }

    #[tokio::test]
    async fn retry_requeues_a_failed_job_at_the_end() {
        let e = env(opts());
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("late"))], p(&e.remote)).await.unwrap();
        e.rec.wait_all_final().await;
        let late = e.rec.job_by_rel("late");
        assert_eq!(late.state, JobState::Failed);
        std::fs::write(e.local.join("late"), b"now").unwrap();
        e.queue.retry(late.id).unwrap();
        e.rec.wait("retried", |r| r.job_by_rel("late").state == JobState::Done).await;
        assert_eq!(std::fs::read(e.remote.join("late")).unwrap(), b"now");
        assert!(matches!(e.queue.retry(late.id), Err(CoreError::Internal { .. })), "only failed jobs");
        let done = e.rec.events().iter().filter(|ev| matches!(ev, TransferEvent::BatchDone { .. })).count();
        assert_eq!(done, 2, "the batch completes again after the retry");
    }

    #[tokio::test]
    async fn unknown_ids_are_errors() {
        let e = env(opts());
        assert!(e.queue.resolve(99, Decision::Skip, false).is_err());
        assert!(e.queue.cancel(CancelTarget::Job { id: 99 }).is_err());
        assert!(e.queue.cancel(CancelTarget::Batch { id: 99 }).is_err());
        assert!(e.queue.retry(99).is_err());
    }

    #[tokio::test]
    async fn clear_forgets_finished_jobs_only() {
        let e = env(opts());
        two_conflicts(&e).await;
        std::fs::write(e.local.join("c"), b"c").unwrap();
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("c"))], p(&e.remote)).await.unwrap();
        e.rec.wait("c done", |r| r.job_by_rel("c").state == JobState::Done).await;
        e.queue.clear();
        let c = e.rec.job_by_rel("c");
        assert!(e.queue.retry(c.id).is_err(), "c is gone");
        let a = e.rec.job_by_rel("a");
        e.queue.resolve(a.id, Decision::Skip, false).unwrap();
    }

    #[tokio::test]
    async fn close_cancels_and_removes_parts_without_events() {
        let e = env(TransferOptions { parallel: 1, upload_mode: None });
        std::fs::write(e.local.join("big"), vec![0u8; 64 * 1024 * 1024]).unwrap();
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("big"))], p(&e.remote)).await.unwrap();
        e.rec.wait("running", |r| r.job_by_rel("big").state == JobState::Running).await;
        let before = e.rec.events().len();
        e.queue.close().await;
        assert!(!e.remote.join("big.filepart").exists());
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(e.rec.events().len(), before, "no event after close");
        std::fs::write(e.local.join("x"), b"1").unwrap();
        assert!(e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("x"))], p(&e.remote)).await.is_err());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p linascp-core transfer::queue`
Expected: FAIL to compile (`resolve`, `cancel`, `retry`, `clear`, `close` missing).

- [ ] **Step 3: Implement**

Add to `impl TransferQueue`:

```rust
    pub fn resolve(&self, id: JobId, decision: Decision, apply_to_all: bool) -> Result<()> {
        let mut st = self.inner.lock();
        let entry = Inner::entry(&mut st, id).ok_or_else(|| unknown("job", id))?;
        if entry.job.state != JobState::WaitingDecision {
            return Err(CoreError::internal(format!("job {id} is not waiting for a decision")));
        }
        let batch_id = entry.job.batch_id;
        if st.conflict_open == Some(id) {
            st.conflict_open = None;
        }
        if decision == Decision::CancelBatch {
            let parts = self.inner.cancel_where(&mut st, |job| job.batch_id == batch_id);
            self.inner.after_change(&mut st, batch_id, parts);
            return Ok(());
        }
        let ids: Vec<JobId> = if apply_to_all {
            if let Some(batch) = st.batches.get_mut(&batch_id) {
                batch.default = Some(decision);
            }
            st.jobs
                .iter()
                .filter(|e| e.job.batch_id == batch_id && e.job.state == JobState::WaitingDecision)
                .map(|e| e.job.id)
                .collect()
        } else {
            vec![id]
        };
        for job_id in ids {
            if st.conflict_open == Some(job_id) {
                st.conflict_open = None;
            }
            let Some(entry) = Inner::entry(&mut st, job_id) else { continue };
            let (source, target) = entry.conflict.take().expect("waiting jobs have a conflict");
            let overwrite = match decision {
                Decision::Overwrite => true,
                Decision::Newer => is_newer(source.mtime, target.mtime),
                Decision::Skip | Decision::CancelBatch => false,
            };
            if overwrite {
                entry.decided = Some(Decision::Overwrite);
                entry.job.state = JobState::Queued;
            } else {
                entry.job.state = JobState::Skipped;
            }
            (self.inner.emit)(TransferEvent::Job { job: entry.job.clone() });
        }
        self.inner.after_change(&mut st, batch_id, Vec::new());
        Ok(())
    }

    pub fn cancel(&self, target: CancelTarget) -> Result<()> {
        let mut st = self.inner.lock();
        let (batch_id, parts) = match target {
            CancelTarget::Job { id } => {
                let batch_id = st.jobs.iter().find(|e| e.job.id == id).ok_or_else(|| unknown("job", id))?.job.batch_id;
                (batch_id, self.inner.cancel_where(&mut st, |job| job.id == id))
            }
            CancelTarget::Batch { id } => {
                if !st.batches.contains_key(&id) {
                    return Err(unknown("batch", id));
                }
                (id, self.inner.cancel_where(&mut st, |job| job.batch_id == id))
            }
        };
        self.inner.after_change(&mut st, batch_id, parts);
        Ok(())
    }

    pub fn retry(&self, id: JobId) -> Result<()> {
        let mut st = self.inner.lock();
        let index = st.jobs.iter().position(|e| e.job.id == id).ok_or_else(|| unknown("job", id))?;
        if st.jobs[index].job.state != JobState::Failed {
            return Err(CoreError::internal(format!("job {id} has not failed")));
        }
        let mut entry = st.jobs.remove(index);
        entry.job.state = JobState::Queued;
        entry.job.error = None;
        entry.job.done = 0;
        entry.resume_at = 0;
        let batch_id = entry.job.batch_id;
        (self.inner.emit)(TransferEvent::Job { job: entry.job.clone() });
        st.jobs.push(entry);
        if let Some(batch) = st.batches.get_mut(&batch_id) {
            batch.done_sent = false;
        }
        self.inner.kick(&mut st);
        Ok(())
    }

    /// Forgets done, skipped and cancelled jobs (the UI clears its list itself).
    pub fn clear(&self) {
        let mut st = self.inner.lock();
        st.jobs.retain(|e| !matches!(e.job.state, JobState::Done | JobState::Skipped | JobState::Cancelled));
        let alive: std::collections::HashSet<BatchId> = st.jobs.iter().map(|e| e.job.batch_id).collect();
        st.batches.retain(|id, _| alive.contains(id));
    }

    /// Cancels everything, removes the parts and waits (up to 5 s) for running workers. No event
    /// after this; later calls fail.
    pub async fn close(&self) {
        let parts = {
            let mut st = self.inner.lock();
            st.closed = true;
            self.inner.cancel_where(&mut st, |_| true)
        };
        self.inner.remove_parts(parts).await;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let idle = self.inner.idle.notified();
            if !self.inner.lock().jobs.iter().any(|e| e.job.state == JobState::Running && e.cancel.is_some()) {
                break;
            }
            if tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), idle).await.is_err() {
                break;
            }
        }
    }
```

`close` relies on `finish` clearing `entry.cancel` and notifying `idle` (Task 3 does both); `cancel_where` marks running jobs `Cancelled` but keeps `cancel` set until the worker finishes, so the loop waits for them.

Add `enqueue`'s first line: `if self.inner.lock().closed { return Err(CoreError::internal("transfer queue is closed")); }`.

Add to `impl Inner`:

```rust
    /// Marks matching unfinished jobs `Cancelled` and stops running ones (their worker removes its
    /// part). Returns the parts of paused jobs, which no worker owns.
    fn cancel_where(&self, st: &mut State, matches: impl Fn(&Job) -> bool) -> Vec<(Direction, String)> {
        let closed = st.closed;
        let mut parts = Vec::new();
        let mut reopen = false;
        for entry in st.jobs.iter_mut().filter(|e| !e.job.state.is_final() && matches(&e.job)) {
            if entry.job.state == JobState::Paused && entry.resume_at > 0 {
                parts.push((entry.job.direction, format!("{}.filepart", entry.job.target)));
            }
            if let Some(token) = &entry.cancel {
                token.cancel();
            }
            reopen |= st.conflict_open == Some(entry.job.id);
            entry.job.state = JobState::Cancelled;
            entry.conflict = None;
            if !closed {
                (self.emit)(TransferEvent::Job { job: entry.job.clone() });
            }
        }
        if reopen {
            st.conflict_open = None;
        }
        parts
    }

    /// Bookkeeping after a resolve or cancel: next conflict, batch end, free slots, orphan parts.
    fn after_change(self: &Arc<Self>, st: &mut State, batch_id: BatchId, parts: Vec<(Direction, String)>) {
        self.open_next_conflict(st);
        self.check_batch(st, batch_id);
        self.kick(st);
        if !parts.is_empty() {
            let inner = self.clone();
            tokio::spawn(async move { inner.remove_parts(parts).await });
        }
    }

    /// Best effort: a remote part stays when there is no connection.
    async fn remove_parts(&self, parts: Vec<(Direction, String)>) {
        let remote = self.lock().remote.clone();
        for (direction, part) in parts {
            let side = match direction {
                Direction::Upload => remote.clone(),
                Direction::Download => Some(self.local.clone()),
            };
            if let Some(side) = side {
                let _ = side.remove(&part).await;
            }
        }
    }
```

And a helper at module level:

```rust
fn unknown(what: &str, id: u64) -> CoreError {
    CoreError::internal(format!("unknown {what} {id}"))
}
```

In `finish`, the early-return branch for non-`Running` entries must stay (a cancelled job's worker reports `Cancelled` after `cancel_where` already changed the state).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p linascp-core transfer::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/transfer/queue.rs
git commit -m "feat(core): transfer conflicts, cancel, retry, clear and close

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Pause on connection loss and resume

**Files:**
- Modify: `crates/core/src/transfer/test_support.rs`, `crates/core/src/transfer/queue.rs` (tests; code only if a test shows a gap)

**Interfaces:**
- Consumes: `TransferQueue::set_remote` (Task 3), `Outcome::Paused` handling.
- Produces: `FaultySide` in test_support:
  - `FaultySide::new() -> Arc<FaultySide>` wrapping `LocalSide`
  - `fail_writes_after(&self, bytes: u64)` – the first write that crosses the limit fails with `Network`, later writes fail too until `heal()`
  - `fail_children(&self)` – `children` fails with `Network` until `heal()`
  - `deny_read(&self, path: &str)` – `open_read(path)` fails with `Fs { PermissionDenied }`
  - `heal(&self)`, `bytes_read(&self) -> u64`

- [ ] **Step 1: Add `FaultySide`** to `test_support.rs`:

```rust
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use async_trait::async_trait;

use super::side::{Child, Node, Reader, Side, Writer};
use super::LocalSide;
use crate::connect::network_error;
use crate::error::{CoreError, FsKind, NetworkKind, Result};

#[derive(Default)]
pub(crate) struct FaultySide {
    write_limit: AtomicU64,
    written: Arc<AtomicU64>,
    read: Arc<AtomicU64>,
    broken: Arc<AtomicBool>,
    children_fail: AtomicBool,
    denied: Mutex<HashSet<String>>,
}

fn dead() -> CoreError {
    network_error(NetworkKind::Other, "connection lost")
}

impl FaultySide {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self { write_limit: AtomicU64::new(u64::MAX), ..Self::default() })
    }
    pub(crate) fn fail_writes_after(&self, bytes: u64) {
        self.written.store(0, Ordering::SeqCst);
        self.write_limit.store(bytes, Ordering::SeqCst);
    }
    pub(crate) fn fail_children(&self) {
        self.children_fail.store(true, Ordering::SeqCst);
    }
    pub(crate) fn deny_read(&self, path: &str) {
        self.denied.lock().unwrap().insert(path.to_string());
    }
    pub(crate) fn heal(&self) {
        self.write_limit.store(u64::MAX, Ordering::SeqCst);
        self.broken.store(false, Ordering::SeqCst);
        self.children_fail.store(false, Ordering::SeqCst);
    }
    pub(crate) fn bytes_read(&self) -> u64 {
        self.read.load(Ordering::SeqCst)
    }
}

struct FaultyWriter {
    inner: Box<dyn Writer>,
    limit: u64,
    written: Arc<AtomicU64>,
    broken: Arc<AtomicBool>,
}

#[async_trait]
impl Writer for FaultyWriter {
    async fn write_at(&self, offset: u64, data: Vec<u8>) -> Result<()> {
        let total = self.written.fetch_add(data.len() as u64, Ordering::SeqCst) + data.len() as u64;
        if self.broken.load(Ordering::SeqCst) || total > self.limit {
            self.broken.store(true, Ordering::SeqCst);
            return Err(dead());
        }
        self.inner.write_at(offset, data).await
    }
    async fn close(&self) -> Result<()> {
        self.inner.close().await
    }
}

struct CountingReader {
    inner: Box<dyn Reader>,
    read: Arc<AtomicU64>,
}

#[async_trait]
impl Reader for CountingReader {
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let data = self.inner.read_at(offset, len).await?;
        self.read.fetch_add(data.len() as u64, Ordering::SeqCst);
        Ok(data)
    }
    async fn close(&self) {
        self.inner.close().await
    }
}

#[async_trait]
impl Side for FaultySide {
    async fn stat(&self, path: &str) -> Result<Option<Node>> {
        LocalSide.stat(path).await
    }
    async fn children(&self, path: &str) -> Result<Vec<Child>> {
        if self.children_fail.load(Ordering::SeqCst) {
            return Err(dead());
        }
        LocalSide.children(path).await
    }
    async fn mkdir(&self, path: &str) -> Result<()> {
        LocalSide.mkdir(path).await
    }
    async fn open_read(&self, path: &str) -> Result<Box<dyn Reader>> {
        if self.denied.lock().unwrap().contains(path) {
            return Err(CoreError::fs(FsKind::PermissionDenied, path, "denied"));
        }
        Ok(Box::new(CountingReader { inner: LocalSide.open_read(path).await?, read: self.read.clone() }))
    }
    async fn open_write(&self, path: &str, resume: bool) -> Result<Box<dyn Writer>> {
        if self.broken.load(Ordering::SeqCst) {
            return Err(dead());
        }
        Ok(Box::new(FaultyWriter {
            inner: LocalSide.open_write(path, resume).await?,
            limit: self.write_limit.load(Ordering::SeqCst),
            written: self.written.clone(),
            broken: self.broken.clone(),
        }))
    }
    async fn set_mode(&self, path: &str, mode: u32) -> Result<()> {
        LocalSide.set_mode(path, mode).await
    }
    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()> {
        LocalSide.set_mtime(path, mtime).await
    }
    async fn rename_over(&self, from: &str, to: &str) -> Result<()> {
        LocalSide.rename_over(from, to).await
    }
    async fn remove(&self, path: &str) -> Result<()> {
        LocalSide.remove(path).await
    }
}
```

Merge the `use` lines with the ones from Task 3 at the top of the file.

- [ ] **Step 2: Write the failing tests** (append to the `tests` module of `queue.rs`)

```rust
    use crate::transfer::test_support::FaultySide;

    fn faulty_env(options: TransferOptions) -> (Env, Arc<FaultySide>) {
        let e = env(options);
        let faulty = FaultySide::new();
        e.queue.set_remote(Some(faulty.clone()));
        (e, faulty)
    }

    #[tokio::test]
    async fn a_lost_connection_pauses_and_the_reconnect_resumes_from_the_part() {
        let (e, faulty) = faulty_env(opts());
        let data: Vec<u8> = (0..(10 * 1024 * 1024)).map(|i| (i % 253) as u8).collect();
        std::fs::write(e.local.join("big"), &data).unwrap();
        faulty.fail_writes_after(3 * 1024 * 1024);
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("big"))], p(&e.remote)).await.unwrap();
        e.rec.wait("paused", |r| r.job_by_rel("big").state == JobState::Paused).await;
        let paused = e.rec.job_by_rel("big");
        assert!(paused.done > 0 && paused.done <= 3 * 1024 * 1024, "{}", paused.done);
        assert!(e.remote.join("big.filepart").exists());
        e.queue.set_remote(None);
        faulty.heal();
        // A new connection: a different `Side` object.
        let fresh = FaultySide::new();
        e.queue.set_remote(Some(fresh.clone()));
        e.rec.wait_all_final().await;
        assert_eq!(e.rec.job_by_rel("big").state, JobState::Done);
        assert_eq!(std::fs::read(e.remote.join("big")).unwrap(), data);
        assert!(!e.remote.join("big.filepart").exists());
    }

    #[tokio::test]
    async fn a_cancelled_download_leaves_no_local_part() {
        let (e, _faulty) = faulty_env(opts());
        std::fs::write(e.remote.join("f"), vec![5u8; 64 * 1024 * 1024]).unwrap();
        e.queue.enqueue(Direction::Download, vec![p(&e.remote.join("f"))], p(&e.local)).await.unwrap();
        e.rec.wait("running", |r| r.job_by_rel("f").state == JobState::Running).await;
        e.queue.cancel(CancelTarget::Job { id: e.rec.job_by_rel("f").id }).unwrap();
        e.rec.wait_all_final().await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!e.local.join("f.filepart").exists());
        assert!(!e.local.join("f").exists());
    }

    #[tokio::test]
    async fn a_changed_source_starts_over_after_the_reconnect() {
        let (e, faulty) = faulty_env(opts());
        std::fs::write(e.local.join("f"), vec![1u8; 4 * 1024 * 1024]).unwrap();
        faulty.fail_writes_after(1024 * 1024);
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("f"))], p(&e.remote)).await.unwrap();
        e.rec.wait("paused", |r| r.job_by_rel("f").state == JobState::Paused).await;
        let new_data = vec![2u8; 3 * 1024 * 1024 + 7];
        std::fs::write(e.local.join("f"), &new_data).unwrap();
        e.queue.set_remote(Some(FaultySide::new()));
        e.rec.wait_all_final().await;
        assert_eq!(std::fs::read(e.remote.join("f")).unwrap(), new_data);
    }

    #[tokio::test]
    async fn a_dir_job_paused_mid_listing_adds_its_children_once() {
        let (e, _faulty) = faulty_env(opts());
        // Download: the remote (faulty) side lists the children.
        let faulty_src = FaultySide::new();
        e.queue.set_remote(Some(faulty_src.clone()));
        std::fs::create_dir_all(e.remote.join("d")).unwrap();
        for i in 0..3 {
            std::fs::write(e.remote.join(format!("d/{i}")), b"x").unwrap();
        }
        faulty_src.fail_children();
        e.queue.enqueue(Direction::Download, vec![p(&e.remote.join("d"))], p(&e.local)).await.unwrap();
        e.rec.wait("paused", |r| r.job_by_rel("d").state == JobState::Paused).await;
        faulty_src.heal();
        e.queue.set_remote(Some(FaultySide::new()));
        e.rec.wait_all_final().await;
        let jobs = e.rec.jobs();
        assert_eq!(jobs.values().filter(|j| j.rel.starts_with("d/")).count(), 3);
        assert_eq!(std::fs::read_dir(e.local.join("d")).unwrap().count(), 3);
    }

    #[tokio::test]
    async fn a_permission_error_fails_only_that_job() {
        let (e, faulty) = faulty_env(opts());
        std::fs::write(e.remote.join("secret"), b"s").unwrap();
        std::fs::write(e.remote.join("open"), b"o").unwrap();
        faulty.deny_read(&p(&e.remote.join("secret")));
        e.queue
            .enqueue(Direction::Download, vec![p(&e.remote.join("secret")), p(&e.remote.join("open"))], p(&e.local))
            .await
            .unwrap();
        e.rec.wait_all_final().await;
        let secret = e.rec.job_by_rel("secret");
        assert_eq!(secret.state, JobState::Failed);
        assert!(matches!(secret.error, Some(CoreError::Fs { kind: FsKind::PermissionDenied, .. })));
        assert!(!e.local.join("secret.filepart").exists());
        assert_eq!(std::fs::read(e.local.join("open")).unwrap(), b"o");
    }

    #[tokio::test]
    async fn queued_jobs_wait_while_disconnected_and_cancel_removes_paused_parts() {
        let (e, faulty) = faulty_env(TransferOptions { parallel: 1, upload_mode: None });
        std::fs::write(e.local.join("a"), vec![1u8; 4 * 1024 * 1024]).unwrap();
        std::fs::write(e.local.join("b"), b"b").unwrap();
        faulty.fail_writes_after(1024 * 1024);
        e.queue
            .enqueue(Direction::Upload, vec![p(&e.local.join("a")), p(&e.local.join("b"))], p(&e.remote))
            .await
            .unwrap();
        e.rec.wait("a paused", |r| r.job_by_rel("a").state == JobState::Paused).await;
        e.queue.set_remote(None);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(e.rec.job_by_rel("b").state, JobState::Queued);
        let a = e.rec.job_by_rel("a");
        e.queue.set_remote(Some(Arc::new(LocalSide)));
        e.queue.cancel(CancelTarget::Job { id: a.id }).unwrap();
        e.rec.wait_all_final().await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!e.remote.join("a.filepart").exists());
        assert_eq!(std::fs::read(e.remote.join("b")).unwrap(), b"b");
    }
```

Reading only the rest of the source on a resume is pinned by Task 2's `resumes_from_an_offset_reading_only_the_rest`; `FaultySide::bytes_read` stays available for debugging.

Note on `queued_jobs_wait_while_disconnected…`: `cancel` of a job that `set_remote(Some)` already re-queued is a plain queued cancel; the part is removed because the job had `resume_at > 0` — make `cancel_where` collect parts for **any** non-running job with `resume_at > 0` (not only `Paused`), since a requeued-but-not-started job still owns its part.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p linascp-core transfer::queue`
Expected: the new tests fail where the engine has gaps (at least the `cancel_where` part rule above); everything else compiles.

- [ ] **Step 4: Fix the gaps** found in Step 3 (expected: the `cancel_where` condition becomes `entry.job.state != JobState::Running && entry.resume_at > 0`). Any other failure is a real engine bug: debug it, do not loosen the test.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p linascp-core transfer::`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/transfer
git commit -m "feat(core): pause transfers on connection loss and resume from the part

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: `SftpSession` as a transfer side

**Files:**
- Modify: `crates/core/src/connect/sftp_session.rs`, `crates/core/src/connect/mod.rs`
- Test: `crates/core/tests/ssh_integration.rs`

**Interfaces:**
- Consumes: `Side`, `Reader`, `Writer` (Task 1), `TransferQueue` (Tasks 3–5).
- Produces: `impl Side for SftpSession` (so `Arc<SftpSession>` coerces to `Arc<dyn Side>`); `SftpSession::start(raw, version, posix_rename: bool, handle, jump, info, on_lost)`; `pub fn posix_rename(&self) -> bool`.

- [ ] **Step 1: Write the failing Docker tests** (append to `crates/core/tests/ssh_integration.rs`)

```rust
mod transfers {
    use super::*;
    use linascp_core::transfer::{Direction, JobState, LocalSide, Side, TransferEvent, TransferOptions, TransferQueue};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    pub(super) fn recorder() -> (Arc<Mutex<Vec<TransferEvent>>>, linascp_core::transfer::TransferEmit) {
        let events: Arc<Mutex<Vec<TransferEvent>>> = Arc::default();
        let emit = {
            let events = events.clone();
            Arc::new(move |e| events.lock().unwrap().push(e)) as linascp_core::transfer::TransferEmit
        };
        (events, emit)
    }

    pub(super) async fn wait_batch_done(events: &Arc<Mutex<Vec<TransferEvent>>>, count: usize, secs: u64) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        loop {
            let done = events.lock().unwrap().iter().filter(|e| matches!(e, TransferEvent::BatchDone { .. })).count();
            if done >= count {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "no BatchDone: {:#?}", events.lock().unwrap());
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    pub(super) fn states(events: &Arc<Mutex<Vec<TransferEvent>>>) -> Vec<(String, JobState)> {
        let mut last = std::collections::BTreeMap::new();
        for e in events.lock().unwrap().iter() {
            if let TransferEvent::Job { job } = e {
                last.insert(job.rel.clone(), job.state);
            }
        }
        last.into_iter().collect()
    }

    #[tokio::test]
    #[ignore = "requires docker"]
    async fn upload_and_download_a_tree_with_modes_and_mtime() {
        let c = Container::start(None);
        let env = Env::new();
        let h = host(c.port, "pwuser", AuthMethod::Password, None);
        env.store_password(&h, "pwpass");
        let (session, home, _) = sftp(&env, h, no_lost()).await;
        let session = Arc::new(session);
        let local = tempfile::tempdir().unwrap();
        let tree = local.path().join("tree");
        std::fs::create_dir_all(tree.join("sub")).unwrap();
        let big: Vec<u8> = (0..(5 * 1024 * 1024 + 3)).map(|i| (i % 241) as u8).collect();
        std::fs::write(tree.join("big.bin"), &big).unwrap();
        std::fs::write(tree.join("sub/key"), b"secret").unwrap();
        std::fs::set_permissions(tree.join("sub/key"), std::fs::Permissions::from_mode(0o600)).unwrap();
        let (events, emit) = recorder();
        let queue = TransferQueue::new(Arc::new(LocalSide), TransferOptions { parallel: 2, upload_mode: Some(0o644) }, emit);
        queue.set_remote(Some(session.clone()));
        queue.enqueue(Direction::Upload, vec![tree.to_str().unwrap().into()], home.path.clone()).await.unwrap();
        wait_batch_done(&events, 1, 60).await;
        assert!(states(&events).iter().all(|(_, s)| *s == JobState::Done), "{:?}", states(&events));
        let remote_tree = format!("{}/tree", home.path);
        let node = session.stat(&format!("{remote_tree}/sub/key")).await.unwrap().unwrap();
        assert_eq!(node.mode & 0o777, 0o644);
        assert_eq!(session.stat(&remote_tree).await.unwrap().unwrap().mode & 0o777, 0o755);
        assert!(session.stat(&format!("{remote_tree}/big.bin.filepart")).await.unwrap().is_none());

        let back = tempfile::tempdir().unwrap();
        queue.enqueue(Direction::Download, vec![remote_tree.clone()], back.path().to_str().unwrap().into()).await.unwrap();
        wait_batch_done(&events, 2, 60).await;
        assert_eq!(std::fs::read(back.path().join("tree/big.bin")).unwrap(), big);
        let local_mtime = std::fs::metadata(tree.join("big.bin")).unwrap().modified().unwrap();
        let back_mtime = std::fs::metadata(back.path().join("tree/big.bin")).unwrap().modified().unwrap();
        assert_eq!(
            local_mtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            back_mtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
        );
        assert_eq!(std::fs::metadata(back.path().join("tree/sub/key")).unwrap().permissions().mode() & 0o777, 0o644);
        queue.close().await;
        session.close().await;
    }

    #[tokio::test]
    #[ignore = "requires docker"]
    async fn overwrite_replaces_an_existing_remote_file() {
        use linascp_core::transfer::Decision;
        let c = Container::start(None);
        let env = Env::new();
        let h = host(c.port, "pwuser", AuthMethod::Password, None);
        env.store_password(&h, "pwpass");
        let (session, home, _) = sftp(&env, h, no_lost()).await;
        assert!(session.posix_rename(), "OpenSSH offers posix-rename@openssh.com");
        let session = Arc::new(session);
        let local = tempfile::tempdir().unwrap();
        std::fs::write(local.path().join("f"), b"first").unwrap();
        let (events, emit) = recorder();
        let queue = TransferQueue::new(Arc::new(LocalSide), TransferOptions { parallel: 1, upload_mode: None }, emit);
        queue.set_remote(Some(session.clone()));
        let src = vec![local.path().join("f").to_str().unwrap().to_string()];
        queue.enqueue(Direction::Upload, src.clone(), home.path.clone()).await.unwrap();
        wait_batch_done(&events, 1, 30).await;
        std::fs::write(local.path().join("f"), b"second!").unwrap();
        queue.enqueue(Direction::Upload, src, home.path.clone()).await.unwrap();
        let job_id = loop {
            let id = events.lock().unwrap().iter().find_map(|e| match e {
                TransferEvent::Conflict { job_id, .. } => Some(*job_id),
                _ => None,
            });
            if let Some(id) = id {
                break id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        queue.resolve(job_id, Decision::Overwrite, false).unwrap();
        wait_batch_done(&events, 2, 30).await;
        let node = session.stat(&format!("{}/f", home.path)).await.unwrap().unwrap();
        assert_eq!(node.size, 7);
        queue.close().await;
        session.close().await;
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p linascp-core --test ssh_integration transfers -- --ignored --test-threads=1`
Expected: FAIL to compile (`SftpSession: Side` not implemented, `posix_rename` missing).

- [ ] **Step 3: Implement**

In `crates/core/src/connect/mod.rs` → `open_sftp`: return the extension flag with the version and pass it on:

```rust
                let version = raw.init().await.map_err(|e| sftp_session::map_sftp_error(e, ""))?;
                self.log(format!("debug1: SFTP protocol version {}", version.version));
                let posix_rename = version.extensions.contains_key("posix-rename@openssh.com");
                Ok((raw, version.version, posix_rename))
            })
            .await;
        let (raw, version, posix_rename) = match opened {
```

and `SftpSession::start(raw, version, posix_rename, handle, jump, info, on_lost)`.

In `crates/core/src/connect/sftp_session.rs`:

1. `raw: Arc<RawSftpSession>` (wrap in `start`: `raw: Arc::new(raw)`), add field `posix_rename: bool` and `pub fn posix_rename(&self) -> bool`.
2. Replace the method `check` by a free function used by the session and the file handles:

```rust
/// Maps the error and reports a lost connection (once) on any transport failure: a request
/// timeout (frozen link without keepalive) or a dead SFTP channel leaves the SSH handle open,
/// but the session is unusable either way; reconnecting is cheap.
fn checked<T>(lost: &Lost, result: std::result::Result<T, SftpError>, path: &str) -> Result<T> {
    result.map_err(|e| {
        let error = map_sftp_error(e, path);
        if matches!(error, CoreError::Network { .. }) {
            lost.fire(error.clone());
        }
        error
    })
}
```

and `fn check<T>(&self, result, path) -> Result<T> { checked(&self.lost, result, path) }`.

3. Add the side implementation:

```rust
/// An open remote file (read or write) of a transfer.
struct RemoteFile {
    raw: Arc<RawSftpSession>,
    lost: Arc<Lost>,
    handle: String,
    path: String,
}

#[async_trait::async_trait]
impl Reader for RemoteFile {
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(len);
        // A server may answer short before the end: keep reading until `len` or EOF.
        while out.len() < len {
            let want = (len - out.len()) as u32;
            match self.raw.read(self.handle.as_str(), offset + out.len() as u64, want).await {
                Ok(data) if data.data.is_empty() => break,
                Ok(data) => out.extend_from_slice(&data.data),
                Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break,
                Err(e) => return checked(&self.lost, Err(e), &self.path),
            }
        }
        Ok(out)
    }

    async fn close(&self) {
        let _ = self.raw.close(self.handle.as_str()).await;
    }
}

#[async_trait::async_trait]
impl Writer for RemoteFile {
    async fn write_at(&self, offset: u64, data: Vec<u8>) -> Result<()> {
        checked(&self.lost, self.raw.write(self.handle.as_str(), offset, data).await, &self.path).map(|_| ())
    }

    async fn close(&self) -> Result<()> {
        checked(&self.lost, self.raw.close(self.handle.as_str()).await, &self.path).map(|_| ())
    }
}

fn node_of(attrs: &FileAttributes, kind: NodeKind) -> Node {
    Node { kind, size: attrs.size.unwrap_or(0), mtime: attrs.mtime.map(i64::from), mode: attrs.permissions.unwrap_or(0) }
}

/// SSH wire string: u32 length + bytes.
fn ssh_string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

#[async_trait::async_trait]
impl Side for SftpSession {
    async fn stat(&self, path: &str) -> Result<Option<Node>> {
        self.ensure_alive()?;
        let attrs = match self.raw.lstat(path).await {
            Ok(a) => a.attrs,
            Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => return Ok(None),
            Err(e) => return self.check(Err(e), path),
        };
        if !attrs.is_symlink() {
            let kind = if attrs.is_dir() { NodeKind::Dir } else if attrs.is_regular() { NodeKind::File } else { NodeKind::Other };
            return Ok(Some(node_of(&attrs, kind)));
        }
        Ok(Some(match self.raw.stat(path).await {
            Ok(t) if t.attrs.is_dir() => node_of(&t.attrs, NodeKind::DirLink),
            Ok(t) if t.attrs.is_regular() => node_of(&t.attrs, NodeKind::File),
            Ok(t) => node_of(&t.attrs, NodeKind::Other),
            Err(SftpError::Status(_)) => node_of(&attrs, NodeKind::BrokenLink),
            Err(e) => return self.check(Err(e), path),
        }))
    }

    async fn children(&self, path: &str) -> Result<Vec<Child>> {
        self.ensure_alive()?;
        Ok(self.list_names(path).await?.into_iter().map(|name| Child { name, lossy: false }).collect())
    }

    async fn mkdir(&self, path: &str) -> Result<()> {
        self.ensure_alive()?;
        match self.raw.mkdir(path, FileAttributes::default()).await {
            Ok(_) => Ok(()),
            Err(SftpError::Status(s)) if s.status_code == StatusCode::Failure && self.exists(path).await => {
                Err(CoreError::fs(FsKind::AlreadyExists, path, s.error_message))
            }
            Err(e) => self.check(Err(e), path),
        }
    }

    async fn open_read(&self, path: &str) -> Result<Box<dyn Reader>> {
        self.ensure_alive()?;
        let handle = self.check(self.raw.open(path, OpenFlags::READ, FileAttributes::default()).await, path)?.handle;
        Ok(Box::new(RemoteFile { raw: self.raw.clone(), lost: self.lost.clone(), handle, path: path.to_string() }))
    }

    async fn open_write(&self, path: &str, resume: bool) -> Result<Box<dyn Writer>> {
        self.ensure_alive()?;
        let mut flags = OpenFlags::WRITE | OpenFlags::CREATE;
        if !resume {
            flags |= OpenFlags::TRUNCATE;
        }
        let attrs = FileAttributes { permissions: Some(0o600), ..FileAttributes::default() };
        let handle = self.check(self.raw.open(path, flags, attrs).await, path)?.handle;
        Ok(Box::new(RemoteFile { raw: self.raw.clone(), lost: self.lost.clone(), handle, path: path.to_string() }))
    }

    async fn set_mode(&self, path: &str, mode: u32) -> Result<()> {
        let attrs = FileAttributes { permissions: Some(mode), ..FileAttributes::default() };
        self.check(self.raw.setstat(path, attrs).await, path).map(|_| ())
    }

    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()> {
        // SFTP v3 sets atime and mtime together.
        let t = mtime.clamp(0, i64::from(u32::MAX)) as u32;
        let attrs = FileAttributes { atime: Some(t), mtime: Some(t), ..FileAttributes::default() };
        self.check(self.raw.setstat(path, attrs).await, path).map(|_| ())
    }

    async fn rename_over(&self, from: &str, to: &str) -> Result<()> {
        self.ensure_alive()?;
        if self.posix_rename {
            let mut data = Vec::new();
            ssh_string(&mut data, from);
            ssh_string(&mut data, to);
            return match self.raw.extended("posix-rename@openssh.com", data).await {
                Ok(Packet::Status(s)) if s.status_code == StatusCode::Ok => Ok(()),
                Ok(Packet::Status(s)) => Err(map_sftp_error(SftpError::Status(s), to)),
                Ok(_) => Err(CoreError::internal("unexpected reply to posix-rename")),
                Err(e) => self.check(Err(e), to),
            };
        }
        // ponytail: not atomic without the extension; a crash between both steps leaves only the part.
        match self.raw.remove(to).await {
            Ok(_) => {}
            Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => {}
            Err(e) => return self.check(Err(e), to),
        }
        self.check(self.raw.rename(from, to).await, from).map(|_| ())
    }

    async fn remove(&self, path: &str) -> Result<()> {
        self.check(self.raw.remove(path).await, path).map(|_| ())
    }
}
```

Imports to add: `use russh_sftp::protocol::{OpenFlags, Packet};` and `use crate::transfer::{Child, Node, NodeKind, Reader, Side, Writer};`. If `OpenFlags` does not implement `|=`, write `flags = flags | OpenFlags::TRUNCATE;`. If `Packet::Status` is reached through a different path in russh-sftp 3.0.1, check `src/protocol/mod.rs`.

- [ ] **Step 4: Run all core tests and the Docker tests**

Run: `cargo test -p linascp-core && cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`
Expected: PASS, Docker suite 25/25.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): SFTP session as a transfer side with posix-rename

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: `Reconnecting<T>` for SFTP

**Files:**
- Create: `crates/core/src/connect/sftp_reconnect.rs`
- Modify: `crates/core/src/connect/mod.rs` (module + exports), `crates/core/src/connect/sftp_session.rs` (`impl Connection`)
- Test: in-module unit tests; `crates/core/tests/ssh_integration.rs` (reconnect + resume)

**Interfaces:**
- Consumes: `BACKOFF_SECS`, `MAX_ATTEMPTS`, `is_fatal`, `Enabled` (`connect/reconnect.rs`), `LostFn`, `SessionInfo`.
- Produces:
  - `#[async_trait] pub trait Connection: Send + Sync + 'static { fn info(&self) -> SessionInfo; async fn close(&self); }`
  - `pub type ConnFuture<T> = Pin<Box<dyn Future<Output = Result<Arc<T>>> + Send>>;`
  - `pub type ConnConnector<T> = Arc<dyn Fn(LostFn, CancellationToken) -> ConnFuture<T> + Send + Sync>;`
  - `pub enum ConnStatus { Reconnecting { attempt: u32, max_attempts: u32, retry_in: Duration, reason: CoreError }, Reconnected { info: SessionInfo }, Lost { error: CoreError } }`
  - `pub type StatusFn = Arc<dyn Fn(ConnStatus) + Send + Sync>; pub type ChangeFn<T> = Arc<dyn Fn(Option<Arc<T>>) + Send + Sync>;`
  - `pub struct Reconnecting<T: Connection>` with `new(connector, enabled, status, change)`, `begin(&self) -> (u64, LostFn)`, `attach(&self, generation: u64, conn: Arc<T>)`, `abandon(&self, generation: u64, error: CoreError)`, `current(&self) -> Result<Arc<T>>`, `reconnect_now(&self)`, `stop(&self)`, `async close(&self)`.

Rules (spec §1 "Wiederverbinden"):
- `begin` cancels a running round, drops the current connection (`change(None)`, closed in the background), bumps the generation and returns a `LostFn` bound to it. Used for the first connect and for the interactive "Neu verbinden".
- `attach(gen, conn)` for a stale generation closes `conn`. Otherwise: `Connected`, `change(Some(conn))`, and `Reconnected { info }` unless this is the very first connection. A `Lost` that arrived for this generation during the attempt makes it lose right away (round or `Lost`).
- `abandon(gen, error)`: an interactive attempt failed → `Lost { error }` (if `gen` is current).
- A `Lost` of the connected generation: `change(None)`, old closed in the background; if `enabled()` → round, else `Lost { reason }`.
- Round = TP2d: for attempt n: `Reconnecting { attempt: n, max_attempts: 8, retry_in }` (first wait `BACKOFF_SECS[0]`), wait (or `reconnect_now`), call the connector with a fresh `LostFn`. Fatal error → `Lost`; all 8 failed → `Lost` with the last error.
- `reconnect_now`: while waiting → skip the wait (a second `Reconnecting` with `retry_in = 0`); after a non-fatal `Lost` → new round without the first wait.
- `stop`: during a round → cancel → `Lost { last reason }`.
- `close`: everything stops, `change(None)`, connection closed, no status afterwards.

- [ ] **Step 1: Write the failing unit tests** (bottom of `sftp_reconnect.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct Fake {
        id: usize,
        closed: AtomicBool,
    }

    #[async_trait::async_trait]
    impl Connection for Fake {
        fn info(&self) -> SessionInfo {
            SessionInfo::default()
        }
        async fn close(&self) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    #[derive(Default)]
    struct Script {
        results: Mutex<VecDeque<Result<()>>>,
        calls: AtomicUsize,
        losts: Mutex<Vec<LostFn>>,
    }

    struct Rig {
        rc: Reconnecting<Fake>,
        script: Arc<Script>,
        statuses: Arc<Mutex<Vec<ConnStatus>>>,
        changes: Arc<Mutex<Vec<Option<usize>>>>,
        enabled: Arc<AtomicBool>,
    }

    fn rig() -> Rig {
        let script = Arc::new(Script::default());
        let connector: ConnConnector<Fake> = {
            let script = script.clone();
            Arc::new(move |lost, _cancel| {
                let script = script.clone();
                Box::pin(async move {
                    let n = script.calls.fetch_add(1, Ordering::SeqCst) + 1;
                    script.losts.lock().unwrap().push(lost);
                    let next = script.results.lock().unwrap().pop_front().unwrap_or(Ok(()));
                    next.map(|()| Arc::new(Fake { id: 100 + n, closed: AtomicBool::new(false) }))
                })
            })
        };
        let statuses: Arc<Mutex<Vec<ConnStatus>>> = Arc::default();
        let changes: Arc<Mutex<Vec<Option<usize>>>> = Arc::default();
        let enabled = Arc::new(AtomicBool::new(true));
        let rc = Reconnecting::new(
            connector,
            { let e = enabled.clone(); Arc::new(move || e.load(Ordering::SeqCst)) },
            { let s = statuses.clone(); Arc::new(move |st| s.lock().unwrap().push(st)) },
            { let c = changes.clone(); Arc::new(move |conn: Option<Arc<Fake>>| c.lock().unwrap().push(conn.map(|c| c.id))) },
        );
        Rig { rc, script, statuses, changes, enabled }
    }

    fn net() -> CoreError {
        network_error(NetworkKind::Other, "boom")
    }

    /// First connection (id 1); returns its LostFn.
    fn connect_first(r: &Rig) -> (Arc<Fake>, LostFn) {
        let (generation, lost) = r.rc.begin();
        let conn = Arc::new(Fake { id: 1, closed: AtomicBool::new(false) });
        r.rc.attach(generation, conn.clone());
        (conn, lost)
    }

    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn first_attach_is_silent_and_current_returns_it() {
        let r = rig();
        let (conn, _) = connect_first(&r);
        assert!(r.statuses.lock().unwrap().is_empty());
        assert_eq!(*r.changes.lock().unwrap(), vec![Some(1)]);
        assert_eq!(r.rc.current().unwrap().id, conn.id);
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_connection_is_restored_with_backoff() {
        let r = rig();
        let (old, lost) = connect_first(&r);
        lost(net());
        settle().await;
        assert!(old.closed.load(Ordering::SeqCst));
        assert!(r.rc.current().is_err());
        assert!(matches!(r.statuses.lock().unwrap()[0], ConnStatus::Reconnecting { attempt: 1, max_attempts: 8, .. }));
        tokio::time::sleep(Duration::from_secs(2)).await;
        settle().await;
        assert!(matches!(r.statuses.lock().unwrap().last(), Some(ConnStatus::Reconnected { .. })));
        assert_eq!(*r.changes.lock().unwrap(), vec![Some(1), None, Some(101)]);
        assert_eq!(r.rc.current().unwrap().id, 101);
    }

    #[tokio::test(start_paused = true)]
    async fn the_old_generations_lost_is_ignored() {
        let r = rig();
        let (_, lost) = connect_first(&r);
        lost(net());
        tokio::time::sleep(Duration::from_secs(2)).await;
        settle().await;
        lost(net());
        settle().await;
        assert_eq!(r.rc.current().unwrap().id, 101);
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_reports_lost_at_once() {
        let r = rig();
        r.enabled.store(false, Ordering::SeqCst);
        let (_, lost) = connect_first(&r);
        lost(net());
        settle().await;
        assert!(matches!(r.statuses.lock().unwrap().as_slice(), [ConnStatus::Lost { .. }]));
        assert_eq!(r.script.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_fatal_attempt_gives_up_and_reconnect_now_does_not_retry_it() {
        let r = rig();
        r.script.results.lock().unwrap().push_back(Err(CoreError::InputRequired));
        let (_, lost) = connect_first(&r);
        lost(net());
        tokio::time::sleep(Duration::from_secs(2)).await;
        settle().await;
        assert!(matches!(r.statuses.lock().unwrap().last(), Some(ConnStatus::Lost { error: CoreError::InputRequired })));
        r.rc.reconnect_now();
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert_eq!(r.script.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn eight_failures_give_up_with_the_last_error() {
        let r = rig();
        for _ in 0..8 {
            r.script.results.lock().unwrap().push_back(Err(net()));
        }
        let (_, lost) = connect_first(&r);
        lost(net());
        tokio::time::sleep(Duration::from_secs(400)).await;
        settle().await;
        assert_eq!(r.script.calls.load(Ordering::SeqCst), 8);
        assert!(matches!(r.statuses.lock().unwrap().last(), Some(ConnStatus::Lost { error: CoreError::Network { .. } })));
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_now_skips_the_wait_and_restarts_after_lost() {
        let r = rig();
        r.enabled.store(false, Ordering::SeqCst);
        let (_, lost) = connect_first(&r);
        lost(net());
        settle().await;
        r.enabled.store(true, Ordering::SeqCst);
        r.rc.reconnect_now();
        settle().await;
        assert_eq!(r.script.calls.load(Ordering::SeqCst), 1, "no first wait after Lost");
        assert_eq!(r.rc.current().unwrap().id, 101);
    }

    #[tokio::test(start_paused = true)]
    async fn stop_ends_the_round_with_lost() {
        let r = rig();
        let (_, lost) = connect_first(&r);
        lost(net());
        settle().await;
        r.rc.stop();
        settle().await;
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert_eq!(r.script.calls.load(Ordering::SeqCst), 0);
        assert!(matches!(r.statuses.lock().unwrap().last(), Some(ConnStatus::Lost { .. })));
    }

    #[tokio::test(start_paused = true)]
    async fn an_interactive_reattach_after_lost_reports_reconnected() {
        let r = rig();
        r.enabled.store(false, Ordering::SeqCst);
        let (_, lost) = connect_first(&r);
        lost(net());
        settle().await;
        let (generation, _) = r.rc.begin();
        r.rc.attach(generation, Arc::new(Fake { id: 7, closed: AtomicBool::new(false) }));
        assert!(matches!(r.statuses.lock().unwrap().last(), Some(ConnStatus::Reconnected { .. })));
        assert_eq!(r.rc.current().unwrap().id, 7);
        let (generation, _) = r.rc.begin();
        r.rc.abandon(generation, CoreError::Cancelled);
        assert!(matches!(r.statuses.lock().unwrap().last(), Some(ConnStatus::Lost { error: CoreError::Cancelled })));
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_during_the_first_connect_counts_after_attach() {
        let r = rig();
        let (generation, lost) = r.rc.begin();
        lost(net());
        r.rc.attach(generation, Arc::new(Fake { id: 1, closed: AtomicBool::new(false) }));
        settle().await;
        assert!(matches!(r.statuses.lock().unwrap()[0], ConnStatus::Reconnecting { attempt: 1, .. }));
    }

    #[tokio::test(start_paused = true)]
    async fn close_is_final() {
        let r = rig();
        let (conn, lost) = connect_first(&r);
        r.rc.close().await;
        assert!(conn.closed.load(Ordering::SeqCst));
        lost(net());
        r.rc.reconnect_now();
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert!(r.statuses.lock().unwrap().is_empty());
        assert_eq!(*r.changes.lock().unwrap(), vec![Some(1), None]);
        let stale = Arc::new(Fake { id: 9, closed: AtomicBool::new(false) });
        r.rc.attach(99, stale.clone());
        assert!(stale.closed.load(Ordering::SeqCst));
    }
}
```

`SessionInfo::default()`: if `SessionInfo` has no `Default`, add `#[derive(Default)]` to it in `connect/mod.rs` (all fields are strings/options).

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p linascp-core sftp_reconnect`
Expected: FAIL to compile.

- [ ] **Step 3: Implement** `crates/core/src/connect/sftp_reconnect.rs`

```rust
//! Keeps an SFTP connection alive (TP3b spec §1 "Wiederverbinden"): same rules as the terminal
//! reconnect of TP2d, but the payload is just "the current connection", handed to `change`.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::reconnect::{BACKOFF_SECS, Enabled, MAX_ATTEMPTS, is_fatal};
use super::{LostFn, SessionInfo, network_error};
use crate::error::{CoreError, NetworkKind, Result};

#[async_trait::async_trait]
pub trait Connection: Send + Sync + 'static {
    fn info(&self) -> SessionInfo;
    async fn close(&self);
}

pub type ConnFuture<T> = Pin<Box<dyn Future<Output = Result<Arc<T>>> + Send>>;
/// Opens a new connection without prompting; `LostFn` must be wired into it.
pub type ConnConnector<T> = Arc<dyn Fn(LostFn, CancellationToken) -> ConnFuture<T> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnStatus {
    Reconnecting { attempt: u32, max_attempts: u32, retry_in: Duration, reason: CoreError },
    Reconnected { info: SessionInfo },
    Lost { error: CoreError },
}

pub type StatusFn = Arc<dyn Fn(ConnStatus) + Send + Sync>;
pub type ChangeFn<T> = Arc<dyn Fn(Option<Arc<T>>) + Send + Sync>;

enum Phase<T> {
    /// A connection of the current generation is being set up.
    Attempting { lost: Option<CoreError> },
    Connected(Arc<T>),
    /// Between attempts of a round.
    Waiting,
    Lost,
    Closed,
}

struct St<T> {
    phase: Phase<T>,
    generation: u64,
    round: CancellationToken,
    /// The reason reported by `stop` and by a round that runs out of attempts.
    reason: CoreError,
    ever_connected: bool,
}

struct Shared<T: Connection> {
    st: Mutex<St<T>>,
    connector: ConnConnector<T>,
    enabled: Enabled,
    status: StatusFn,
    change: ChangeFn<T>,
    now: Notify,
}

pub struct Reconnecting<T: Connection> {
    shared: Arc<Shared<T>>,
}

fn not_connected() -> CoreError {
    network_error(NetworkKind::Other, "not connected")
}

fn close_in_background<T: Connection>(conn: Arc<T>) {
    tokio::spawn(async move { conn.close().await });
}

impl<T: Connection> Reconnecting<T> {
    pub fn new(connector: ConnConnector<T>, enabled: Enabled, status: StatusFn, change: ChangeFn<T>) -> Self {
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let st = St {
            phase: Phase::Lost,
            generation: 0,
            round: cancelled,
            reason: not_connected(),
            ever_connected: false,
        };
        Self {
            shared: Arc::new(Shared { st: Mutex::new(st), connector, enabled, status, change, now: Notify::new() }),
        }
    }

    /// Starts an interactive connect: returns its generation and the `LostFn` to wire into it.
    pub fn begin(&self) -> (u64, LostFn) {
        let mut st = self.shared.lock();
        st.round.cancel();
        st.generation += 1;
        if let Phase::Connected(old) = std::mem::replace(&mut st.phase, Phase::Attempting { lost: None }) {
            (self.shared.change)(None);
            close_in_background(old);
        }
        if matches!(st.phase, Phase::Closed) {
            // unreachable in practice: callers never begin after close.
        }
        (st.generation, lost_fn(Arc::downgrade(&self.shared), st.generation))
    }

    pub fn attach(&self, generation: u64, conn: Arc<T>) {
        if let Attach::LostMeanwhile(reason) = self.shared.attach(generation, conn) {
            let mut st = self.shared.lock();
            self.shared.lose(&mut st, reason);
        }
    }

    pub fn abandon(&self, generation: u64, error: CoreError) {
        let mut st = self.shared.lock();
        if st.generation == generation && matches!(st.phase, Phase::Attempting { .. }) {
            st.phase = Phase::Lost;
            st.reason = error.clone();
            (self.shared.status)(ConnStatus::Lost { error });
        }
    }

    pub fn current(&self) -> Result<Arc<T>> {
        match &self.shared.lock().phase {
            Phase::Connected(conn) => Ok(conn.clone()),
            _ => Err(not_connected()),
        }
    }

    pub fn reconnect_now(&self) {
        let mut st = self.shared.lock();
        match st.phase {
            Phase::Waiting => self.shared.now.notify_one(),
            Phase::Lost if st.ever_connected && !is_fatal(&st.reason) => {
                let reason = st.reason.clone();
                self.shared.start_round(&mut st, reason, false);
            }
            _ => {}
        }
    }

    pub fn stop(&self) {
        let mut st = self.shared.lock();
        let in_round = !st.round.is_cancelled() && matches!(st.phase, Phase::Waiting | Phase::Attempting { .. });
        if in_round {
            st.round.cancel();
            st.generation += 1;
            st.phase = Phase::Lost;
            (self.shared.status)(ConnStatus::Lost { error: st.reason.clone() });
        }
    }

    pub async fn close(&self) {
        let old = {
            let mut st = self.shared.lock();
            st.round.cancel();
            st.generation += 1;
            match std::mem::replace(&mut st.phase, Phase::Closed) {
                Phase::Connected(conn) => {
                    (self.shared.change)(None);
                    Some(conn)
                }
                _ => None,
            }
        };
        if let Some(conn) = old {
            conn.close().await;
        }
    }
}

enum Attach {
    Done,
    LostMeanwhile(CoreError),
}

fn lost_fn<T: Connection>(shared: Weak<Shared<T>>, generation: u64) -> LostFn {
    Arc::new(move |reason| {
        if let Some(shared) = shared.upgrade() {
            shared.on_lost(generation, reason);
        }
    })
}

impl<T: Connection> Shared<T> {
    fn lock(&self) -> MutexGuard<'_, St<T>> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn on_lost(self: &Arc<Self>, generation: u64, reason: CoreError) {
        let mut st = self.lock();
        if st.generation != generation {
            return;
        }
        match &mut st.phase {
            Phase::Attempting { lost } => *lost = Some(reason),
            Phase::Connected(_) => self.lose(&mut st, reason),
            _ => {}
        }
    }

    /// The connection broke: start a round, or report `Lost` when auto-reconnect is off.
    fn lose(self: &Arc<Self>, st: &mut St<T>, reason: CoreError) {
        st.generation += 1;
        if let Phase::Connected(old) = std::mem::replace(&mut st.phase, Phase::Waiting) {
            (self.change)(None);
            close_in_background(old);
        }
        st.reason = reason.clone();
        if (self.enabled)() {
            self.start_round(st, reason, true);
        } else {
            st.phase = Phase::Lost;
            (self.status)(ConnStatus::Lost { error: reason });
        }
    }

    fn start_round(self: &Arc<Self>, st: &mut St<T>, reason: CoreError, wait_first: bool) {
        st.phase = Phase::Waiting;
        st.round = CancellationToken::new();
        tokio::spawn(run_round(self.clone(), st.round.clone(), reason, wait_first));
    }

    fn attach(self: &Arc<Self>, generation: u64, conn: Arc<T>) -> Attach {
        let mut st = self.lock();
        if st.generation != generation || !matches!(st.phase, Phase::Attempting { .. }) {
            drop(st);
            close_in_background(conn);
            return Attach::Done;
        }
        let Phase::Attempting { lost } = std::mem::replace(&mut st.phase, Phase::Connected(conn.clone())) else {
            unreachable!("checked above");
        };
        if let Some(reason) = lost {
            st.phase = Phase::Waiting;
            drop(st);
            close_in_background(conn);
            return Attach::LostMeanwhile(reason);
        }
        (self.change)(Some(conn.clone()));
        if st.ever_connected {
            (self.status)(ConnStatus::Reconnected { info: conn.info() });
        }
        st.ever_connected = true;
        Attach::Done
    }

    fn give_up(&self, round: &CancellationToken, reason: CoreError) {
        let mut st = self.lock();
        if round.is_cancelled() {
            return;
        }
        st.phase = Phase::Lost;
        st.reason = reason.clone();
        (self.status)(ConnStatus::Lost { error: reason });
    }
}

/// Up to `MAX_ATTEMPTS` attempts, each after its backoff wait.
async fn run_round<T: Connection>(shared: Arc<Shared<T>>, round: CancellationToken, mut reason: CoreError, wait_first: bool) {
    for (index, secs) in BACKOFF_SECS.iter().enumerate() {
        let attempt = index as u32 + 1;
        let delay = if index == 0 && !wait_first { Duration::ZERO } else { Duration::from_secs(*secs) };
        {
            let _st = shared.lock();
            if round.is_cancelled() {
                return;
            }
            (shared.status)(ConnStatus::Reconnecting { attempt, max_attempts: MAX_ATTEMPTS, retry_in: delay, reason: reason.clone() });
        }
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = shared.now.notified() => {
                let _st = shared.lock();
                if round.is_cancelled() {
                    return;
                }
                (shared.status)(ConnStatus::Reconnecting { attempt, max_attempts: MAX_ATTEMPTS, retry_in: Duration::ZERO, reason: reason.clone() });
            }
            () = round.cancelled() => return,
        }
        let generation = {
            let mut st = shared.lock();
            if round.is_cancelled() {
                return;
            }
            st.generation += 1;
            st.phase = Phase::Attempting { lost: None };
            st.generation
        };
        let lost = lost_fn(Arc::downgrade(&shared), generation);
        let result = tokio::select! {
            result = (shared.connector)(lost, round.child_token()) => result,
            () = round.cancelled() => return,
        };
        match result {
            Ok(conn) => match shared.attach(generation, conn) {
                Attach::Done => return,
                Attach::LostMeanwhile(error) => reason = error,
            },
            Err(error) if is_fatal(&error) => {
                shared.give_up(&round, error);
                return;
            }
            Err(error) => reason = error,
        }
        let mut st = shared.lock();
        if round.is_cancelled() {
            return;
        }
        st.phase = Phase::Waiting;
        st.reason = reason.clone();
    }
    shared.give_up(&round, reason);
}
```

Remove the empty `if matches!(st.phase, Phase::Closed)` block from `begin` – it is a leftover; `begin` after `close` is not supported (Tauri removes the entry before closing). `reconnect_now` on `Phase::Lost` requires `ever_connected` so the initial placeholder `Lost` never starts a round.

In `crates/core/src/connect/mod.rs`:

```rust
pub mod sftp_reconnect;
pub use sftp_reconnect::{ChangeFn, ConnConnector, ConnFuture, ConnStatus, Connection, Reconnecting, StatusFn};
```

In `sftp_session.rs`:

```rust
#[async_trait::async_trait]
impl super::sftp_reconnect::Connection for SftpSession {
    fn info(&self) -> SessionInfo {
        self.info.clone()
    }
    async fn close(&self) {
        SftpSession::close(self).await;
    }
}
```

- [ ] **Step 4: Run the unit tests**

Run: `cargo test -p linascp-core sftp_reconnect`
Expected: PASS (11 tests).

- [ ] **Step 5: Write the Docker test for reconnect + resume** (inside `mod transfers` in `ssh_integration.rs`)

```rust
    #[tokio::test]
    #[ignore = "requires docker"]
    async fn a_frozen_link_reconnects_and_the_upload_finishes_identical() {
        use linascp_core::connect::{ConnConnector, ConnStatus, PromptBroker, Reconnecting, SftpSession};
        let c = Container::start(None);
        let env = Arc::new(Env::new());
        let mut h = host(c.port, "pwuser", AuthMethod::Password, None);
        h.keepalive_s = 1;
        h.timeout_s = 3;
        env.store_password(&h, "pwpass");
        // Trust the host key once (interactive), so the non-interactive attempts can connect.
        let (first, home, _) = sftp(&env, h.clone(), Arc::new(|_| {})).await;
        first.close().await;

        let (events, emit) = recorder();
        let queue = TransferQueue::new(Arc::new(LocalSide), TransferOptions { parallel: 1, upload_mode: None }, emit);
        let statuses: Arc<Mutex<Vec<ConnStatus>>> = Arc::default();
        let connector: ConnConnector<SftpSession> = {
            let (env, h) = (env.clone(), h.clone());
            Arc::new(move |lost, cancel| {
                let (env, h) = (env.clone(), h.clone());
                Box::pin(async move {
                    let broker = Arc::new(PromptBroker::non_interactive());
                    let (s, _) = connect_sftp(request(h, None), env.deps(None), broker, Arc::new(|_| {}), lost, cancel).await?;
                    Ok(Arc::new(s))
                })
            })
        };
        let rc = Reconnecting::new(
            connector.clone(),
            Arc::new(|| true),
            { let s = statuses.clone(); Arc::new(move |st| s.lock().unwrap().push(st)) },
            { let q = queue.clone(); Arc::new(move |conn: Option<Arc<SftpSession>>| q.set_remote(conn.map(|c| c as Arc<dyn Side>))) },
        );
        let (generation, lost) = rc.begin();
        let session = connector(lost, CancellationToken::new()).await.expect("connected");
        rc.attach(generation, session);

        let local = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..(64 * 1024 * 1024)).map(|i| (i % 249) as u8).collect();
        std::fs::write(local.path().join("big.bin"), &data).unwrap();
        queue.enqueue(Direction::Upload, vec![local.path().join("big.bin").to_str().unwrap().into()], home.path.clone()).await.unwrap();
        // Wait for some progress, then freeze the server for longer than timeout + keepalive.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !events.lock().unwrap().iter().any(|e| matches!(e, TransferEvent::Progress { done, .. } if *done > 4 * 1024 * 1024)) {
            assert!(std::time::Instant::now() < deadline, "no progress");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let pause = std::process::Command::new("docker").args(["pause", &c.id]).output().unwrap();
        assert!(pause.status.success(), "{pause:?}");
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        let _ = std::process::Command::new("docker").args(["unpause", &c.id]).output();
        wait_batch_done(&events, 1, 120).await;
        assert!(statuses.lock().unwrap().iter().any(|s| matches!(s, ConnStatus::Reconnected { .. })), "{:?}", statuses.lock().unwrap());
        assert!(events.lock().unwrap().iter().any(|e| matches!(e, TransferEvent::Job { job } if job.state == JobState::Paused)));
        assert_eq!(states(&events), vec![("big.bin".to_string(), JobState::Done)]);
        let back = tempfile::tempdir().unwrap();
        queue.enqueue(Direction::Download, vec![format!("{}/big.bin", home.path)], back.path().to_str().unwrap().into()).await.unwrap();
        wait_batch_done(&events, 2, 120).await;
        assert!(std::fs::read(back.path().join("big.bin")).unwrap() == data, "content differs after resume");
        queue.close().await;
        rc.close().await;
    }
```

- [ ] **Step 6: Run the Docker suite**

Run: `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`
Expected: PASS, 26/26. If the pause does not produce `Paused` (the upload finished before the freeze), lower the progress threshold rather than growing the file.

- [ ] **Step 7: Commit**

```bash
git add crates/core
git commit -m "feat(core): reconnecting SFTP connection that resumes transfers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Tauri – `SftpEntry`, events, transfer and reconnect commands, `autoReconnect`

**Files:**
- Modify: `src-tauri/src/commands/sftp.rs`, `src-tauri/src/commands/connect.rs`, `src-tauri/src/state.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/dto.rs`, `src-tauri/src/commands/settings.rs`
- Create: `src-tauri/src/commands/sftp_transfers.rs`
- Regenerate: `ui/src/ipc/bindings.ts`

**Interfaces:**
- Consumes: `TransferQueue`, `LocalSide`, `TransferOptions::from_settings`, `TransferEvent`, `Reconnecting`, `ConnStatus`, `ConnConnector`.
- Produces (bindings):
  - `SftpEvent = { kind: "reconnecting"; attempt: number; maxAttempts: number; retryInMs: number; reason: CoreError } | { kind: "reconnected"; info: SessionInfo } | { kind: "lost"; error: CoreError } | { kind: "transfer"; event: TransferEvent }`
  - `connectSftp(connectId, hostId, sessionId: string | null, events, sftpEvents)`
  - `transferEnqueue(sessionId, direction, sources, targetDir) -> number`, `transferResolve(sessionId, jobId, decision, applyToAll)`, `transferCancel(sessionId, target)`, `transferRetry(sessionId, jobId)`, `transferClear(sessionId)`, `sftpReconnectNow(sessionId)`, `sftpStopReconnect(sessionId)`
  - `Settings.autoReconnect` (was `terminalAutoReconnect`).

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/commands/sftp.rs` tests: replace `connect_sftp_for_a_missing_host_emits_exactly_one_failed` with the new signature and add:

```rust
    fn quiet_sftp() -> SftpEmit {
        Arc::new(|_| {})
    }

    #[tokio::test]
    async fn connect_sftp_for_a_missing_host_emits_exactly_one_failed() {
        let (_d, state) = setup();
        let events: Arc<StdMutex<Vec<ConnectEvent>>> = Arc::default();
        let emit: EmitFn = {
            let events = events.clone();
            Arc::new(move |e| events.lock().unwrap().push(e))
        };
        let result = connect_sftp_inner(&state, Uuid::new_v4(), Uuid::new_v4(), None, emit, quiet_sftp()).await;
        assert!(result.is_err());
        let events = events.lock().unwrap();
        assert_eq!(events.iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count(), 1);
    }

    #[tokio::test]
    async fn reattaching_to_an_unknown_session_fails_once() {
        let (_d, state) = setup();
        let host = crate::commands::hosts::save_host(&state, linascp_core::model::Host::new("h", "h.example")).unwrap();
        let events: Arc<StdMutex<Vec<ConnectEvent>>> = Arc::default();
        let emit: EmitFn = {
            let events = events.clone();
            Arc::new(move |e| events.lock().unwrap().push(e))
        };
        let result = connect_sftp_inner(&state, Uuid::new_v4(), host.id, Some(Uuid::new_v4()), emit, quiet_sftp()).await;
        assert!(result.is_err());
        assert_eq!(events.lock().unwrap().iter().filter(|e| matches!(e, ConnectEvent::Failed { .. })).count(), 1);
    }

    #[test]
    fn sftp_events_serialize_for_the_ui() {
        let event = SftpEvent::Reconnecting {
            attempt: 2,
            max_attempts: 8,
            retry_in_ms: 2000,
            reason: CoreError::Cancelled,
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "kind": "reconnecting", "attempt": 2, "maxAttempts": 8, "retryInMs": 2000, "reason": { "type": "cancelled" } })
        );
        let event = SftpEvent::Transfer { event: linascp_core::transfer::TransferEvent::Progress { job_id: 1, done: 2 } };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "kind": "transfer", "event": { "kind": "progress", "jobId": 1, "done": 2 } })
        );
    }
```

`src-tauri/src/commands/sftp_transfers.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use linascp_core::transfer::{CancelTarget, Decision, Direction};

    #[tokio::test]
    async fn unknown_sessions_are_errors_not_panics() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let id = uuid::Uuid::new_v4();
        assert!(enqueue(&state, id, Direction::Upload, vec!["/a".into()], "/b".into()).await.is_err());
        assert!(entry(&state, id).is_err());
        assert!(with_entry(&state, id, |e| e.queue.resolve(1, Decision::Skip, false)).is_err());
        assert!(with_entry(&state, id, |e| e.queue.cancel(CancelTarget::Job { id: 1 })).is_err());
    }
}
```

`src-tauri/src/dto.rs` test (adapt the existing JSON assertion at line ~153): `"autoReconnect": true` instead of `"terminalAutoReconnect": true`.

`src-tauri/src/commands/settings.rs` test (new):

```rust
    #[test]
    fn auto_reconnect_reads_the_old_key() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::state::AppState::for_tests(dir.path());
        state.store.set_setting(TERMINAL_AUTO_RECONNECT, "false").unwrap();
        assert!(!load_settings(&state.store).unwrap().auto_reconnect);
    }
```

(Use the same flag string format `flag_str(false)` produces; check `flag_str` in `settings.rs`.)

`src-tauri/src/lib.rs` `exports_bindings`: add

```rust
        assert!(ts.contains("transferEnqueue: (sessionId: string, direction: Direction, sources: string[], targetDir: string)"));
        assert!(ts.contains("export type TransferEvent ="));
        assert!(ts.contains("autoReconnect: boolean"));
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p linascp`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`commands/connect.rs`: make `find_host_in`, `build_request_in`, `connect_deps_in` `pub(crate)`.

`commands/sftp.rs` (replace the event type, the session lookup, connect and close):

```rust
use linascp_core::connect::{
    ConnConnector, ConnStatus, ConnectEvent, EmitFn, PromptBroker, Reconnecting, SessionInfo, SftpSession,
    connect_sftp as run_pipeline,
};
use linascp_core::transfer::{LocalSide, Side, TransferEvent, TransferOptions, TransferQueue};

use super::connect::{build_request, build_request_in, channel_emit, connect_deps, connect_deps_in, fail, find_host, find_host_in, register_connect};
use super::settings::auto_reconnect_enabled;
use crate::state::{AppState, ConnectEnv};

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SftpEvent {
    Reconnecting { attempt: u32, max_attempts: u32, retry_in_ms: u32, reason: CoreError },
    Reconnected { info: SessionInfo },
    /// Reconnecting gave up (or is off); `connect_sftp` with this session id re-attaches.
    Lost { error: CoreError },
    Transfer { event: TransferEvent },
}

pub(crate) type SftpEmit = Arc<dyn Fn(SftpEvent) + Send + Sync>;

/// One SFTP tab: the self-healing connection and its transfer queue.
pub struct SftpEntry {
    pub conn: Reconnecting<SftpSession>,
    pub queue: TransferQueue,
}

fn status_event(status: ConnStatus) -> SftpEvent {
    match status {
        ConnStatus::Reconnecting { attempt, max_attempts, retry_in, reason } => SftpEvent::Reconnecting {
            attempt,
            max_attempts,
            retry_in_ms: u32::try_from(retry_in.as_millis()).unwrap_or(u32::MAX),
            reason,
        },
        ConnStatus::Reconnected { info } => SftpEvent::Reconnected { info },
        ConnStatus::Lost { error } => SftpEvent::Lost { error },
    }
}

/// Reconnect attempts: pipeline without prompts; a refused prompt is `InputRequired`.
fn sftp_connector(env: ConnectEnv, host_id: Id) -> ConnConnector<SftpSession> {
    Arc::new(move |on_lost, cancel| {
        let env = env.clone();
        Box::pin(async move {
            let host = find_host_in(&env, host_id)?;
            let request = build_request_in(&env, host, 80, 24)?;
            let broker = Arc::new(PromptBroker::non_interactive());
            let quiet: EmitFn = Arc::new(|_| {});
            match run_pipeline(request, connect_deps_in(&env), broker.clone(), quiet, on_lost, cancel).await {
                Ok((session, _)) => Ok(Arc::new(session)),
                Err(_) if broker.refused_input() => Err(CoreError::InputRequired),
                Err(error) => Err(error),
            }
        })
    })
}

fn new_entry(state: &AppState, host: &linascp_core::model::Host, sftp_emit: SftpEmit) -> Arc<SftpEntry> {
    let queue = {
        let emit = sftp_emit.clone();
        TransferQueue::new(
            Arc::new(LocalSide),
            TransferOptions::from_settings(&host.sftp),
            Arc::new(move |event| emit(SftpEvent::Transfer { event })),
        )
    };
    let store = state.store.clone();
    let conn = Reconnecting::new(
        sftp_connector(state.connect_env(), host.id),
        Arc::new(move || auto_reconnect_enabled(&store)),
        Arc::new(move |status| sftp_emit(status_event(status))),
        {
            let queue = queue.clone();
            Arc::new(move |session: Option<Arc<SftpSession>>| queue.set_remote(session.map(|s| s as Arc<dyn Side>)))
        },
    );
    Arc::new(SftpEntry { conn, queue })
}

/// `existing`: re-attach a fresh, interactive connection to that tab's session (queue survives).
pub(crate) async fn connect_sftp_inner(
    state: &AppState,
    connect_id: Uuid,
    host_id: Id,
    existing: Option<SessionId>,
    emit: EmitFn,
    sftp_emit: SftpEmit,
) -> Result<SftpConnected> {
    let guard = register_connect(state, connect_id);
    let host = match find_host(state, host_id) {
        Ok(host) => host,
        Err(error) => return fail(&emit, error),
    };
    let (session_id, entry) = match existing {
        Some(id) => match lookup(state, id) {
            Ok(entry) => (id, entry),
            Err(error) => return fail(&emit, error),
        },
        None => (Uuid::new_v4(), new_entry(state, &host, sftp_emit)),
    };
    let request = match build_request(state, host, 80, 24) {
        Ok(request) => request,
        Err(error) => return fail(&emit, error),
    };
    if guard.is_cancelled() {
        return fail(&emit, CoreError::Cancelled);
    }
    let cancel = guard.token();
    let (generation, on_lost) = entry.conn.begin();
    let result =
        run_pipeline(request, connect_deps(state), state.broker.clone(), emit.clone(), on_lost, cancel.clone()).await;
    drop(guard);
    match result {
        Ok((session, _)) if cancel.is_cancelled() => {
            session.close().await;
            entry.conn.abandon(generation, CoreError::Cancelled);
            fail(&emit, CoreError::Cancelled)
        }
        Ok((session, listing)) => {
            let connected =
                SftpConnected { session_id, info: session.info().clone(), version: session.version(), listing };
            entry.conn.attach(generation, Arc::new(session));
            lock(&state.sftp).insert(session_id, entry);
            emit(ConnectEvent::Connected { session_id });
            Ok(connected)
        }
        Err(error) => {
            entry.conn.abandon(generation, error.clone());
            Err(error)
        }
    }
}

pub(crate) fn lookup(state: &AppState, id: SessionId) -> Result<Arc<SftpEntry>> {
    lock(&state.sftp)
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::Internal { message: format!("unknown sftp session {id}") })
}

fn session(state: &AppState, id: SessionId) -> Result<Arc<SftpSession>> {
    lookup(state, id)?.conn.current()
}
```

The tauri command `connect_sftp` gains `session_id: Option<SessionId>` (place it after `host_id`) and builds `sftp_emit` from the channel:

```rust
    let sftp_emit: SftpEmit = Arc::new(move |event| {
        if let Err(e) = sftp_events.send(event) {
            tracing::debug!(error = %e, "sftp event dropped: channel closed");
        }
    });
    connect_sftp_inner(&state, connect_id, host_id, session_id, channel_emit(events), sftp_emit).await
```

`sftp_close`:

```rust
pub async fn sftp_close(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    let removed = lock(&state.sftp).remove(&session_id);
    if let Some(entry) = removed {
        entry.queue.close().await;
        entry.conn.close().await;
    }
    Ok(())
}
```

`state.rs`: `pub sftp: Arc<Mutex<HashMap<SessionId, Arc<crate::commands::sftp::SftpEntry>>>>` and drop the now unused `SftpSession` import.

`commands/sftp_transfers.rs`:

```rust
//! Transfer queue and reconnect commands of an SFTP tab (TP3b).

use std::sync::Arc;

use linascp_core::error::Result;
use linascp_core::session::SessionId;
use linascp_core::transfer::{BatchId, CancelTarget, Decision, Direction, JobId};
use tauri::State;

use super::sftp::{SftpEntry, lookup};
use crate::state::AppState;

pub(crate) fn entry(state: &AppState, id: SessionId) -> Result<Arc<SftpEntry>> {
    lookup(state, id)
}

pub(crate) fn with_entry<T>(state: &AppState, id: SessionId, f: impl FnOnce(&SftpEntry) -> Result<T>) -> Result<T> {
    f(&entry(state, id)?)
}

pub(crate) async fn enqueue(
    state: &AppState,
    id: SessionId,
    direction: Direction,
    sources: Vec<String>,
    target_dir: String,
) -> Result<BatchId> {
    let entry = entry(state, id)?;
    entry.queue.enqueue(direction, sources, target_dir).await
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_enqueue(
    state: State<'_, AppState>,
    session_id: SessionId,
    direction: Direction,
    sources: Vec<String>,
    target_dir: String,
) -> Result<BatchId> {
    enqueue(&state, session_id, direction, sources, target_dir).await
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_resolve(
    state: State<'_, AppState>,
    session_id: SessionId,
    job_id: JobId,
    decision: Decision,
    apply_to_all: bool,
) -> Result<()> {
    with_entry(&state, session_id, |e| e.queue.resolve(job_id, decision, apply_to_all))
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_cancel(state: State<'_, AppState>, session_id: SessionId, target: CancelTarget) -> Result<()> {
    with_entry(&state, session_id, |e| e.queue.cancel(target))
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_retry(state: State<'_, AppState>, session_id: SessionId, job_id: JobId) -> Result<()> {
    with_entry(&state, session_id, |e| e.queue.retry(job_id))
}

#[tauri::command]
#[specta::specta]
pub async fn transfer_clear(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    with_entry(&state, session_id, |e| {
        e.queue.clear();
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_reconnect_now(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    with_entry(&state, session_id, |e| {
        e.conn.reconnect_now();
        Ok(())
    })
}

#[tauri::command]
#[specta::specta]
pub async fn sftp_stop_reconnect(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    with_entry(&state, session_id, |e| {
        e.conn.stop();
        Ok(())
    })
}
```

`commands/mod.rs`: `pub mod sftp_transfers;`. `lib.rs`: add `sftp_transfers` to the `use commands::{…}` list and register the seven commands after `sftp::local_trash`.

Settings rename: in `dto.rs` rename the field `terminal_auto_reconnect` → `auto_reconnect` (default `true`, update the JSON test), in `settings.rs` read/write it under the unchanged key constant `TERMINAL_AUTO_RECONNECT`, and `auto_reconnect_enabled` returns `settings.auto_reconnect`.

- [ ] **Step 4: Regenerate bindings and run the tests**

Run: `cargo test -p linascp exports_bindings && cargo test -p linascp && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS. `ui/src/ipc/bindings.ts` changed (UI typecheck fails until Task 9; do not commit a broken `bun run typecheck` across a task boundary – Task 9 starts immediately and fixes it).

- [ ] **Step 5: Commit**

```bash
git add src-tauri ui/src/ipc/bindings.ts
git commit -m "feat(tauri): SFTP entries with reconnect and transfer commands

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: UI state – API wrappers, transfers store, SFTP events, settings rename

**Files:**
- Modify: `ui/src/ipc/api.ts`, `ui/src/stores/sftp.ts`, `ui/src/stores/sftp.test.ts`, `ui/src/stores/connect.ts`, `ui/src/stores/connect.test.ts`, `ui/src/stores/settings.ts`, `ui/src/screens/settings/SettingsDialog.tsx`, `ui/src/screens/settings/SettingsDialog.test.tsx`, `ui/src/i18n/de.ts`
- Create: `ui/src/stores/transfers.ts`, `ui/src/stores/transfers.test.ts`

**Interfaces:**
- Consumes: bindings from Task 8.
- Produces:
  - api: `transferEnqueue(sessionId, direction, sources, targetDir): Promise<number>`, `transferResolve(sessionId, jobId, decision, applyToAll)`, `transferCancel(sessionId, target)`, `transferRetry(sessionId, jobId)`, `transferClear(sessionId)`, `sftpReconnectNow(sessionId)`, `sftpStopReconnect(sessionId)`; `connectSftp(connectId, hostId, sessionId | null, events, sftpEvents)`; types `Job`, `JobState`, `TransferEvent`, `Direction`, `Decision`, `CancelTarget`, `FileStamp`.
  - `stores/transfers.ts`: `useTransfersStore` with state `byTab: Record<string, TabTransfers>`; `TabTransfers = { jobs: Record<number, Job>; order: number[]; conflicts: ConflictInfo[]; samples: Record<number, Sample[]>; view: TransferView; collapsed: boolean }`; `ConflictInfo = { jobId: number; source: FileStamp; target: FileStamp }`; `TransferView = "queue" | "done" | "failed"`; actions `onEvent(tabId, event, now?)`, `enqueue(tabId, direction, sources, targetDir)`, `resolve(tabId, decision, applyToAll)`, `cancel(tabId, target)`, `retry(tabId, jobId)`, `clear(tabId)`, `setView(tabId, view)`, `toggleCollapsed(tabId)`, `dropTab(tabId)`; selectors `viewOf(state: JobState): TransferView`, `openCount(t?: TabTransfers): number`, `speedOf(samples: Sample[], now: number): number | null`.
  - `stores/sftp.ts`: `SftpTab.status: "connected" | "reconnecting" | "lost"`, `SftpTab.reconnect?: { attempt; maxAttempts; retryAt; reason }`; `onEvent(tabId, sessionId, event: SftpEvent)`; `reconnect(tabId)` runs the interactive connect with the tab's session id; `closeTab` also calls `useTransfersStore.dropTab`.
  - `Settings.autoReconnect`.

- [ ] **Step 1: Write the failing tests**

`ui/src/stores/transfers.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../ipc/api";
import type { Job } from "../ipc/api";
import { useSftpStore } from "./sftp";
import { openCount, speedOf, useTransfersStore } from "./transfers";

vi.mock("../ipc/api");

const job = (patch: Partial<Job>): Job => ({
  id: 1, batchId: 1, direction: "upload", source: "/l/a", target: "/r/a", rel: "a", kind: "file",
  size: 100, done: 0, state: "queued", note: null, error: null, ...patch,
});
const store = useTransfersStore.getState;

beforeEach(() => {
  useTransfersStore.setState(useTransfersStore.getInitialState(), true);
  useSftpStore.setState(useSftpStore.getInitialState(), true);
});

describe("transfers store", () => {
  it("tracks jobs in order and sorts them into the three views", () => {
    store().onEvent("t", { kind: "job", job: job({ id: 1 }) });
    store().onEvent("t", { kind: "job", job: job({ id: 2, state: "failed" }) });
    store().onEvent("t", { kind: "job", job: job({ id: 1, state: "done", done: 100 }) });
    const t = store().byTab.t!;
    expect(t.order).toEqual([1, 2]);
    expect(t.jobs[1]!.state).toBe("done");
    expect(openCount(t)).toBe(0);
    store().onEvent("t", { kind: "job", job: job({ id: 3, state: "paused" }) });
    expect(openCount(store().byTab.t)).toBe(1);
  });

  it("applies progress and computes the speed over the last two seconds", () => {
    store().onEvent("t", { kind: "job", job: job({ id: 1, state: "running" }) });
    store().onEvent("t", { kind: "progress", jobId: 1, done: 0 }, 1000);
    store().onEvent("t", { kind: "progress", jobId: 1, done: 4000 }, 2000);
    store().onEvent("t", { kind: "progress", jobId: 1, done: 8000 }, 3000);
    const t = store().byTab.t!;
    expect(t.jobs[1]!.done).toBe(8000);
    expect(speedOf(t.samples[1]!, 3000)).toBe(4000);
    expect(speedOf([], 3000)).toBeNull();
  });

  it("queues conflicts and resolves the first one", async () => {
    useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
    store().onEvent("t", { kind: "conflict", jobId: 5, source: { size: 2, mtime: 2 }, target: { size: 1, mtime: 1 } });
    store().onEvent("t", { kind: "conflict", jobId: 6, source: { size: 2, mtime: 2 }, target: { size: 1, mtime: 1 } });
    await store().resolve("t", "overwrite", true);
    expect(api.transferResolve).toHaveBeenCalledWith("s1", 5, "overwrite", true);
    expect(store().byTab.t!.conflicts.map((c) => c.jobId)).toEqual([6]);
  });

  it("drops conflicts of jobs that ended otherwise", () => {
    store().onEvent("t", { kind: "conflict", jobId: 5, source: { size: 2, mtime: 2 }, target: { size: 1, mtime: 1 } });
    store().onEvent("t", { kind: "job", job: job({ id: 5, state: "cancelled" }) });
    expect(store().byTab.t!.conflicts).toEqual([]);
  });

  it("clear removes finished jobs locally and in the backend", async () => {
    useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
    store().onEvent("t", { kind: "job", job: job({ id: 1, state: "done" }) });
    store().onEvent("t", { kind: "job", job: job({ id: 2, state: "failed" }) });
    await store().clear("t");
    expect(api.transferClear).toHaveBeenCalledWith("s1");
    expect(store().byTab.t!.order).toEqual([2]);
  });

  it("enqueue passes the session id and opens the queue view", async () => {
    useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
    vi.mocked(api.transferEnqueue).mockResolvedValue(3);
    store().setView("t", "done");
    await store().enqueue("t", "download", ["/r/a"], "/l");
    expect(api.transferEnqueue).toHaveBeenCalledWith("s1", "download", ["/r/a"], "/l");
    expect(store().byTab.t!.view).toBe("queue");
  });
});
```

Extend `ui/src/stores/sftp.test.ts`:

```ts
  it("tracks reconnecting, reconnected and lost from SFTP events and reloads both panes", async () => {
    openWebTab(); // existing helper of this file that opens tab "t1" with session "s1"; create it if missing
    const sftp = useSftpStore.getState();
    sftp.onEvent("t1", "s1", { kind: "reconnecting", attempt: 2, maxAttempts: 8, retryInMs: 4000, reason: { type: "cancelled" } });
    let tab = useSftpStore.getState().tabs[0]!;
    expect(tab.status).toBe("reconnecting");
    expect(tab.reconnect?.attempt).toBe(2);
    vi.mocked(api.sftpList).mockClear();
    vi.mocked(api.localList).mockClear();
    sftp.onEvent("t1", "s1", { kind: "reconnected", info: tab.info });
    tab = useSftpStore.getState().tabs[0]!;
    expect(tab.status).toBe("connected");
    expect(tab.reconnect).toBeUndefined();
    await vi.waitFor(() => expect(api.sftpList).toHaveBeenCalled());
    expect(api.localList).toHaveBeenCalled();
    sftp.onEvent("t1", "s1", { kind: "lost", error: { type: "inputRequired" } });
    expect(useSftpStore.getState().tabs[0]!.status).toBe("lost");
  });

  it("ignores events of another session and forwards transfer events", () => {
    openWebTab();
    useSftpStore.getState().onEvent("t1", "other", { kind: "lost", error: { type: "cancelled" } });
    expect(useSftpStore.getState().tabs[0]!.status).toBe("connected");
    useSftpStore.getState().onEvent("t1", "s1", { kind: "transfer", event: { kind: "progress", jobId: 1, done: 5 } });
    expect(useTransfersStore.getState().byTab.t1).toBeDefined();
  });

  it("reconnect re-attaches through the connect flow with the tab's session id", () => {
    openWebTab();
    const start = vi.spyOn(useConnectStore.getState(), "start").mockResolvedValue();
    useSftpStore.getState().reconnect("t1");
    expect(start).toHaveBeenCalledWith(expect.any(String), "sftp", { sftpTabId: "t1" });
    expect(api.sftpClose).not.toHaveBeenCalled();
  });

  it("closing a tab drops its transfers", () => {
    openWebTab();
    useTransfersStore.getState().onEvent("t1", { kind: "progress", jobId: 1, done: 1 });
    useSftpStore.getState().closeTab("t1");
    expect(useTransfersStore.getState().byTab.t1).toBeUndefined();
  });
```

If `sftp.test.ts` has no `openWebTab` helper, add one that calls `useSftpStore.getState().openTab("t1", "h1", { sessionId: "s1", info, version: 3, listing: { path: "/r", entries: [] } })` with the mocks the existing tests use.

In `ui/src/stores/connect.test.ts`, adjust the SFTP tests to the new `connectSftp` arity (`connectId, hostId, null, events, sftpEvents`) and add:

```ts
  it("a reconnect of an SFTP tab passes its session id and keeps the session on a stale result", async () => {
    // tab t1 with session s1 exists (see the sftp tests' helper), its status is lost
    vi.mocked(api.connectSftp).mockImplementation(async (_c, _h, sessionId) => ({ ...connected, sessionId: sessionId! }));
    await useConnectStore.getState().start(web.id, "sftp", { sftpTabId: "t1" });
    expect(vi.mocked(api.connectSftp).mock.calls[0]![2]).toBe("s1");
    expect(api.sftpClose).not.toHaveBeenCalled();
  });
```

`SettingsDialog.test.tsx`: `terminalAutoReconnect` → `autoReconnect`; label text from `t.settings.autoReconnect`.

- [ ] **Step 2: Run them to verify they fail**

Run: `bun run test -- transfers sftp connect SettingsDialog`
Expected: FAIL (module `./transfers` missing, `onEvent` missing, old arity).

- [ ] **Step 3: Implement**

`ui/src/ipc/api.ts` (next to the other SFTP wrappers; add the new types to both `import type` lists):

```ts
export function connectSftp(
  connectId: string,
  hostId: Id,
  sessionId: SessionId | null,
  events: Channel<ConnectEvent>,
  sftpEvents: Channel<SftpEvent>,
): Promise<SftpConnected> {
  return unwrap(commands.connectSftp(connectId, hostId, sessionId, events, sftpEvents));
}
export function transferEnqueue(sessionId: SessionId, direction: Direction, sources: string[], targetDir: string): Promise<number> {
  return unwrap(commands.transferEnqueue(sessionId, direction, sources, targetDir));
}
export function transferResolve(sessionId: SessionId, jobId: number, decision: Decision, applyToAll: boolean): Promise<void> {
  return unwrapVoid(commands.transferResolve(sessionId, jobId, decision, applyToAll));
}
export function transferCancel(sessionId: SessionId, target: CancelTarget): Promise<void> {
  return unwrapVoid(commands.transferCancel(sessionId, target));
}
export function transferRetry(sessionId: SessionId, jobId: number): Promise<void> {
  return unwrapVoid(commands.transferRetry(sessionId, jobId));
}
export function transferClear(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.transferClear(sessionId));
}
export function sftpReconnectNow(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sftpReconnectNow(sessionId));
}
export function sftpStopReconnect(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sftpStopReconnect(sessionId));
}
```

and re-export the types `Job, JobState, JobKind, JobNote, TransferEvent, Direction, Decision, CancelTarget, FileStamp`.

`ui/src/stores/transfers.ts`:

```ts
import { create } from "zustand";
import {
  type CancelTarget,
  type Decision,
  type Direction,
  type FileStamp,
  type Job,
  type JobState,
  type TransferEvent,
  transferCancel,
  transferClear,
  transferEnqueue,
  transferResolve,
  transferRetry,
} from "../ipc/api";
import { fireAndForget } from "./connectProgress";
import { useSftpStore } from "./sftp";

export type TransferView = "queue" | "done" | "failed";
export interface Sample { at: number; done: number }
export interface ConflictInfo { jobId: number; source: FileStamp; target: FileStamp }
export interface TabTransfers {
  jobs: Record<number, Job>;
  order: number[];
  conflicts: ConflictInfo[];
  samples: Record<number, Sample[]>;
  view: TransferView;
  collapsed: boolean;
}

const SPEED_WINDOW_MS = 2000;

export function viewOf(state: JobState): TransferView {
  if (state === "failed") return "failed";
  return state === "done" || state === "skipped" || state === "cancelled" ? "done" : "queue";
}

export function openCount(t: TabTransfers | undefined): number {
  return t ? t.order.filter((id) => viewOf(t.jobs[id]!.state) === "queue").length : 0;
}

/** Bytes per second over the samples of the last two seconds; `null` without two samples. */
export function speedOf(samples: Sample[], now: number): number | null {
  const recent = samples.filter((s) => now - s.at <= SPEED_WINDOW_MS);
  if (recent.length < 2) return null;
  const first = recent[0]!;
  const last = recent.at(-1)!;
  return last.at > first.at ? ((last.done - first.done) * 1000) / (last.at - first.at) : null;
}

const empty = (): TabTransfers => ({ jobs: {}, order: [], conflicts: [], samples: {}, view: "queue", collapsed: false });

interface TransfersState {
  byTab: Record<string, TabTransfers>;
  onEvent(tabId: string, event: TransferEvent, now?: number): void;
  enqueue(tabId: string, direction: Direction, sources: string[], targetDir: string): Promise<void>;
  resolve(tabId: string, decision: Decision, applyToAll: boolean): Promise<void>;
  cancel(tabId: string, target: CancelTarget): void;
  retry(tabId: string, jobId: number): void;
  clear(tabId: string): Promise<void>;
  setView(tabId: string, view: TransferView): void;
  toggleCollapsed(tabId: string): void;
  dropTab(tabId: string): void;
}

const sessionOf = (tabId: string) => useSftpStore.getState().tabs.find((t) => t.id === tabId)?.sessionId;

export const useTransfersStore = create<TransfersState>()((set, get) => {
  const patch = (tabId: string, change: (t: TabTransfers) => Partial<TabTransfers>) =>
    set((s) => {
      const t = s.byTab[tabId] ?? empty();
      return { byTab: { ...s.byTab, [tabId]: { ...t, ...change(t) } } };
    });

  return {
    byTab: {},

    onEvent: (tabId, event, now = Date.now()) => {
      switch (event.kind) {
        case "job":
          patch(tabId, (t) => ({
            jobs: { ...t.jobs, [event.job.id]: event.job },
            order: t.order.includes(event.job.id) ? t.order : [...t.order, event.job.id],
            conflicts: event.job.state === "waitingDecision" ? t.conflicts : t.conflicts.filter((c) => c.jobId !== event.job.id),
            samples: event.job.state === "running" ? t.samples : { ...t.samples, [event.job.id]: [] },
          }));
          break;
        case "progress":
          patch(tabId, (t) => {
            const old = t.jobs[event.jobId];
            const samples = [...(t.samples[event.jobId] ?? []), { at: now, done: event.done }].filter((s) => now - s.at <= SPEED_WINDOW_MS);
            return {
              jobs: old ? { ...t.jobs, [event.jobId]: { ...old, done: event.done } } : t.jobs,
              samples: { ...t.samples, [event.jobId]: samples },
            };
          });
          break;
        case "conflict":
          patch(tabId, (t) => ({ conflicts: [...t.conflicts, { jobId: event.jobId, source: event.source, target: event.target }] }));
          break;
        case "batchDone": {
          const side = event.direction === "upload" ? "remote" : "local";
          const tab = useSftpStore.getState().tabs.find((x) => x.id === tabId);
          if (tab?.panes[side].path === event.targetDir) void useSftpStore.getState().reload(tabId, side);
          break;
        }
      }
    },

    enqueue: async (tabId, direction, sources, targetDir) => {
      const sessionId = sessionOf(tabId);
      if (!sessionId) return;
      patch(tabId, () => ({ view: "queue", collapsed: false }));
      await transferEnqueue(sessionId, direction, sources, targetDir);
    },

    resolve: async (tabId, decision, applyToAll) => {
      const sessionId = sessionOf(tabId);
      const first = get().byTab[tabId]?.conflicts[0];
      if (!sessionId || !first) return;
      patch(tabId, (t) => ({ conflicts: t.conflicts.slice(1) }));
      await transferResolve(sessionId, first.jobId, decision, applyToAll);
    },

    cancel: (tabId, target) => {
      const sessionId = sessionOf(tabId);
      if (sessionId) fireAndForget(() => transferCancel(sessionId, target));
    },

    retry: (tabId, jobId) => {
      const sessionId = sessionOf(tabId);
      if (sessionId) fireAndForget(() => transferRetry(sessionId, jobId));
    },

    clear: async (tabId) => {
      const sessionId = sessionOf(tabId);
      if (!sessionId) return;
      patch(tabId, (t) => {
        const keep = t.order.filter((id) => viewOf(t.jobs[id]!.state) !== "done");
        return { order: keep, jobs: Object.fromEntries(keep.map((id) => [id, t.jobs[id]!])) };
      });
      await transferClear(sessionId);
    },

    setView: (tabId, view) => patch(tabId, () => ({ view })),
    toggleCollapsed: (tabId) => patch(tabId, (t) => ({ collapsed: !t.collapsed })),
    dropTab: (tabId) =>
      set((s) => {
        const { [tabId]: _gone, ...rest } = s.byTab;
        return { byTab: rest };
      }),
  };
});
```

`stores/sftp.ts`:
- `status: "connected" | "reconnecting" | "lost"`; add `reconnect?: { attempt: number; maxAttempts: number; retryAt: number; reason: CoreError }`.
- Replace `markLost` with:

```ts
    onEvent: (tabId, sessionId, event) => {
      if (findTab(tabId)?.sessionId !== sessionId) return;
      switch (event.kind) {
        case "transfer":
          useTransfersStore.getState().onEvent(tabId, event.event);
          return;
        case "reconnecting":
          patchTab(tabId, () => ({
            status: "reconnecting",
            reconnect: { attempt: event.attempt, maxAttempts: event.maxAttempts, retryAt: Date.now() + event.retryInMs, reason: event.reason },
          }));
          return;
        case "reconnected":
          patchTab(tabId, () => ({ status: "connected", reconnect: undefined, lostError: undefined, info: event.info }));
          void get().reload(tabId, "remote");
          void get().reload(tabId, "local");
          return;
        case "lost":
          patchTab(tabId, () => ({ status: "lost", reconnect: undefined, lostError: event.error }));
      }
    },
```

- `reconnect(tabId)`: only `void useConnectStore.getState().start(tab.hostId, "sftp", { sftpTabId: tabId });` (no `sftpClose`).
- `closeTab`: also `useTransfersStore.getState().dropTab(tabId);`.
- `attachSession`: unchanged behaviour (the session id stays the same on a re-attach).
- Interface: replace `markLost(...)` by `onEvent(tabId: string, sessionId: string, event: SftpEvent): void`.

The circular import `sftp.ts` ↔ `transfers.ts` is fine for zustand stores (only used inside functions).

`stores/connect.ts` → `runSftp`:

```ts
    const existing = flow.sftpTabId ? useSftpStore.getState().tabs.find((t) => t.id === flow.sftpTabId)?.sessionId ?? null : null;
    const sftpEvents = createEventChannel<SftpEvent>((event) => {
      if (bound !== null) useSftpStore.getState().onEvent(tabId, bound, event);
    });
    const connected = await connectSftp(connectId, flow.hostId, existing, events, sftpEvents);
    if (!isCurrent(connectId)) {
      // A re-attached connection belongs to the tab's session: never close that one here.
      if (!existing) fireAndForget(() => sftpClose(connected.sessionId));
      return;
    }
```

Note: on a re-attach the events keep arriving on the tab's **original** channel (the backend keeps the first `SftpEmit`), whose handler is already bound; the new channel stays silent.

Settings: `stores/settings.ts` default `autoReconnect: true`; `SettingsDialog.tsx` uses `settings.autoReconnect` / `persist({ autoReconnect })`; `de.ts` `settings.autoReconnect: "Abgebrochene Verbindungen automatisch wiederherstellen (SSH und SFTP)"`; `editor.sftpHint: "Startverzeichnisse, Parallelität und Rechte beim Upload wirken ab sofort. Modus und Bandbreite wirken ab Teilprojekt 4."`.

Every other `terminalAutoReconnect` hit (`grep -rn terminalAutoReconnect ui/src`) is renamed too.

- [ ] **Step 4: Run the tests**

Run: `bun run typecheck && bun run test`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add ui/src
git commit -m "feat(ui): transfers store, SFTP reconnect events and the shared auto-reconnect setting

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: UI triggers – F5, context menu, toolbar, pointer drag, Nautilus drop

**Files:**
- Create: `ui/src/screens/sftp/transferTargets.ts`, `ui/src/screens/sftp/transferTargets.test.ts`, `ui/src/screens/sftp/useExternalDrop.ts`
- Modify: `ui/src/screens/sftp/FilePane.tsx`, `ui/src/screens/sftp/FilePane.module.css`, `ui/src/screens/sftp/FilePane.test.tsx`, `ui/src/screens/sftp/SftpScreen.tsx`, `ui/src/screens/sftp/SftpScreen.test.tsx`, `ui/src/i18n/de.ts`

**Interfaces:**
- Consumes: `useTransfersStore.enqueue` (Task 9), `targets` (`paneKeys.ts`), `joinPath`, `isDirLike` (`entries.ts`).
- Produces:
  - `transferTargets.ts`: `export interface TransferRequest { direction: Direction; sources: string[]; targetDir: string }`; `export function requestFor(tab: SftpTab, from: Side, names: string[], dropDir?: string | null): TransferRequest | null` (null when nothing usable or when the remote is not connected); `export function dropDirAt(tab: SftpTab, side: Side, rowName: string | null): string` (a directory row → its path, else the pane path).
  - `FilePane` prop `onTransfer(names: string[], toSide: Side, dropRow: string | null)` (`dropRow`: the row the drag ended on in the other pane, `null` for its background or F5/menu); data attributes `data-sftp-pane={side}` on the section, `data-sftp-row={name}` on rows.
  - `useExternalDrop(tabId: string)` hook.

- [ ] **Step 1: Write the failing tests**

`ui/src/screens/sftp/transferTargets.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import type { Entry } from "../../ipc/api";
import type { SftpTab } from "../../stores/sftp";
import { dropDirAt, requestFor } from "./transferTargets";

const entry = (name: string, type: "file" | "dir" = "file", nameLossy = false): Entry =>
  ({ name, kind: { type }, size: 1, mtime: 0, mode: 0, owner: null, nameLossy }) as Entry;

function tab(status: SftpTab["status"] = "connected"): SftpTab {
  const pane = (path: string, entries: Entry[]) => ({ path, entries, loading: false, selection: [], cursor: null, anchor: null,
    sort: { key: "name", dir: "asc" }, showHidden: false, editingPath: false, token: 0 });
  return {
    id: "t", hostId: "h", sessionId: "s", title: "web", status, info: {} as never, version: 3, active: "local",
    panes: { local: pane("/home/me", [entry("a.txt"), entry("docs", "dir"), entry("bad", "file", true)]), remote: pane("/srv", [entry("www", "dir")]) },
  } as SftpTab;
}

describe("requestFor", () => {
  it("uploads from the local pane into the remote directory", () => {
    expect(requestFor(tab(), "local", ["a.txt", "docs"])).toEqual({
      direction: "upload", sources: ["/home/me/a.txt", "/home/me/docs"], targetDir: "/srv",
    });
  });

  it("downloads into a drop directory", () => {
    expect(requestFor(tab(), "remote", ["www"], "/home/me/docs")).toEqual({
      direction: "download", sources: ["/srv/www"], targetDir: "/home/me/docs",
    });
  });

  it("skips .. and lossy names and returns null when nothing is left", () => {
    expect(requestFor(tab(), "local", ["..", "bad"])).toBeNull();
    expect(requestFor(tab(), "local", ["..", "a.txt"])?.sources).toEqual(["/home/me/a.txt"]);
  });

  it("needs a connected remote", () => {
    expect(requestFor(tab("reconnecting"), "local", ["a.txt"])).toBeNull();
    expect(requestFor(tab("lost"), "remote", ["www"])).toBeNull();
  });
});

describe("dropDirAt", () => {
  it("uses a directory row, else the pane path", () => {
    expect(dropDirAt(tab(), "local", "docs")).toBe("/home/me/docs");
    expect(dropDirAt(tab(), "local", "a.txt")).toBe("/home/me");
    expect(dropDirAt(tab(), "local", null)).toBe("/home/me");
    expect(dropDirAt(tab(), "local", "..")).toBe("/home/me");
  });
});
```

`FilePane.test.tsx` additions (use the file's existing render helper; `onTransfer` is a `vi.fn()` prop):

```tsx
  it("F5 transfers the selection to the other pane", async () => {
    const onTransfer = vi.fn();
    renderPane("local", { onTransfer }); // existing helper; add the prop pass-through if needed
    selectNames("local", ["a.txt"]);
    await userEvent.keyboard("{F5}");
    expect(onTransfer).toHaveBeenCalledWith(["a.txt"], "remote", null);
  });

  it("offers Hochladen in the local context menu", async () => {
    const onTransfer = vi.fn();
    renderPane("local", { onTransfer });
    await userEvent.pointer({ keys: "[MouseRight]", target: screen.getByText("a.txt") });
    await userEvent.click(screen.getByRole("menuitem", { name: "Hochladen" }));
    expect(onTransfer).toHaveBeenCalledWith(["a.txt"], "remote", null);
  });

  it("dragging rows onto the other pane's folder transfers into that folder", () => {
    const onTransfer = vi.fn();
    renderBothPanes({ onTransfer }); // renders local + remote FilePane of tab "t1"
    const row = within(screen.getByRole("region", { name: "Lokal" })).getByText("a.txt");
    const target = within(screen.getByRole("region", { name: "Remote" })).getByText("www");
    document.elementFromPoint = vi.fn(() => target);
    fireEvent.pointerDown(row, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(document, { clientX: 300, clientY: 40, pointerId: 1 });
    fireEvent.pointerUp(document, { clientX: 300, clientY: 40, pointerId: 1 });
    expect(onTransfer).toHaveBeenCalledWith(["a.txt"], "remote", "www");
  });

  it("a short click is not a drag", () => {
    const onTransfer = vi.fn();
    renderBothPanes({ onTransfer });
    const row = within(screen.getByRole("region", { name: "Lokal" })).getByText("a.txt");
    fireEvent.pointerDown(row, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerUp(document, { clientX: 12, clientY: 11, pointerId: 1 });
    expect(onTransfer).not.toHaveBeenCalled();
  });
```

(The `section` already has `aria-label` = "Lokal"/"Remote"; testing-library finds it as `region` only with an accessible name — it has one.)

`SftpScreen.test.tsx` addition:

```tsx
  it("the toolbar transfer button uploads from the local pane and downloads from the remote pane", async () => {
    openTab(); // existing helper
    vi.mocked(api.transferEnqueue).mockResolvedValue(1);
    selectNames("local", ["a.txt"]);
    useSftpStore.getState().setActivePane("t1", "local");
    await userEvent.click(screen.getByRole("button", { name: "Hochladen" }));
    expect(api.transferEnqueue).toHaveBeenCalledWith("s1", "upload", ["/home/me/a.txt"], "/srv");
  });
```

Adapt names/paths to the fixtures already used in these test files.

- [ ] **Step 2: Run them to verify they fail**

Run: `bun run test -- transferTargets FilePane SftpScreen`
Expected: FAIL.

- [ ] **Step 3: Implement**

`ui/src/screens/sftp/transferTargets.ts`:

```ts
import type { Direction } from "../../ipc/api";
import type { Side, SftpTab } from "../../stores/sftp";
import { isDirLike, joinPath } from "./entries";

export interface TransferRequest {
  direction: Direction;
  sources: string[];
  targetDir: string;
}

/** What a transfer of `names` from pane `from` means; null if nothing usable or not connected. */
export function requestFor(tab: SftpTab, from: Side, names: string[], dropDir: string | null = null): TransferRequest | null {
  if (tab.status !== "connected") return null;
  const pane = tab.panes[from];
  const usable = names.filter((n) => n !== ".." && pane.entries.some((e) => e.name === n && !e.nameLossy));
  if (usable.length === 0) return null;
  const to: Side = from === "local" ? "remote" : "local";
  return {
    direction: from === "local" ? "upload" : "download",
    sources: usable.map((n) => joinPath(pane.path, n)),
    targetDir: dropDir ?? tab.panes[to].path,
  };
}

/** Target folder of a drop on `rowName` (a directory row) or on the pane background. */
export function dropDirAt(tab: SftpTab, side: Side, rowName: string | null): string {
  const pane = tab.panes[side];
  const entry = rowName ? pane.entries.find((e) => e.name === rowName) : undefined;
  return entry && isDirLike(entry) && !entry.nameLossy ? joinPath(pane.path, entry.name) : pane.path;
}
```

`FilePane.tsx`:
- Add prop `onTransfer: (names: string[], toSide: Side, dropRow: string | null) => void` (the screen turns the row into a directory via `dropDirAt`; this keeps `FilePane` free of the other pane's state). The tests above pass the row name (`"www"`) and `null` for "current folder".
- `const other: Side = side === "local" ? "remote" : "local";`
- Key map: `F5: () => { const chosen = targets(p); if (chosen.length) onTransfer(chosen, other, null); },`
- Context menu: after "Öffnen": `{menuItem(side === "local" ? t.sftp.upload : t.sftp.download, () => onTransfer(targets(p), other, null))}`.
- Pointer drag (rows only, left button, not when `disabled`):

```tsx
  const drag = useRef<{ names: string[]; x: number; y: number; active: boolean } | null>(null);
  const [dragging, setDragging] = useState(false);

  const onRowPointerDown = (event: PointerEvent<HTMLDivElement>, name: string) => {
    if (event.button !== 0 || disabled || name === "..") return;
    const names = p.selection.includes(name) ? targets(p) : [name];
    drag.current = { names, x: event.clientX, y: event.clientY, active: false };
    const move = (e: globalThis.PointerEvent) => {
      const d = drag.current;
      if (d && !d.active && Math.hypot(e.clientX - d.x, e.clientY - d.y) > 4) {
        d.active = true;
        setDragging(true);
      }
    };
    const up = (e: globalThis.PointerEvent) => {
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", up);
      const d = drag.current;
      drag.current = null;
      setDragging(false);
      if (!d?.active) return;
      const hit = document.elementFromPoint(e.clientX, e.clientY);
      const pane = hit?.closest<HTMLElement>("[data-sftp-pane]")?.dataset.sftpPane as Side | undefined;
      if (pane !== other) return;
      const row = hit?.closest<HTMLElement>("[data-sftp-row]")?.dataset.sftpRow ?? null;
      onTransfer(d.names, other, row === ".." ? null : row);
    };
    document.addEventListener("pointermove", move);
    document.addEventListener("pointerup", up);
  };
```

Rows get `data-sftp-row={name}` and `onPointerDown={(e) => onRowPointerDown(e, name)}`; the `<section>` gets `data-sftp-pane={side}` and `className={cx(..., dragging && styles.dragging)}` (cursor `copy` while dragging). Import `type PointerEvent` from React.

`FilePane.module.css`:

```css
.dragging {
  cursor: copy;
}

.dropTarget {
  outline: 2px dashed var(--accent);
  outline-offset: -2px;
}
```

`useExternalDrop.ts` (Nautilus → remote pane):

```ts
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useState } from "react";
import { useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { dropDirAt } from "./transferTargets";

/** Files dropped from outside the app onto the remote pane are uploaded. Returns whether a drag hovers it. */
export function useExternalDrop(tabId: string): boolean {
  const [over, setOver] = useState(false);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    const hit = (x: number, y: number) => {
      const scale = window.devicePixelRatio || 1;
      return document.elementFromPoint(x / scale, y / scale);
    };
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "leave") return setOver(false);
        const el = hit(payload.position.x, payload.position.y);
        const onRemote = el?.closest<HTMLElement>("[data-sftp-pane]")?.dataset.sftpPane === "remote";
        if (payload.type !== "drop") return setOver(onRemote);
        setOver(false);
        const tab = useSftpStore.getState().tabs.find((t) => t.id === tabId);
        if (!onRemote || !tab || tab.status !== "connected" || payload.paths.length === 0) return;
        const row = el?.closest<HTMLElement>("[data-sftp-row]")?.dataset.sftpRow ?? null;
        void useTransfersStore.getState().enqueue(tabId, "upload", payload.paths, dropDirAt(tab, "remote", row));
      })
      .then((fn) => (alive ? (unlisten = fn) : fn()));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [tabId]);
  return over;
}
```

Tests mock `@tauri-apps/api/webview` (`vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent: vi.fn().mockResolvedValue(() => {}) }) }))`) in `SftpScreen.test.tsx` and in any test that renders `SftpScreen`. If the payload type for `over` is named `"over"` in the installed `@tauri-apps/api` version, the branches above still hold (only `leave` and `drop` are special-cased).

`SftpScreen.tsx`:
- `const onTransfer = (from: Side) => (names: string[], to: Side, row: string | null) => { const request = requestFor(tab, from, names, row === null ? null : dropDirAt(tab, to, row)); if (request) void useTransfersStore.getState().enqueue(tab.id, request.direction, request.sources, request.targetDir); };` — pass `onTransfer={onTransfer("local")}` / `onTransfer={onTransfer("remote")}` to the panes.
- Toolbar button (first in the toolbar): `<Button icon={side === "local" ? Upload : Download} disabled={tab.status !== "connected"} onClick={() => onTransfer(side)(targets(pane), side === "local" ? "remote" : "local", null)}>{side === "local" ? t.sftp.upload : t.sftp.download}</Button>` (`Upload`, `Download` from lucide-react).
- `const externalOver = useExternalDrop(tab.id);` → wrap the remote `FilePane`'s container with `className={cx(styles.remote, externalOver && filePaneStyles.dropTarget)}` (or add the class to `SftpScreen.module.css` as `.dropTarget` with the same rule; keep one definition).
- `sideDisabled(s)` becomes `s === "remote" && tab.status !== "connected"`.

`de.ts` (`t.sftp`): `upload: "Hochladen", download: "Herunterladen",`.

- [ ] **Step 4: Run the tests**

Run: `bun run typecheck && bun run test`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add ui/src
git commit -m "feat(ui): start transfers by F5, menu, toolbar, drag and Nautilus drop

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: UI – transfer panel, conflict dialog, reconnect bar, closing tabs

**Files:**
- Create: `ui/src/screens/sftp/TransferPanel.tsx`, `ui/src/screens/sftp/TransferPanel.module.css`, `ui/src/screens/sftp/TransferPanel.test.tsx`, `ui/src/screens/sftp/ConflictDialog.tsx`, `ui/src/screens/sftp/ConflictDialog.test.tsx`, `ui/src/screens/sftp/CloseTabDialog.tsx`
- Modify: `ui/src/screens/terminal/ReconnectBar.tsx`, `ui/src/screens/terminal/ReconnectBar.test.tsx`, `ui/src/screens/terminal/PaneLayout.tsx`, `ui/src/screens/sftp/SftpScreen.tsx`, `ui/src/screens/sftp/SftpScreen.module.css`, `ui/src/screens/sftp/SftpScreen.test.tsx`, `ui/src/i18n/de.ts`

**Interfaces:**
- Consumes: `useTransfersStore`, `viewOf`, `openCount`, `speedOf` (Task 9); `formatSize`, `formatMtime` (`entries.ts`); `sftpReconnectNow`, `sftpStopReconnect` (Task 9).
- Produces:
  - `ReconnectBar({ state, onNow, onStop })` where `state: { attempt: number; maxAttempts: number; retryAt: number }`.
  - `TransferPanel({ tabId })`, `ConflictDialog({ tabId })`, `CloseTabDialog({ count, onConfirm, onClose })`.

- [ ] **Step 1: Add the strings** (`de.ts`, inside `t.sftp`):

```ts
    transfers: {
      title: "Übertragungen",
      views: { queue: "Warteschlange", done: "Abgeschlossen", failed: "Fehler" },
      summary: (open: number, done: number, failed: number) => `${open} laufen · ${done} fertig · ${failed} Fehler`,
      expand: "Übertragungen ausklappen",
      collapse: "Übertragungen einklappen",
      states: {
        queued: "Wartet", running: "", waitingDecision: "Wartet auf Entscheidung", paused: "Unterbrochen",
        done: "Fertig", skipped: "Übersprungen", failed: "Fehler", cancelled: "Abgebrochen",
      },
      notes: { dirLink: "Link auf Ordner – übersprungen", brokenLink: "Link ohne Ziel – übersprungen", special: "Spezialdatei – übersprungen" },
      cancel: (name: string) => `${name} abbrechen`,
      retry: (name: string) => `${name} wiederholen`,
      clear: "Leeren",
      speed: (perSecond: string) => `${perSecond}/s`,
      empty: "Keine Einträge",
      networkError: "Verbindung unterbrochen",
      inputRequired: "Für die Verbindung ist eine Eingabe nötig",
    },
    conflict: {
      title: "Datei existiert bereits",
      text: (name: string) => `${name} ist im Zielordner schon vorhanden.`,
      newSide: (side: string) => `Neu · ${side}`,
      existingSide: (side: string) => `Vorhanden · ${side}`,
      applyToAll: "Für alle weiteren Konflikte übernehmen",
      skip: "Überspringen",
      newer: "Nur wenn neuer",
      overwrite: "Überschreiben",
    },
    closeDialog: {
      title: "Übertragungen offen",
      text: (count: number) => `Noch ${count} ${count === 1 ? "Übertragung" : "Übertragungen"} offen. Tab trotzdem schließen?`,
      confirm: "Schließen",
    },
    pendingConflict: "Wartet auf eine Entscheidung",
```

- [ ] **Step 2: Write the failing tests**

`ReconnectBar.test.tsx`: change the rendering to the new props:

```tsx
render(<ReconnectBar state={{ attempt: 2, maxAttempts: 8, retryAt: Date.now() + 3000 }} onNow={onNow} onStop={onStop} />);
```

and assert `onNow`/`onStop` are called by the buttons (instead of `sessionReconnectNow`). `PaneLayout.tsx` passes `state={pane.reconnect}`, `onNow={() => fireAndForget(() => sessionReconnectNow(sessionId))}`, `onStop={…sessionReconnectStop…}` — keep its existing tests green.

`TransferPanel.test.tsx`:

```tsx
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { Job } from "../../ipc/api";
import { useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { TransferPanel } from "./TransferPanel";

vi.mock("../../ipc/api");

const job = (patch: Partial<Job>): Job => ({
  id: 1, batchId: 1, direction: "upload", source: "/l/a", target: "/srv/a", rel: "a.zip", kind: "file",
  size: 1000, done: 0, state: "queued", note: null, error: null, ...patch,
});
const event = (j: Partial<Job>) => useTransfersStore.getState().onEvent("t", { kind: "job", job: job(j) });

beforeEach(() => {
  useTransfersStore.setState(useTransfersStore.getInitialState(), true);
  useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
});

describe("TransferPanel", () => {
  it("is hidden without jobs", () => {
    const { container } = render(<TransferPanel tabId="t" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("sorts jobs into the three views with counters", async () => {
    event({ id: 1, state: "running", done: 640, rel: "theme.zip" });
    event({ id: 2, state: "done", rel: "logo.svg", direction: "download", target: "/home/me/logo.svg" });
    event({ id: 3, state: "failed", rel: ".htaccess", error: { type: "fs", kind: "permissionDenied", path: "/srv/.htaccess", message: "denied" } });
    render(<TransferPanel tabId="t" />);
    const tabs = screen.getByRole("tablist", { name: "Übertragungen" });
    expect(within(tabs).getByRole("tab", { name: "Warteschlange 1" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("theme.zip")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "64");
    await userEvent.click(within(tabs).getByRole("tab", { name: "Fehler 1" }));
    expect(screen.getByText("Keine Berechtigung")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: ".htaccess wiederholen" }));
    expect(api.transferRetry).toHaveBeenCalledWith("s1", 3);
    await userEvent.click(within(tabs).getByRole("tab", { name: "Abgeschlossen 1" }));
    await userEvent.click(screen.getByRole("button", { name: "Leeren" }));
    expect(api.transferClear).toHaveBeenCalledWith("s1");
  });

  it("cancels a queued job and collapses to a summary", async () => {
    event({ id: 1, state: "running", rel: "a.zip" });
    event({ id: 2, state: "done", rel: "b" });
    render(<TransferPanel tabId="t" />);
    await userEvent.click(screen.getByRole("button", { name: "a.zip abbrechen" }));
    expect(api.transferCancel).toHaveBeenCalledWith("s1", { type: "job", id: 1 });
    await userEvent.click(screen.getByRole("button", { name: "Übertragungen einklappen" }));
    expect(screen.getByText("1 laufen · 1 fertig · 0 Fehler")).toBeInTheDocument();
    expect(screen.queryByText("a.zip")).not.toBeInTheDocument();
  });

  it("shows skip notes and network errors in German, never the raw message", () => {
    event({ id: 1, state: "skipped", rel: "link", note: "dirLink" });
    event({ id: 2, state: "failed", rel: "x", error: { type: "network", kind: "other", message: "connection lost" } });
    useTransfersStore.getState().setView("t", "done");
    const { rerender } = render(<TransferPanel tabId="t" />);
    expect(screen.getByText("Link auf Ordner – übersprungen")).toBeInTheDocument();
    useTransfersStore.getState().setView("t", "failed");
    rerender(<TransferPanel tabId="t" />);
    expect(screen.getByText("Verbindung unterbrochen")).toBeInTheDocument();
    expect(screen.queryByText("connection lost")).not.toBeInTheDocument();
  });
});
```

`ConflictDialog.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import { useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { ConflictDialog } from "./ConflictDialog";

vi.mock("../../ipc/api");

beforeEach(() => {
  useTransfersStore.setState(useTransfersStore.getInitialState(), true);
  useSftpStore.setState({ tabs: [{ id: "t", sessionId: "s1" } as never] });
  const s = useTransfersStore.getState();
  s.onEvent("t", { kind: "job", job: { id: 5, batchId: 2, direction: "upload", source: "/l/site.sql.gz", target: "/srv/site.sql.gz", rel: "site.sql.gz", kind: "file", size: 18_400_000, done: 0, state: "waitingDecision", note: null, error: null } });
  s.onEvent("t", { kind: "conflict", jobId: 5, source: { size: 18_400_000, mtime: 1_791_000_000 }, target: { size: 17_900_000, mtime: 1_790_000_000 } });
});

describe("ConflictDialog", () => {
  it("shows both sides and focuses Überspringen", () => {
    render(<ConflictDialog tabId="t" />);
    expect(screen.getByRole("dialog", { name: "Datei existiert bereits" })).toBeInTheDocument();
    expect(screen.getByText("site.sql.gz ist im Zielordner schon vorhanden.")).toBeInTheDocument();
    expect(screen.getByText("Neu · Lokal")).toBeInTheDocument();
    expect(screen.getByText("Vorhanden · Remote")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Überspringen" })).toHaveFocus();
  });

  it("answers with the chosen decision and the checkbox", async () => {
    render(<ConflictDialog tabId="t" />);
    await userEvent.click(screen.getByRole("checkbox", { name: "Für alle weiteren Konflikte übernehmen" }));
    await userEvent.click(screen.getByRole("button", { name: "Nur wenn neuer" }));
    expect(api.transferResolve).toHaveBeenCalledWith("s1", 5, "newer", true);
  });

  it("Escape cancels the whole batch", async () => {
    render(<ConflictDialog tabId="t" />);
    await userEvent.keyboard("{Escape}");
    expect(api.transferResolve).toHaveBeenCalledWith("s1", 5, "cancelBatch", false);
  });

  it("renders nothing without a conflict", () => {
    useTransfersStore.setState(useTransfersStore.getInitialState(), true);
    const { container } = render(<ConflictDialog tabId="t" />);
    expect(container).toBeEmptyDOMElement();
  });
});
```

`SftpScreen.test.tsx` additions:

```tsx
  it("shows the reconnect bar while reconnecting and calls sftpReconnectNow", async () => {
    openTab();
    useSftpStore.getState().onEvent("t1", "s1", { kind: "reconnecting", attempt: 1, maxAttempts: 8, retryInMs: 1000, reason: { type: "cancelled" } });
    await userEvent.click(await screen.findByRole("button", { name: "Jetzt versuchen" }));
    expect(api.sftpReconnectNow).toHaveBeenCalledWith("s1");
  });

  it("asks before closing a tab with open transfers", async () => {
    openTab();
    useTransfersStore.getState().onEvent("t1", { kind: "job", job: { id: 1, batchId: 1, direction: "upload", source: "/a", target: "/b", rel: "a", kind: "file", size: 1, done: 0, state: "running", note: null, error: null } });
    await userEvent.click(screen.getByRole("button", { name: /schließen$/ }));
    expect(screen.getByRole("alertdialog", { name: "Übertragungen offen" })).toBeInTheDocument();
    expect(api.sftpClose).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Schließen" }));
    expect(api.sftpClose).toHaveBeenCalledWith("s1");
  });

  it("marks another tab that waits for a conflict decision", () => {
    openTwoTabs(); // t1 active, t2 in the background
    useTransfersStore.getState().onEvent("t2", { kind: "conflict", jobId: 1, source: { size: 1, mtime: 1 }, target: { size: 1, mtime: 1 } });
    expect(screen.getByRole("tab", { name: /Wartet auf eine Entscheidung/ })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Datei existiert bereits" })).not.toBeInTheDocument();
  });
```

(Add `openTwoTabs` next to the existing `openTab` helper if missing.)

- [ ] **Step 3: Run them to verify they fail**

Run: `bun run test -- ReconnectBar TransferPanel ConflictDialog SftpScreen PaneLayout`
Expected: FAIL.

- [ ] **Step 4: Implement**

`ReconnectBar.tsx`:

```tsx
/** Strip over a pane whose connection is being restored: countdown, "Jetzt versuchen", "Abbrechen". */
export function ReconnectBar({ state, onNow, onStop }: {
  state: { attempt: number; maxAttempts: number; retryAt: number };
  onNow: () => void;
  onStop: () => void;
}) {
  const texts = t.terminal.reconnecting;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const seconds = Math.max(0, Math.ceil((state.retryAt - now) / 1000));
  return (
    <div className={styles.bar}>
      <span className={styles.text}>
        {/* Only the attempt is live: the countdown would be re-announced every second. */}
        <span role="status">{texts.attempt(state.attempt, state.maxAttempts)}</span>
        {seconds > 0 ? texts.countdown(seconds) : texts.connecting}
      </span>
      <Button size="sm" variant="primary" onClick={onNow}>{texts.now}</Button>
      <Button size="sm" variant="ghost" onClick={onStop}>{texts.stop}</Button>
    </div>
  );
}
```

`PaneLayout.tsx`:

```tsx
      {pane.status === "reconnecting" && pane.reconnect && pane.sessionId ? (
        <ReconnectBar
          state={pane.reconnect}
          onNow={() => fireAndForget(() => sessionReconnectNow(pane.sessionId!))}
          onStop={() => fireAndForget(() => sessionReconnectStop(pane.sessionId!))}
        />
      ) : null}
```

`TransferPanel.tsx`:

```tsx
import { ChevronDown, ChevronUp, RotateCcw, X } from "lucide-react";
import { Button, cx, IconButton, ProgressBar } from "../../components";
import { t } from "../../i18n/de";
import type { CoreError, Job } from "../../ipc/api";
import { openCount, speedOf, type TransferView, useTransfersStore, viewOf } from "../../stores/transfers";
import { formatSize } from "./entries";
import styles from "./TransferPanel.module.css";

const VIEWS: TransferView[] = ["queue", "done", "failed"];

function errorText(error: CoreError): string {
  const texts = t.sftp.transfers;
  if (error.type === "fs") return t.sftp.fsErrors[error.kind];
  if (error.type === "network") return texts.networkError;
  if (error.type === "inputRequired") return texts.inputRequired;
  return t.sftp.failed;
}

function stateText(job: Job): string {
  const texts = t.sftp.transfers;
  if (job.state === "failed" && job.error) return errorText(job.error);
  if (job.state === "skipped" && job.note) return texts.notes[job.note];
  if (job.state === "running") return `${job.size > 0 ? Math.floor((job.done * 100) / job.size) : 100} %`;
  return texts.states[job.state];
}

const parentDir = (path: string) => path.slice(0, path.lastIndexOf("/")) || "/";

export function TransferPanel({ tabId }: { tabId: string }) {
  const tab = useTransfersStore((s) => s.byTab[tabId]);
  const store = useTransfersStore.getState;
  if (!tab || tab.order.length === 0) return null;
  const texts = t.sftp.transfers;
  const counts = { queue: 0, done: 0, failed: 0 };
  for (const id of tab.order) counts[viewOf(tab.jobs[id]!.state)]++;
  const now = Date.now();
  const rows = tab.order.map((id) => tab.jobs[id]!).filter((j) => viewOf(j.state) === tab.view);

  return (
    <section aria-label={texts.title} className={styles.panel}>
      <header className={styles.head}>
        {tab.collapsed ? (
          <span className={styles.summary}>{texts.summary(openCount(tab), counts.done, counts.failed)}</span>
        ) : (
          <div role="tablist" aria-label={texts.title} className={styles.views}>
            {VIEWS.map((view) => (
              <button key={view} type="button" role="tab" aria-selected={tab.view === view}
                className={cx(styles.view, tab.view === view && styles.activeView)}
                onClick={() => store().setView(tabId, view)}>
                {`${texts.views[view]} ${counts[view]}`}
              </button>
            ))}
          </div>
        )}
        <span className={styles.spacer} />
        {!tab.collapsed && tab.view === "done" && counts.done > 0 ? (
          <Button size="sm" variant="ghost" onClick={() => void store().clear(tabId)}>{texts.clear}</Button>
        ) : null}
        <IconButton size="sm" icon={tab.collapsed ? ChevronUp : ChevronDown}
          label={tab.collapsed ? texts.expand : texts.collapse} onClick={() => store().toggleCollapsed(tabId)} />
      </header>
      {tab.collapsed ? null : (
        <div className={styles.list}>
          {rows.length === 0 ? <p className={styles.empty}>{texts.empty}</p> : null}
          {rows.map((job) => {
            const speed = job.state === "running" ? speedOf(tab.samples[job.id] ?? [], now) : null;
            const percent = job.size > 0 ? Math.floor((job.done * 100) / job.size) : job.state === "done" ? 100 : 0;
            return (
              <div key={job.id} className={styles.row}>
                <span aria-hidden className={job.direction === "upload" ? styles.up : styles.down}>
                  {job.direction === "upload" ? "↑" : "↓"}
                </span>
                <span className={styles.name} title={job.source}>{job.rel}</span>
                <span className={cx(styles.target, "mono")} title={job.target}>{parentDir(job.target)}</span>
                <ProgressBar value={percent} tone={job.state === "failed" ? "red" : job.state === "done" ? "green" : "accent"} label={job.rel} />
                <span className={cx(styles.speed, "mono")}>{speed === null ? "—" : texts.speed(formatSize(Math.round(speed)))}</span>
                <span className={styles.state}>{stateText(job)}</span>
                {viewOf(job.state) === "queue" ? (
                  <IconButton size="sm" icon={X} label={texts.cancel(job.rel)} onClick={() => store().cancel(tabId, { type: "job", id: job.id })} />
                ) : job.state === "failed" ? (
                  <IconButton size="sm" icon={RotateCcw} label={texts.retry(job.rel)} onClick={() => store().retry(tabId, job.id)} />
                ) : <span />}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
```

Check the `ProgressBar` tone names in `ui/src/components/tone.ts` and use the existing ones (e.g. `"amber"` if there is no `"accent"`); check that `ProgressBar` renders `role="progressbar"` with `aria-valuenow` (the test relies on it; if it does not, assert on the text "64 %" instead).

`TransferPanel.module.css`:

```css
.panel {
  border-top: 1px solid var(--bd);
  background: var(--s1);
  max-height: 30vh;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
}

.views {
  display: flex;
  gap: 4px;
}

.view {
  background: none;
  border: 1px solid transparent;
  border-radius: 6px;
  color: var(--text2);
  font: inherit;
  padding: 2px 8px;
  cursor: pointer;
}

.activeView {
  border-color: var(--bd);
  background: var(--bg);
  color: var(--text);
}

.summary {
  color: var(--text2);
  font-size: 12px;
}

.spacer {
  flex: 1;
}

.list {
  overflow: auto;
  padding: 0 8px 6px;
}

.row {
  display: grid;
  grid-template-columns: 16px minmax(120px, 2fr) minmax(100px, 2fr) minmax(80px, 1fr) 80px 120px 28px;
  align-items: center;
  gap: 8px;
  padding: 3px 0;
  font-size: 13px;
}

.name,
.target {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.target {
  color: var(--text2);
  font-size: 12px;
}

.speed {
  color: var(--text3);
  font-size: 12px;
  text-align: right;
}

.state {
  font-size: 12px;
  color: var(--text2);
}

.up {
  color: var(--accent);
}

.down {
  color: var(--green);
}

.empty {
  color: var(--muted);
  font-size: 12px;
}
```

Use only tokens that exist in the theme (`grep -n -- "--text3\|--accent\|--green" ui/src/styles` – replace a missing one with the nearest existing token).

`ConflictDialog.tsx`:

```tsx
import { useRef, useState } from "react";
import { Button, Checkbox, Dialog } from "../../components";
import { t } from "../../i18n/de";
import type { Decision } from "../../ipc/api";
import { useTransfersStore } from "../../stores/transfers";
import { formatMtime, formatSize } from "./entries";
import styles from "./SftpScreen.module.css";

/** One open conflict of the tab ("Datei existiert bereits"). Escape cancels the whole batch. */
export function ConflictDialog({ tabId }: { tabId: string }) {
  const conflict = useTransfersStore((s) => s.byTab[tabId]?.conflicts[0]);
  const job = useTransfersStore((s) => (conflict ? s.byTab[tabId]?.jobs[conflict.jobId] : undefined));
  const [all, setAll] = useState(false);
  const skipRef = useRef<HTMLButtonElement>(null);
  if (!conflict || !job) return null;
  const texts = t.sftp.conflict;
  const [newSide, oldSide] = job.direction === "upload" ? [t.sftp.local, t.sftp.remote] : [t.sftp.remote, t.sftp.local];
  const answer = (decision: Decision) => {
    setAll(false);
    void useTransfersStore.getState().resolve(tabId, decision, decision === "cancelBatch" ? false : all);
  };
  const name = job.rel.split("/").pop() ?? job.rel;
  return (
    <Dialog
      open
      size="sm"
      title={texts.title}
      onClose={() => answer("cancelBatch")}
      initialFocusRef={skipRef}
      footer={
        <>
          <Button variant="ghost" onClick={() => answer("cancelBatch")}>{t.common.cancel}</Button>
          <span className={styles.footerGap} />
          <Button ref={skipRef} onClick={() => answer("skip")}>{texts.skip}</Button>
          <Button onClick={() => answer("newer")}>{texts.newer}</Button>
          <Button variant="primary" onClick={() => answer("overwrite")}>{texts.overwrite}</Button>
        </>
      }
    >
      <p>{texts.text(name)}</p>
      <div className={styles.compare}>
        <div>
          <span>{texts.newSide(newSide)}</span>
          <span className="mono">{formatSize(conflict.source.size)}</span>
          <span className="mono">{formatMtime(conflict.source.mtime)}</span>
        </div>
        <div>
          <span>{texts.existingSide(oldSide)}</span>
          <span className="mono">{formatSize(conflict.target.size)}</span>
          <span className="mono">{formatMtime(conflict.target.mtime)}</span>
        </div>
      </div>
      <Checkbox label={texts.applyToAll} checked={all} onChange={setAll} />
    </Dialog>
  );
}
```

If `Dialog`'s `onClose` is not wired to Escape, check `ui/src/components/Dialog/Dialog.tsx` (it is for the other dialogs – the delete dialog test relies on it). `formatMtime` takes the `mtime` shape `Entry.mtime` uses (`number | null`); `FileStamp.mtime` matches.

`SftpScreen.module.css` additions:

```css
.compare {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  margin: 12px 0;
}

.compare > div {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px;
  border: 1px solid var(--bd);
  border-radius: 8px;
}

.footerGap {
  flex: 1;
}

.pendingDot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--amber);
}
```

`CloseTabDialog.tsx`:

```tsx
import { useRef } from "react";
import { Button, Dialog } from "../../components";
import { t } from "../../i18n/de";

export function CloseTabDialog({ count, onConfirm, onClose }: { count: number; onConfirm: () => void; onClose: () => void }) {
  const texts = t.sftp.closeDialog;
  const cancelRef = useRef<HTMLButtonElement>(null);
  return (
    <Dialog
      open
      size="sm"
      role="alertdialog"
      title={texts.title}
      onClose={onClose}
      initialFocusRef={cancelRef}
      footer={
        <>
          <Button ref={cancelRef} variant="ghost" onClick={onClose}>{t.common.cancel}</Button>
          <Button variant="danger" onClick={() => { onConfirm(); onClose(); }}>{texts.confirm}</Button>
        </>
      }
    >
      <p>{texts.text(count)}</p>
    </Dialog>
  );
}
```

`SftpScreen.tsx`:
- `const byTab = useTransfersStore((s) => s.byTab);`
- Tab strip: the tab button gets `aria-label={pending ? `${x.title} · ${t.sftp.pendingConflict}` : undefined}` and renders `<span className={styles.pendingDot} aria-hidden />` when `x.id !== tab.id && (byTab[x.id]?.conflicts.length ?? 0) > 0`.
- Close button: `onClick={() => requestClose(x.id)}` with

```tsx
  const [closing, setClosing] = useState<{ tabId: string; count: number } | null>(null);
  const requestClose = (tabId: string) => {
    const count = openCount(useTransfersStore.getState().byTab[tabId]);
    if (count > 0) setClosing({ tabId, count });
    else store().closeTab(tabId);
  };
```

  and `{closing ? <CloseTabDialog count={closing.count} onConfirm={() => store().closeTab(closing.tabId)} onClose={() => setClosing(null)} /> : null}`.
- Remote pane container: before the lost banner, `{tab.status === "reconnecting" && tab.reconnect ? <ReconnectBar state={tab.reconnect} onNow={() => fireAndForget(() => sftpReconnectNow(tab.sessionId))} onStop={() => fireAndForget(() => sftpStopReconnect(tab.sessionId))} /> : null}`; the `FilePane` for remote gets `disabled={tab.status !== "connected"}`.
- After `.panes`, before the footer: `<TransferPanel tabId={tab.id} />`; at the end next to the other dialogs: `<ConflictDialog tabId={tab.id} />`.

- [ ] **Step 5: Run the tests**

Run: `bun run typecheck && bun run test`
Expected: PASS, no warnings (watch for act() warnings from the 1 s ReconnectBar interval – the existing tests use fake timers or unmount; follow their pattern).

- [ ] **Step 6: Commit**

```bash
git add ui/src
git commit -m "feat(ui): transfer panel, conflict dialog, SFTP reconnect bar and close confirmation

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Docs and the full check

**Files:**
- Create: `docs/acceptance-tp3b.md`
- Modify: `CLAUDE.md`, `docs/backlog-after-tp1.md` (only if items are deferred during execution)

- [ ] **Step 1: Write `docs/acceptance-tp3b.md`**

```markdown
# Abnahme TP3b – SFTP-Übertragungen

Voraussetzung: ein SFTP-Host mit gespeichertem Passwort oder Schlüssel, eine Datei > 500 MB lokal, ein Ordnerbaum
mit Unterordnern, einem Symlink auf einen Ordner und einem kaputten Symlink.

## Auslösen
- [ ] F5 im lokalen Panel lädt die Auswahl in den Remote-Ordner hoch; F5 im Remote-Panel lädt herunter.
- [ ] Kontextmenü „Hochladen“ / „Herunterladen“ und der Toolbar-Knopf tun dasselbe.
- [ ] Einträge per Maus ins andere Panel ziehen: Drop auf einen Ordner überträgt in diesen Ordner, sonst in den aktuellen.
- [ ] Ein kurzer Klick startet nichts.
- [ ] Dateien aus Nautilus auf das Remote-Panel ziehen: Rahmen erscheint, Upload startet. Über dem lokalen Panel passiert nichts.

## Warteschlange
- [ ] Fortschritt, Geschwindigkeit und Status laufen; höchstens „Parallele Übertragungen“ Dateien gleichzeitig.
- [ ] Während des Uploads liegt remote nur `name.filepart`, danach nur `name`.
- [ ] Abbrechen einer Zeile entfernt die `.filepart`; fertige Dateien bleiben.
- [ ] Symlink auf Ordner und kaputter Link stehen unter „Abgeschlossen“ als übersprungen.
- [ ] Eine nicht lesbare Datei steht unter „Fehler“ mit „Keine Berechtigung“; „Wiederholen“ nach `chmod` klappt.
- [ ] „Leeren“ räumt „Abgeschlossen“ auf. Einklappen zeigt die Zusammenfassung. Ohne Jobs ist das Panel weg.
- [ ] Nach dem Ende lädt das Ziel-Panel neu.

## Konflikte
- [ ] Erneutes Hochladen derselben Datei zeigt „Datei existiert bereits“ mit Größe und Datum beider Seiten.
- [ ] Überspringen, Nur wenn neuer, Überschreiben tun das Erwartete; Esc bricht den ganzen Auftrag ab.
- [ ] „Für alle weiteren Konflikte übernehmen“ beantwortet die restlichen Konflikte des Auftrags ohne Dialog.
- [ ] Ein Konflikt in einem Hintergrund-Tab zeigt einen Punkt am Reiter, kein Dialog.

## Rechte
- [ ] Host mit „Rechte beim Upload“ = 0644: Dateien 0644, neue Ordner 0755.
- [ ] Ohne Einstellung: die lokalen Rechte werden übernommen.
- [ ] Download einer Datei mit 0600 bleibt 0600.

## Wiederverbinden
- [ ] Während eines großen Uploads WLAN aus: Leiste „Verbindung unterbrochen · Versuch n von 8“, Job „Unterbrochen“.
- [ ] WLAN an: die Übertragung macht weiter (nicht von vorn), die Datei ist identisch (`sha256sum`).
- [ ] Auto-Reconnect in den Einstellungen aus: sofort „Verbindung verloren“, „Neu verbinden“ setzt die Übertragung fort.
- [ ] Host ohne gespeichertes Passwort: nach dem Abbruch „Verbindung verloren“, „Neu verbinden“ fragt nach dem Passwort und setzt fort.

## Tabs
- [ ] Tab mit laufenden Übertragungen schließen fragt nach; „Schließen“ bricht ab und hinterlässt keine `.filepart`.
```

- [ ] **Step 2: Update `CLAUDE.md`**

- Under the spec list: `` - `docs/superpowers/specs/2026-10-15-linascp-tp3b-transfers-design.md`: TP3b spec (transfers, queue, conflicts, SFTP reconnect). ``
- Acceptance line: add `docs/acceptance-tp3b.md` for TP3b.
- Layout bullet for core: add `` - `transfer/`: transfer queue (TP3b) over the `Side` trait (local fs, `SftpSession`). ``
- Roadmap: `TP3: SFTP (3a browse done; 3b transfers done; 3c bookmarks + edit).`
- Docker suite count stays implicit.

- [ ] **Step 3: Run the full check and the Docker suite**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bun run typecheck && bun run test`
Expected: all green, no warnings.

Run: `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`
Expected: 26/26 pass.

- [ ] **Step 4: Commit**

```bash
git add docs/acceptance-tp3b.md CLAUDE.md
git commit -m "docs: TP3b acceptance checklist and project notes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
