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
    Interrupted {
        resume_at: u64,
        stamp: Option<FileStamp>,
    },
    /// The connection broke; `used` is the remote the job ran on.
    Paused {
        resume_at: u64,
        stamp: Option<FileStamp>,
        used: Arc<dyn Side>,
    },
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
        st.batches
            .insert(batch_id, Batch { direction, target_dir: target_dir.clone(), default: None, done_sent: false });
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
            Outcome::Interrupted { resume_at, stamp } => {
                entry.resume_at = resume_at;
                entry.stamp = stamp;
                entry.job.done = resume_at;
                entry.job.state = JobState::Paused;
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

async fn run_file(
    inner: &Inner,
    snap: &Snapshot,
    src: &dyn Side,
    dst: &dyn Side,
    cancel: &CancellationToken,
) -> Outcome {
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
        assert_eq!(
            meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            mtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
        );
        assert!(!e.remote.join("a.txt.filepart").exists());
        let job = e.rec.job_by_rel("a.txt");
        assert_eq!((job.state, job.size, job.done, job.batch_id), (JobState::Done, 5, 5, batch));
        assert!(
            e.rec
                .events()
                .iter()
                .any(|ev| matches!(ev, TransferEvent::BatchDone { batch_id, .. } if *batch_id == batch))
        );
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
                if job.state == JobState::Running {
                    running.insert(job.id);
                } else {
                    running.remove(&job.id);
                }
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
                matches!(
                    e.queue.enqueue(Direction::Upload, sources.clone(), target).await,
                    Err(CoreError::Config { .. })
                ),
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
        let states: Vec<_> = e
            .rec
            .events()
            .into_iter()
            .filter_map(|ev| match ev {
                TransferEvent::Job { job } => Some(job.state),
                _ => None,
            })
            .collect();
        assert_eq!(states, vec![JobState::Queued, JobState::Running, JobState::Running, JobState::Done]);
    }
}
