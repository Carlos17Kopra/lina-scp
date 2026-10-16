//! Editing remote files in a local editor (TP3c): download into a private directory, watch the
//! copy and upload every saved state through the transfer queue, unless the server's file changed
//! since the last known state.

use std::collections::HashMap;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::task::AbortHandle;

use crate::connect::network_error;
use crate::error::{CoreError, FsKind, NetworkKind, Result};
use crate::transfer::{Decision, Direction, EnqueueOptions, FileStamp, Job, JobState, Node, NodeKind, TransferQueue};
use crate::validate::validate_file_name;

pub type EditId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum EditState {
    Downloading,
    /// The server has the content of the local copy.
    Synced,
    /// Saved locally, not uploaded yet.
    Changed,
    Uploading,
    /// The server's file changed since it was opened; answer with `EditManager::resolve`.
    Conflict,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditFile {
    pub id: EditId,
    pub name: String,
    /// Canonical path on the server (symlinks resolved).
    pub remote_path: String,
    pub state: EditState,
    /// The local copy holds a state the server does not have.
    pub dirty: bool,
    pub error: Option<CoreError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EditEvent {
    /// Full file on creation and on every change.
    File { file: EditFile },
}

/// Opens a file in the user's editor, or in `app` (a desktop id) when one was chosen
/// (implemented outside the core).
pub trait Launcher: Send + Sync {
    fn open(&self, path: &Path, app: Option<&str>) -> Result<()>;
}

pub type EditEmit = Arc<dyn Fn(EditEvent) + Send + Sync>;

/// Size and mtime of the local copy, by path (editors often save by renaming a new file over it).
type LocalStamp = (u64, SystemTime);

fn local_stamp(path: &Path) -> Option<LocalStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

fn stamp(node: &Node) -> FileStamp {
    FileStamp { size: node.size, mtime: node.mtime }
}

fn make_private_dir(path: &Path) -> Result<()> {
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn utf8(path: &Path) -> Result<String> {
    path.to_str().map(str::to_string).ok_or_else(|| CoreError::config("path is not valid UTF-8"))
}

fn unknown(id: EditId) -> CoreError {
    CoreError::internal(format!("unknown edit {id}"))
}

fn job_error(job: Option<&Job>, what: &str) -> CoreError {
    match job {
        Some(job) if job.state == JobState::Cancelled => CoreError::Cancelled,
        Some(job) => job.error.clone().unwrap_or_else(|| CoreError::internal(format!("{what} did not finish"))),
        None => CoreError::internal(format!("{what} did not start")),
    }
}

struct FileState {
    file: EditFile,
    dir: PathBuf,
    copy: PathBuf,
    /// Server stamp after the last download or upload: anything else there is someone else's change.
    remote: Option<FileStamp>,
    remote_mode: u32,
    /// Local stamp of the content the server has.
    synced: Option<LocalStamp>,
    /// Do not upload this local state again (declined conflict, failed upload).
    hold: Option<LocalStamp>,
    decision: Option<bool>,
    decided: Arc<Notify>,
    watch: Option<AbortHandle>,
    /// The app of the last open; `reopen` uses it again.
    app: Option<String>,
}

impl FileState {
    /// A downloaded copy that is missing counts as dirty: editors move it away while saving.
    fn dirty(&self) -> bool {
        match local_stamp(&self.copy) {
            Some(now) => Some(now) != self.synced,
            None => self.synced.is_some(),
        }
    }
}

struct Inner {
    queue: TransferQueue,
    root: PathBuf,
    launcher: Arc<dyn Launcher>,
    emit: EditEmit,
    poll: Duration,
    files: Mutex<HashMap<EditId, FileState>>,
    next_id: AtomicU64,
}

/// The files being edited in one SFTP session.
#[derive(Clone)]
pub struct EditManager {
    inner: Arc<Inner>,
}

impl EditManager {
    pub fn new(
        queue: TransferQueue,
        root: PathBuf,
        launcher: Arc<dyn Launcher>,
        emit: EditEmit,
        poll: Duration,
    ) -> Self {
        let inner = Inner { queue, root, launcher, emit, poll, files: Mutex::default(), next_id: AtomicU64::new(1) };
        Self { inner: Arc::new(inner) }
    }

    /// Downloads the file (once per canonical path), starts watching the copy and opens the editor.
    /// A launcher error is returned after the watch started: `reopen` can try again.
    pub async fn open(&self, remote_path: &str) -> Result<EditId> {
        self.open_with(remote_path, None).await
    }

    /// `open` in `app` instead of the editor; `reopen` keeps using it.
    pub async fn open_with(&self, remote_path: &str, app: Option<String>) -> Result<EditId> {
        let inner = &self.inner;
        let remote = inner.queue.remote().ok_or_else(|| network_error(NetworkKind::Other, "not connected"))?;
        let real = remote.realpath(remote_path).await?;
        let node = remote.stat(&real).await?.ok_or_else(|| CoreError::fs(FsKind::NotFound, &real, "not found"))?;
        if node.kind != NodeKind::File {
            return Err(CoreError::fs(FsKind::Other, &real, "not a file"));
        }
        let existing = inner
            .lock()
            .iter()
            .find(|(_, f)| f.file.remote_path == real)
            .map(|(id, f)| (*id, f.copy.clone(), f.file.state, f.synced.is_some()));
        if let Some((id, ..)) = existing
            && let Some(file) = inner.lock().get_mut(&id)
        {
            file.app = app.clone();
        }
        let mut reuse = None;
        match existing {
            // The first `open` launches the editor once the copy exists.
            Some((id, _, EditState::Downloading, _)) => return Ok(id),
            // The download failed: start over under the same id.
            Some((id, _, EditState::Failed, false)) => {
                inner.lock().remove(&id);
                reuse = Some(id);
            }
            Some((id, copy, _, _)) => {
                inner.launcher.open(&copy, app.as_deref())?;
                return Ok(id);
            }
            None => {}
        }
        let name = real.rsplit('/').next().unwrap_or_default().to_string();
        validate_file_name(&name)?;
        make_private_dir(&inner.root)?;
        let dir = inner.root.join(uuid::Uuid::new_v4().to_string());
        make_private_dir(&dir)?;
        let copy = dir.join(&name);
        let id = reuse.unwrap_or_else(|| inner.next_id.fetch_add(1, Ordering::SeqCst));
        let file =
            EditFile { id, name, remote_path: real.clone(), state: EditState::Downloading, dirty: false, error: None };
        (inner.emit)(EditEvent::File { file: file.clone() });
        inner.lock().insert(
            id,
            FileState {
                file,
                dir: dir.clone(),
                copy: copy.clone(),
                remote: Some(stamp(&node)),
                // ponytail: a server that sends no mode gets the common 0644, never 000.
                remote_mode: node.mode.unwrap_or(0o644) & 0o777,
                synced: None,
                hold: None,
                decision: None,
                decided: Arc::new(Notify::new()),
                watch: None,
                app: app.clone(),
            },
        );

        let jobs = match inner
            .queue
            .enqueue_with(Direction::Download, vec![real], utf8(&dir)?, EnqueueOptions::default())
            .await
        {
            Ok(batch) => inner.queue.wait_batch(batch).await,
            Err(error) => return Err(inner.fail_download(id, &dir, error)),
        };
        if jobs.first().map(|j| j.state) != Some(JobState::Done) {
            return Err(inner.fail_download(id, &dir, job_error(jobs.first(), "download")));
        }
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode((node.mode.unwrap_or(0) & 0o666) | 0o600))?;
        let synced = local_stamp(&copy);
        let watch = tokio::spawn(watch(inner.clone(), id, synced)).abort_handle();
        inner.update(id, |f| {
            f.synced = synced;
            f.watch = Some(watch);
            f.file.state = EditState::Synced;
        });
        let app = inner.lock().get(&id).and_then(|f| f.app.clone());
        inner.launcher.open(&copy, app.as_deref())?;
        Ok(id)
    }

    /// Answer to a `Conflict`: `true` uploads the local copy over the server's change.
    pub fn resolve(&self, id: EditId, overwrite: bool) -> Result<()> {
        let mut files = self.inner.lock();
        let file = files.get_mut(&id).ok_or_else(|| unknown(id))?;
        if file.file.state != EditState::Conflict {
            return Err(CoreError::internal(format!("edit {id} has no conflict")));
        }
        file.decision = Some(overwrite);
        file.decided.notify_one();
        Ok(())
    }

    pub fn reopen(&self, id: EditId) -> Result<()> {
        let (copy, app) =
            self.inner.lock().get(&id).map(|f| (f.copy.clone(), f.app.clone())).ok_or_else(|| unknown(id))?;
        self.inner.launcher.open(&copy, app.as_deref())
    }

    /// Stops watching. The copy is deleted unless it holds changes the server does not have;
    /// then its path is returned and it stays.
    pub async fn stop(&self, id: EditId) -> Result<Option<PathBuf>> {
        let file = self.inner.lock().remove(&id).ok_or_else(|| unknown(id))?;
        if let Some(watch) = &file.watch {
            watch.abort();
        }
        if file.dirty() {
            return Ok(Some(file.copy));
        }
        let _ = tokio::fs::remove_dir_all(&file.dir).await;
        Ok(None)
    }

    /// `stop` for every file; returns the copies that were kept.
    pub async fn close(&self) -> Vec<PathBuf> {
        let ids: Vec<EditId> = self.inner.lock().keys().copied().collect();
        let mut kept = Vec::new();
        for id in ids {
            if let Ok(Some(path)) = self.stop(id).await {
                kept.push(path);
            }
        }
        kept
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, HashMap<EditId, FileState>> {
        self.files.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Changes the file and emits it; `false` when it is gone (stopped).
    fn update(&self, id: EditId, change: impl FnOnce(&mut FileState)) -> bool {
        let mut files = self.lock();
        let Some(file) = files.get_mut(&id) else { return false };
        change(file);
        (self.emit)(EditEvent::File { file: file.file.clone() });
        true
    }

    fn fail_download(&self, id: EditId, dir: &Path, error: CoreError) -> CoreError {
        let _ = std::fs::remove_dir_all(dir);
        self.update(id, |f| {
            f.file.state = EditState::Failed;
            f.file.error = Some(error.clone());
        });
        error
    }

    /// Uploads the local state `now` unless the server's file changed meanwhile and the user keeps it.
    async fn upload(&self, id: EditId, now: LocalStamp) {
        // Not connected: the next tick tries again.
        let Some(remote) = self.queue.remote() else { return };
        let Some((real, base, mode, copy, decided)) = self
            .lock()
            .get(&id)
            .map(|f| (f.file.remote_path.clone(), f.remote, f.remote_mode, f.copy.clone(), f.decided.clone()))
        else {
            return;
        };
        let Ok(node) = remote.stat(&real).await else { return };
        if let Some(node) = &node
            && Some(stamp(node)) != base
        {
            self.update(id, |f| {
                f.file.state = EditState::Conflict;
                f.decision = None;
            });
            decided.notified().await;
            let overwrite = self.lock().get_mut(&id).and_then(|f| f.decision.take()).unwrap_or(false);
            if !overwrite {
                self.update(id, |f| {
                    f.hold = Some(now);
                    f.file.state = EditState::Changed;
                });
                return;
            }
        }
        // Editing never changes rights: the server's current mode, or the one it had (re-created file).
        let mode = node.and_then(|n| n.mode).map(|m| m & 0o777).unwrap_or(mode);
        self.update(id, |f| {
            f.file.state = EditState::Uploading;
            f.file.error = None;
        });
        let parent = match real.rsplit_once('/') {
            Some(("", _)) | None => "/".to_string(),
            Some((parent, _)) => parent.to_string(),
        };
        let options = EnqueueOptions { decision: Some(Decision::Overwrite), file_mode: Some(mode) };
        let jobs = match utf8(&copy) {
            Ok(source) => match self.queue.enqueue_with(Direction::Upload, vec![source], parent, options).await {
                Ok(batch) => self.queue.wait_batch(batch).await,
                Err(error) => return self.fail_upload(id, now, error),
            },
            Err(error) => return self.fail_upload(id, now, error),
        };
        if jobs.first().map(|j| j.state) != Some(JobState::Done) {
            return self.fail_upload(id, now, job_error(jobs.first(), "upload"));
        }
        let after = match self.queue.remote() {
            Some(remote) => remote.stat(&real).await.ok().flatten().map(|n| stamp(&n)),
            None => None,
        };
        self.update(id, |f| {
            f.synced = Some(now);
            f.remote = after.or(f.remote);
            f.remote_mode = mode;
            f.hold = None;
            f.file.dirty = f.dirty();
            f.file.state = if f.file.dirty { EditState::Changed } else { EditState::Synced };
        });
    }

    fn fail_upload(&self, id: EditId, now: LocalStamp, error: CoreError) {
        self.update(id, |f| {
            f.hold = Some(now);
            f.file.state = EditState::Failed;
            f.file.error = Some(error);
        });
    }
}

/// Polls the copy; a changed state that stayed the same for one more tick is uploaded.
async fn watch(inner: Arc<Inner>, id: EditId, mut seen: Option<LocalStamp>) {
    loop {
        tokio::time::sleep(inner.poll).await;
        let Some((copy, synced, hold, state)) =
            inner.lock().get(&id).map(|f| (f.copy.clone(), f.synced, f.hold, f.file.state))
        else {
            return;
        };
        let now = local_stamp(&copy);
        if now != seen {
            seen = now;
            if now.is_some() && now != synced && state != EditState::Changed {
                inner.update(id, |f| {
                    f.file.state = EditState::Changed;
                    f.file.dirty = true;
                    f.file.error = None;
                });
            }
            continue;
        }
        match now {
            Some(now) if Some(now) != synced && Some(now) != hold => inner.upload(id, now).await,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
