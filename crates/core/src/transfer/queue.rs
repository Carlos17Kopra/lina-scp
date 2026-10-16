//! The per-session transfer engine (TP3b spec §1): FIFO jobs, at most `parallel` files at once,
//! conflicts one at a time, pause on connection loss and resume after the reconnect.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use futures::{StreamExt, stream};
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

use super::copy::{IN_FLIGHT, copy_range};
use super::side::{Node, NodeKind, Side};
use super::{
    BatchId, CancelTarget, Decision, Direction, EnqueueOptions, FileStamp, Job, JobId, JobKind, JobNote, JobState,
    TransferEvent, TransferOptions,
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
    file_mode: Option<u32>,
    retryable: bool,
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
    /// Signalled when a batch may have become final, and on close (`wait_batch`).
    settled: Notify,
    /// Chunk requests in flight across all files of the session.
    in_flight: Semaphore,
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

fn unknown(what: &str, id: u64) -> CoreError {
    CoreError::internal(format!("unknown {what} {id}"))
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
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(state),
                local,
                options,
                emit,
                idle: Notify::new(),
                settled: Notify::new(),
                in_flight: Semaphore::new(IN_FLIGHT),
            }),
        }
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

    /// The current connection; `None` while disconnected.
    pub fn remote(&self) -> Option<Arc<dyn Side>> {
        self.inner.lock().remote.clone()
    }

    pub async fn enqueue(&self, direction: Direction, sources: Vec<String>, target_dir: String) -> Result<BatchId> {
        self.enqueue_with(direction, sources, target_dir, EnqueueOptions::default()).await
    }

    /// Waits until every job of the batch is final (or the queue is closed) and returns them.
    /// An unknown or cleared batch returns at once with what is left of it.
    pub async fn wait_batch(&self, id: BatchId) -> Vec<Job> {
        loop {
            let settled = self.inner.settled.notified();
            tokio::pin!(settled);
            settled.as_mut().enable();
            {
                let st = self.inner.lock();
                let jobs: Vec<Job> = st.jobs.iter().filter(|e| e.job.batch_id == id).map(|e| e.job.clone()).collect();
                if st.closed || !st.batches.contains_key(&id) || jobs.iter().all(|j| j.state.is_final()) {
                    return jobs;
                }
            }
            settled.await;
        }
    }

    pub async fn enqueue_with(
        &self,
        direction: Direction,
        sources: Vec<String>,
        target_dir: String,
        options: EnqueueOptions,
    ) -> Result<BatchId> {
        if self.inner.lock().closed {
            return Err(CoreError::internal("transfer queue is closed"));
        }
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
        // Owned paths: a closure over `&String` makes the future non-`Send` for Tauri commands.
        let nodes: Vec<Result<Option<Node>>> = stream::iter(sources.clone())
            .map(|s| {
                let src = src.clone();
                async move { src.stat(&s).await }
            })
            .buffered(STAT_CONCURRENCY)
            .collect()
            .await;
        let mut st = self.inner.lock();
        let batch_id = st.next_id;
        st.next_id += 1;
        st.batches.insert(
            batch_id,
            Batch {
                direction,
                target_dir: target_dir.clone(),
                default: options.decision,
                file_mode: options.file_mode.map(|m| m & 0o7777),
                retryable: options == EnqueueOptions::default(),
                done_sent: false,
            },
        );
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
            entry.conflict = None;
            if decision == Decision::Skip {
                entry.job.state = JobState::Skipped;
            } else {
                // `Newer` is checked by the worker against the target as it is now, not as it was
                // when the dialog opened.
                entry.decided = Some(decision);
                entry.job.state = JobState::Queued;
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
        if !st.jobs[index].job.retryable {
            return Err(CoreError::internal(format!("job {id} cannot be retried")));
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
        self.inner.settled.notify_waiters();
        self.inner.remove_parts(parts).await;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let idle = self.inner.idle.notified();
            // Cancelled workers keep their token until `finish`: wait for them to remove their parts.
            if !self.inner.lock().jobs.iter().any(|e| e.cancel.is_some()) {
                break;
            }
            if tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), idle).await.is_err() {
                break;
            }
        }
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
            retryable: st.batches.get(&batch_id).is_none_or(|b| b.retryable),
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
        if all_final {
            self.settled.notify_waiters();
        }
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
        let mut dead = false;
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
                // The connection it ran on is gone: start nothing more on it until `set_remote`
                // brings a new one (queued jobs stay queued instead of failing into `Paused`).
                dead = remote.as_ref().is_some_and(|r| same_side(r, &used));
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
        if dead {
            st.remote = None;
        }
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

    /// Marks matching unfinished jobs `Cancelled` and stops running ones (their worker removes its
    /// part). Returns the parts of jobs that are not running, which no worker owns.
    fn cancel_where(&self, st: &mut State, matches: impl Fn(&Job) -> bool) -> Vec<(Direction, String)> {
        let closed = st.closed;
        let mut parts = Vec::new();
        let mut reopen = false;
        for entry in st.jobs.iter_mut().filter(|e| !e.job.state.is_final() && matches(&e.job)) {
            // A paused or re-queued job owns its part; a running one removes it in its worker.
            if entry.job.state != JobState::Running && entry.resume_at > 0 {
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
}

/// Reads what a worker needs from its entry.
struct Snapshot {
    job: Job,
    resume_at: u64,
    stamp: Option<FileStamp>,
    decision: Option<Decision>,
    file_mode: Option<u32>,
}

async fn run_job(inner: Arc<Inner>, id: JobId, remote: Arc<dyn Side>, cancel: CancellationToken) {
    let snapshot = {
        let mut st = inner.lock();
        let Some(entry) = st.jobs.iter().find(|e| e.job.id == id) else { return };
        let batch = st.batches.get(&entry.job.batch_id);
        let snap = Snapshot {
            job: entry.job.clone(),
            resume_at: entry.resume_at,
            stamp: entry.stamp,
            decision: entry.decided.or(batch.and_then(|b| b.default)),
            file_mode: batch.and_then(|b| b.file_mode),
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
        Outcome::Failed(CoreError::Network { .. }) => {
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
            // Names come from the server: `../x` or `a/b` must never be joined into a path.
            let usable = !child.lossy && validate_file_name(&child.name).is_ok();
            let node = if usable { Some(src.stat(&join(&job.source, &child.name)).await) } else { None };
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
            None => Err(CoreError::config(format!("invalid file name: {:?}", child.name))),
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
    let mut resume = if snap.resume_at > 0 && snap.stamp == Some(now) { snap.resume_at } else { 0 };
    let part = format!("{}.filepart", job.target);
    if resume > 0 {
        // The part may have vanished or shrunk while paused: never write past what is there.
        resume = match dst.stat(&part).await {
            Ok(Some(n)) if n.kind == NodeKind::File => resume.min(n.size),
            Ok(_) => 0,
            Err(error) => return Outcome::Failed(error),
        };
    }
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
        let copied = copy_range(&*reader, &*writer, resume, node.size, &inner.in_flight, cancel, &mut progress).await;
        reader.close().await;
        let closed = writer.close().await;
        let done = copied?;
        closed.map_err(|e| (e, done))?;
        let mode = snap.file_mode.unwrap_or_else(|| inner.options.file_mode(job.direction, node.mode));
        dst.set_mode(&part, mode).await.map_err(|e| (e, done))?;
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
    fn conflicts(rec: &Recorder) -> Vec<JobId> {
        rec.events()
            .into_iter()
            .filter_map(|e| match e {
                TransferEvent::Conflict { job_id, .. } => Some(job_id),
                _ => None,
            })
            .collect()
    }

    /// One file at a time, so `a` reaches its conflict before `b` (with more workers, conflicts
    /// open in the order the workers find them).
    fn serial() -> TransferOptions {
        TransferOptions { parallel: 1, upload_mode: None }
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
        e.rec
            .wait("both waiting", |r| r.jobs().values().filter(|j| j.state == JobState::WaitingDecision).count() == 2)
            .await;
    }

    #[tokio::test]
    async fn conflicts_open_one_at_a_time_and_others_keep_running() {
        let e = env(serial());
        two_conflicts(&e).await;
        std::fs::write(e.local.join("c"), b"c").unwrap();
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("c"))], p(&e.remote)).await.unwrap();
        e.rec.wait("c done", |r| r.job_by_rel("c").state == JobState::Done).await;
        assert_eq!(conflicts(&e.rec).len(), 1);
        let a = e.rec.job_by_rel("a");
        assert_eq!(conflicts(&e.rec), vec![a.id]);
        let TransferEvent::Conflict { source, target, .. } =
            e.rec.events().into_iter().find(|ev| matches!(ev, TransferEvent::Conflict { .. })).unwrap()
        else {
            unreachable!()
        };
        assert_eq!((source.size, target.size, target.mtime), (4, 3, Some(1_000)));
        e.queue.resolve(a.id, Decision::Skip, false).unwrap();
        e.rec.wait("second conflict", |r| conflicts(r).len() == 2).await;
        assert_eq!(e.rec.job_by_rel("a").state, JobState::Skipped);
        assert_eq!(std::fs::read(e.remote.join("a")).unwrap(), b"old");
    }

    #[tokio::test]
    async fn overwrite_and_apply_to_all_resolve_the_waiting_ones_too() {
        let e = env(serial());
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
        let e = env(serial());
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
        let e = env(serial());
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
        let e = env(serial());
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
        let e = env(serial());
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

    #[tokio::test]
    async fn hostile_child_names_never_reach_the_file_system() {
        let (e, faulty) = faulty_env(opts());
        std::fs::create_dir_all(e.remote.join("d")).unwrap();
        std::fs::write(e.remote.join("d/ok"), b"1").unwrap();
        std::fs::write(e.remote.join("evil"), b"x").unwrap();
        faulty.add_fake_children(&["../evil", "a\u{FFFD}", "sub/x"]);
        e.queue.enqueue(Direction::Download, vec![p(&e.remote.join("d"))], p(&e.local)).await.unwrap();
        e.rec.wait_all_final().await;
        assert_eq!(std::fs::read(e.local.join("d/ok")).unwrap(), b"1");
        assert!(!e.local.join("evil").exists() && !e.local.join("evil.filepart").exists());
        assert_eq!(std::fs::read_dir(e.local.join("d")).unwrap().count(), 1);
        let bad: Vec<_> = e.rec.jobs().into_values().filter(|j| j.rel != "d" && j.rel != "d/ok").collect();
        assert_eq!(bad.len(), 3);
        assert!(
            bad.iter().all(|j| j.state == JobState::Failed && matches!(j.error, Some(CoreError::Config { .. }))),
            "{bad:?}"
        );
    }

    #[tokio::test]
    async fn a_part_that_vanished_while_paused_starts_over() {
        let (e, faulty) = faulty_env(opts());
        let data: Vec<u8> = (0..(4 * 1024 * 1024)).map(|i| (i % 251) as u8).collect();
        std::fs::write(e.local.join("f"), &data).unwrap();
        faulty.fail_writes_after(1024 * 1024);
        e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("f"))], p(&e.remote)).await.unwrap();
        e.rec.wait("paused", |r| r.job_by_rel("f").state == JobState::Paused).await;
        std::fs::remove_file(e.remote.join("f.filepart")).unwrap();
        e.queue.set_remote(Some(FaultySide::new()));
        e.rec.wait_all_final().await;
        assert!(std::fs::read(e.remote.join("f")).unwrap() == data, "content differs");
    }

    mod edit_hooks {
        use super::*;
        use crate::transfer::EnqueueOptions;

        #[tokio::test]
        async fn enqueue_with_overwrite_never_asks() {
            let e = env(opts());
            std::fs::write(e.local.join("a"), b"new").unwrap();
            std::fs::write(e.remote.join("a"), b"old").unwrap();
            let options = EnqueueOptions { decision: Some(Decision::Overwrite), file_mode: None };
            let batch = e
                .queue
                .enqueue_with(Direction::Upload, vec![p(&e.local.join("a"))], p(&e.remote), options)
                .await
                .unwrap();
            let jobs = e.queue.wait_batch(batch).await;
            assert_eq!(jobs.iter().map(|j| j.state).collect::<Vec<_>>(), [JobState::Done]);
            assert!(!e.rec.events().iter().any(|ev| matches!(ev, TransferEvent::Conflict { .. })));
            assert_eq!(std::fs::read(e.remote.join("a")).unwrap(), b"new");
        }

        #[tokio::test]
        async fn enqueue_with_file_mode_sets_exactly_that_mode() {
            let e = env(TransferOptions { parallel: 4, upload_mode: Some(0o600) });
            std::fs::write(e.local.join("a"), b"x").unwrap();
            let options = EnqueueOptions { decision: None, file_mode: Some(0o640) };
            let batch = e
                .queue
                .enqueue_with(Direction::Upload, vec![p(&e.local.join("a"))], p(&e.remote), options)
                .await
                .unwrap();
            e.queue.wait_batch(batch).await;
            assert_eq!(std::fs::metadata(e.remote.join("a")).unwrap().permissions().mode() & 0o777, 0o640);
        }

        #[tokio::test]
        async fn wait_batch_returns_final_jobs() {
            let e = env(opts());
            std::fs::write(e.remote.join("a"), b"x").unwrap();
            let batch = e.queue.enqueue(Direction::Download, vec![p(&e.remote.join("a"))], p(&e.local)).await.unwrap();
            let jobs = e.queue.wait_batch(batch).await;
            assert_eq!(jobs.len(), 1);
            assert_eq!((jobs[0].state, jobs[0].batch_id), (JobState::Done, batch));
        }

        #[tokio::test]
        async fn wait_batch_of_unknown_batch_is_empty() {
            let e = env(opts());
            assert!(e.queue.wait_batch(99).await.is_empty());
        }

        #[tokio::test]
        async fn wait_batch_returns_on_close() {
            let e = env(opts());
            e.queue.set_remote(None);
            std::fs::write(e.local.join("a"), b"x").unwrap();
            let batch = e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("a"))], p(&e.remote)).await.unwrap();
            let queue = e.queue.clone();
            let waiter = tokio::spawn(async move { queue.wait_batch(batch).await });
            tokio::time::sleep(Duration::from_millis(20)).await;
            e.queue.close().await;
            let jobs = tokio::time::timeout(Duration::from_secs(2), waiter).await.unwrap().unwrap();
            assert_eq!(jobs[0].state, JobState::Cancelled);
        }

        #[tokio::test]
        async fn remote_reports_the_current_connection() {
            let e = env(opts());
            assert!(e.queue.remote().is_some());
            e.queue.set_remote(None);
            assert!(e.queue.remote().is_none());
        }

        #[tokio::test]
        async fn edit_batches_are_not_retryable() {
            let e = env(opts());
            for name in ["plain", "edit"] {
                std::fs::write(e.local.join(name), b"x").unwrap();
                std::fs::set_permissions(e.local.join(name), std::fs::Permissions::from_mode(0o000)).unwrap();
            }
            let plain =
                e.queue.enqueue(Direction::Upload, vec![p(&e.local.join("plain"))], p(&e.remote)).await.unwrap();
            let options = EnqueueOptions { decision: Some(Decision::Overwrite), file_mode: Some(0o644) };
            let edit = e
                .queue
                .enqueue_with(Direction::Upload, vec![p(&e.local.join("edit"))], p(&e.remote), options)
                .await
                .unwrap();
            let plain = e.queue.wait_batch(plain).await.remove(0);
            let edit = e.queue.wait_batch(edit).await.remove(0);
            assert_eq!((plain.state, plain.retryable), (JobState::Failed, true));
            assert_eq!((edit.state, edit.retryable), (JobState::Failed, false));
            assert!(e.queue.retry(edit.id).is_err());
            assert!(e.queue.retry(plain.id).is_ok());
        }
    }
}
