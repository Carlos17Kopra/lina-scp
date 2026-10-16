//! Keeping a remote directory up to date (TP4c): the local directory is polled and every new or changed entry is
//! uploaded through the queue once it stopped changing. Nothing is ever deleted.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use super::compare::{TreeEntry, walk};
use super::side::{NodeKind, Side};
use super::sync::{SyncItem, enqueue_sync};
use super::{Direction, FileStamp, Job, JobState, LocalSide, TransferQueue};
use crate::connect::network_error;
use crate::error::{CoreError, NetworkKind, Result};
use crate::sftp::join;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum KeepEvent {
    /// On start and on every change. `run` tells the events of one start from those of an earlier one.
    State { run: u64, local_dir: String, remote_dir: String, uploaded: u32, error: Option<CoreError> },
}

pub type KeepEmit = Arc<dyn Fn(KeepEvent) + Send + Sync>;

static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

fn stamp(entry: &TreeEntry) -> FileStamp {
    FileStamp { size: entry.size, mtime: entry.mtime }
}

fn under(rel: &str, dir: &str) -> bool {
    rel.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// One queued entry and the local entries it carries (a directory carries its whole tree).
#[derive(Debug, Clone)]
struct Planned {
    item: SyncItem,
    entries: Vec<TreeEntry>,
}

/// What has been seen, uploaded and is known to be on the server, by path relative to the start directories.
struct Tracker {
    /// The local tree at the previous tick: an entry that differs from it is still being written.
    seen: HashMap<String, TreeEntry>,
    /// Local state that was uploaded or tried (or was there at the start); changes from it are uploaded.
    sent: HashMap<String, FileStamp>,
    /// Server state as last known: the start, then what finished uploading.
    server: HashMap<String, TreeEntry>,
    /// Queued and not finished: nothing at or below is queued again, so two jobs never write one target.
    pending: HashSet<String>,
}

impl Tracker {
    fn new(local: Vec<TreeEntry>, remote: Vec<TreeEntry>) -> Self {
        Self {
            sent: local.iter().map(|e| (e.rel.clone(), stamp(e))).collect(),
            seen: local.into_iter().map(|e| (e.rel.clone(), e)).collect(),
            server: remote.into_iter().map(|e| (e.rel.clone(), e)).collect(),
            pending: HashSet::new(),
        }
    }

    /// The entries to upload now; they are pending until `finish`. A directory the server does not have goes up
    /// as one entry (the queue copies its contents), and only once nothing in it changed during the last tick.
    fn ready(&mut self, now: Vec<TreeEntry>) -> Vec<Planned> {
        let stable = |e: &TreeEntry| self.seen.get(&e.rel) == Some(e);
        let changed = |e: &TreeEntry| {
            if e.dir { !self.sent.contains_key(&e.rel) } else { self.sent.get(&e.rel) != Some(&stamp(e)) }
        };
        let (mut planned, mut handled) = (Vec::new(), Vec::<&str>::new());
        for e in &now {
            let busy = |d: &str| e.rel == d || under(&e.rel, d);
            if handled.iter().any(|d| under(&e.rel, d)) || self.pending.iter().any(|d| busy(d)) {
                continue;
            }
            if e.dir && !self.server.get(&e.rel).is_some_and(|s| s.dir) {
                // ponytail: scans the whole tree per missing dir, O(n·dirs); fine for project-sized trees.
                let tree: Vec<&TreeEntry> = now.iter().filter(|c| c.rel == e.rel || under(&c.rel, &e.rel)).collect();
                if tree.iter().any(|c| changed(c)) && tree.iter().all(|c| stable(c)) {
                    let item = SyncItem { rel: e.rel.clone(), expected: None, tree: None };
                    planned.push(Planned { item, entries: tree.into_iter().cloned().collect() });
                }
                handled.push(&e.rel);
            } else if !e.dir && changed(e) && stable(e) {
                let expected = self.server.get(&e.rel).filter(|s| !s.dir).map(stamp);
                let item = SyncItem { rel: e.rel.clone(), expected, tree: None };
                planned.push(Planned { item, entries: vec![e.clone()] });
            }
        }
        self.pending.extend(planned.iter().map(|p| p.item.rel.clone()));
        self.seen = now.into_iter().map(|e| (e.rel.clone(), e)).collect();
        planned
    }

    /// The queue gave up on `rel` before it started: it is tried again.
    fn unplan(&mut self, rel: &str) {
        self.pending.remove(rel);
    }

    /// `rel` finished. Each local entry it carried counts as tried (a failed or skipped one waits for the next
    /// change); the server state is known only for the entries that were uploaded.
    fn finish(&mut self, rel: &str, outcome: Vec<(TreeEntry, Option<TreeEntry>)>) {
        self.pending.remove(rel);
        for (local, server) in outcome {
            self.sent.insert(local.rel.clone(), stamp(&local));
            if let Some(server) = server {
                self.server.insert(server.rel.clone(), server);
            }
        }
    }
}

/// The server tree; a local directory that is a dir link on the server counts as the directory it points to.
async fn server_tree(
    remote: &dyn Side,
    remote_dir: &str,
    local: &[TreeEntry],
    exclude: &[String],
    never: &CancellationToken,
) -> Result<Vec<TreeEntry>> {
    let mut server = walk(remote, remote_dir, exclude, never).await?.entries;
    let mut known: HashSet<String> = server.iter().map(|e| e.rel.clone()).collect();
    // `local` is sorted, so a link is walked before the directories below it are looked at.
    for dir in local.iter().filter(|e| e.dir) {
        if known.contains(&dir.rel) {
            continue;
        }
        let path = join(remote_dir, &dir.rel);
        if let Some(node) = remote.stat(&path).await?
            && node.kind == NodeKind::DirLink
        {
            // ponytail: one level of links; a link inside the linked tree is hidden as in `walk`.
            let inner = walk(remote, &path, exclude, never).await?.entries;
            let linked = TreeEntry { rel: dir.rel.clone(), dir: true, size: 0, mtime: node.mtime };
            for e in std::iter::once(linked)
                .chain(inner.into_iter().map(|e| TreeEntry { rel: format!("{}/{}", dir.rel, e.rel), ..e }))
            {
                known.insert(e.rel.clone());
                server.push(e);
            }
        }
    }
    Ok(server)
}

/// What a finished batch did to each entry it carried: the server's state for the uploaded ones.
async fn outcome(
    queue: &TransferQueue,
    remote_dir: &str,
    entries: Vec<TreeEntry>,
    jobs: &[Job],
) -> (Vec<(TreeEntry, Option<TreeEntry>)>, u32) {
    let (mut out, mut uploaded) = (Vec::with_capacity(entries.len()), 0u32);
    for e in entries {
        let target = join(remote_dir, &e.rel);
        let done = jobs.iter().any(|j| j.target == target && j.state == JobState::Done);
        let server = match (done, e.dir, queue.remote()) {
            (false, ..) => None,
            (true, true, _) => Some(e.clone()),
            // The source may have changed until the job read it: ask the server what it has now.
            (true, false, Some(remote)) => remote.stat(&target).await.ok().flatten().map(|n| TreeEntry {
                rel: e.rel.clone(),
                dir: false,
                size: n.size,
                mtime: n.mtime,
            }),
            (true, false, None) => None,
        };
        if done && !e.dir {
            uploaded += 1;
        }
        out.push((e, server));
    }
    (out, uploaded)
}

/// Stops polling when stopped or dropped.
pub struct KeepHandle {
    task: AbortHandle,
}

impl KeepHandle {
    pub fn stop(&self) {
        self.task.abort();
    }
}

impl Drop for KeepHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Reads both trees as the starting state, then polls `local_dir` every `poll`.
pub async fn keep_up_to_date(
    queue: TransferQueue,
    local_dir: String,
    remote_dir: String,
    exclude: Vec<String>,
    poll: Duration,
    emit: KeepEmit,
) -> Result<KeepHandle> {
    let remote = queue.remote().ok_or_else(|| network_error(NetworkKind::Other, "not connected"))?;
    let never = CancellationToken::new();
    let local = walk(&LocalSide, &local_dir, &exclude, &never).await?.entries;
    let server = server_tree(&*remote, &remote_dir, &local, &exclude, &never).await?;
    let mut tracker = Tracker::new(local, server);
    let run = NEXT_RUN.fetch_add(1, Ordering::Relaxed);
    let (local_name, remote_name) = (local_dir.clone(), remote_dir.clone());
    let state = move |uploaded, error| KeepEvent::State {
        run,
        local_dir: local_name.clone(),
        remote_dir: remote_name.clone(),
        uploaded,
        error,
    };
    let mut last = state(0, None);
    emit(last.clone());
    let (finished, mut results) = mpsc::unbounded_channel::<(Vec<Planned>, Vec<Job>)>();
    let task = tokio::spawn(async move {
        let mut uploaded = 0u32;
        loop {
            tokio::time::sleep(poll).await;
            while let Ok((planned, jobs)) = results.try_recv() {
                for p in planned {
                    let (out, n) = outcome(&queue, &remote_dir, p.entries, &jobs).await;
                    uploaded = uploaded.saturating_add(n);
                    tracker.finish(&p.item.rel, out);
                }
            }
            // ponytail: polls a full walk like editing does (TP3c); inotify if trees get large. Times are seconds, so a
            // same-size rewrite within the same second is missed until the next change.
            let error = match walk(&LocalSide, &local_dir, &exclude, &never).await {
                Ok(now) => {
                    let mut groups: BTreeMap<String, Vec<Planned>> = BTreeMap::new();
                    for p in tracker.ready(now.entries) {
                        let parent = p.item.rel.rsplit_once('/').map(|(d, _)| d).unwrap_or_default().to_string();
                        groups.entry(parent).or_default().push(p);
                    }
                    let mut error = None;
                    // One batch per target directory, so each batch's jobs come back with the entries it carried.
                    for planned in groups.into_values() {
                        let items = planned.iter().map(|p| p.item.clone()).collect();
                        match enqueue_sync(&queue, Direction::Upload, &local_dir, &remote_dir, items, &exclude).await {
                            Ok(batches) => {
                                let (queue, finished) = (queue.clone(), finished.clone());
                                tokio::spawn(async move {
                                    let mut jobs = Vec::new();
                                    for batch in batches {
                                        jobs.extend(queue.wait_batch(batch).await);
                                    }
                                    let _ = finished.send((planned, jobs));
                                });
                            }
                            Err(e) => {
                                planned.iter().for_each(|p| tracker.unplan(&p.item.rel));
                                error = Some(e);
                            }
                        }
                    }
                    error
                }
                Err(error) => Some(error),
            };
            let next = state(uploaded, error);
            if next != last {
                emit(next.clone());
                last = next;
            }
        }
    });
    Ok(KeepHandle { task: task.abort_handle() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::test_support::{FaultySide, Recorder, p};
    use crate::transfer::{JobState, LocalSide, Side, TransferOptions};
    use std::sync::Mutex;

    fn file(rel: &str, size: u64, mtime: i64) -> TreeEntry {
        TreeEntry { rel: rel.into(), dir: false, size, mtime: Some(mtime) }
    }

    fn dir(rel: &str, mtime: i64) -> TreeEntry {
        TreeEntry { rel: rel.into(), dir: true, size: 0, mtime: Some(mtime) }
    }

    fn rels(planned: &[Planned]) -> Vec<(&str, Option<FileStamp>)> {
        planned.iter().map(|p| (p.item.rel.as_str(), p.item.expected)).collect()
    }

    /// `ready`, then every entry uploaded as it is.
    fn ready_done(t: &mut Tracker, now: Vec<TreeEntry>) -> Vec<Planned> {
        let planned = t.ready(now);
        for p in planned.clone() {
            t.finish(&p.item.rel, p.entries.into_iter().map(|e| (e.clone(), Some(e))).collect());
        }
        planned
    }

    #[test]
    fn a_changed_file_goes_up_once_it_stood_still_for_a_tick() {
        let start = vec![file("a", 1, 10), file("b", 1, 10)];
        let mut t = Tracker::new(start.clone(), vec![file("a", 1, 10)]);
        assert!(ready_done(&mut t, start.clone()).is_empty(), "the start state is not uploaded");
        let changed = vec![file("a", 2, 20), file("b", 1, 10), file("c", 3, 30)];
        assert!(ready_done(&mut t, changed.clone()).is_empty(), "still being written");
        let items = ready_done(&mut t, changed.clone());
        assert_eq!(rels(&items), [("a", Some(FileStamp { size: 1, mtime: Some(10) })), ("c", None)]);
        assert!(ready_done(&mut t, changed.clone()).is_empty(), "uploaded once");
        let again = vec![file("a", 4, 40), file("b", 1, 10), file("c", 3, 30)];
        ready_done(&mut t, again.clone());
        let what_was_uploaded = Some(FileStamp { size: 2, mtime: Some(20) });
        assert_eq!(rels(&ready_done(&mut t, again)), [("a", what_was_uploaded)]);
    }

    #[test]
    fn an_entry_in_flight_waits_and_a_failed_one_waits_for_the_next_change() {
        let start = vec![file("a", 1, 10)];
        let mut t = Tracker::new(start.clone(), start.clone());
        let v1 = vec![file("a", 2, 20)];
        t.ready(v1.clone());
        let planned = t.ready(v1.clone());
        assert_eq!(rels(&planned), [("a", Some(FileStamp { size: 1, mtime: Some(10) }))]);
        let v2 = vec![file("a", 3, 30)];
        t.ready(v2.clone());
        assert!(t.ready(v2.clone()).is_empty(), "never two jobs for one target");
        t.finish("a", vec![(file("a", 2, 20), None)]);
        assert_eq!(rels(&t.ready(v2.clone())), [("a", Some(FileStamp { size: 1, mtime: Some(10) }))], "server unknown");
        t.finish("a", vec![(file("a", 3, 30), None)]);
        assert!(t.ready(v2).is_empty(), "a failed upload is not retried by itself");
    }

    #[test]
    fn a_dir_missing_on_the_server_goes_up_as_one_entry_when_all_of_it_stood_still() {
        let start = vec![dir("site", 1), file("site/x", 1, 1), dir("old", 1), file("old/y", 1, 1)];
        let mut t = Tracker::new(start.clone(), vec![dir("site", 1), file("site/x", 1, 1)]);
        let mut now = start.clone();
        now.extend([dir("new", 5), dir("new/sub", 5), file("new/sub/f", 1, 5), file("site/n", 1, 5)]);
        now.sort_by(|a, b| a.rel.cmp(&b.rel));
        ready_done(&mut t, now.clone());
        let mut later = now.clone();
        later.retain(|e| e.rel != "new/sub/f");
        later.push(file("new/sub/f", 2, 6));
        later.sort_by(|a, b| a.rel.cmp(&b.rel));
        assert_eq!(rels(&ready_done(&mut t, later.clone())), [("site/n", None)], "new/ is still changing");
        assert_eq!(rels(&ready_done(&mut t, later.clone())), [("new", None)]);
        assert!(ready_done(&mut t, later.clone()).is_empty());
        // A change inside a start dir the server does not have uploads that dir.
        let mut edited = later.clone();
        edited.retain(|e| e.rel != "old/y");
        edited.push(file("old/y", 2, 9));
        edited.sort_by(|a, b| a.rel.cmp(&b.rel));
        ready_done(&mut t, edited.clone());
        assert_eq!(rels(&ready_done(&mut t, edited)), [("old", None)]);
    }

    struct Env {
        _d: tempfile::TempDir,
        l: std::path::PathBuf,
        r: std::path::PathBuf,
        rec: Recorder,
        queue: TransferQueue,
        events: Arc<Mutex<Vec<KeepEvent>>>,
    }

    fn env() -> Env {
        let d = tempfile::tempdir().unwrap();
        let (l, r) = (d.path().join("l"), d.path().join("r"));
        std::fs::create_dir_all(&l).unwrap();
        std::fs::create_dir_all(&r).unwrap();
        let rec = Recorder::default();
        let options = TransferOptions { parallel: 2, upload_mode: None, bandwidth: None };
        let queue = TransferQueue::new(Arc::new(LocalSide), options, rec.emit());
        queue.set_remote(Some(Arc::new(LocalSide)));
        Env { _d: d, l, r, rec, queue, events: Arc::default() }
    }

    impl Env {
        async fn start(&self, exclude: &[&str]) -> KeepHandle {
            let events = self.events.clone();
            let exclude = exclude.iter().map(|s| s.to_string()).collect();
            let emit: KeepEmit = Arc::new(move |e| events.lock().unwrap().push(e));
            keep_up_to_date(self.queue.clone(), p(&self.l), p(&self.r), exclude, Duration::from_millis(20), emit)
                .await
                .unwrap()
        }

        fn last(&self) -> (u32, Option<CoreError>) {
            match self.events.lock().unwrap().last().cloned() {
                Some(KeepEvent::State { uploaded, error, .. }) => (uploaded, error),
                None => (0, None),
            }
        }

        async fn wait(&self, what: &str, pred: impl Fn(&Env) -> bool) {
            for _ in 0..300 {
                if pred(self) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("timed out waiting for {what}: {:?}", self.events.lock().unwrap());
        }
    }

    #[tokio::test]
    async fn uploads_new_and_changed_files_but_not_the_start_state_or_excluded_names() {
        let e = env();
        std::fs::write(e.l.join("before"), b"start").unwrap();
        let keep = e.start(&["*.tmp"]).await;
        std::fs::write(e.l.join("index.html"), b"new").unwrap();
        std::fs::write(e.l.join("x.tmp"), b"scratch").unwrap();
        e.wait("upload", |e| e.r.join("index.html").exists()).await;
        e.rec.wait_all_final().await;
        assert!(!e.r.join("before").exists() && !e.r.join("x.tmp").exists());
        e.wait("count", |e| e.last().0 == 1).await;
        keep.stop();
        tokio::time::sleep(Duration::from_millis(100)).await;
        std::fs::write(e.l.join("after_stop"), b"x").unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!e.r.join("after_stop").exists());
    }

    #[tokio::test]
    async fn a_file_changed_on_the_server_meanwhile_asks_before_it_is_overwritten() {
        let e = env();
        std::fs::write(e.l.join("f"), b"local").unwrap();
        std::fs::write(e.r.join("f"), b"server").unwrap();
        let _keep = e.start(&[]).await;
        std::fs::write(e.r.join("f"), b"someone else").unwrap();
        std::fs::write(e.l.join("f"), b"local edit").unwrap();
        e.rec.wait("asks", |r| r.jobs().values().any(|j| j.state == JobState::WaitingDecision)).await;
        assert_eq!(std::fs::read(e.r.join("f")).unwrap(), b"someone else");
    }

    #[tokio::test]
    async fn an_unreadable_local_dir_is_reported_and_watching_goes_on() {
        let e = env();
        let _keep = e.start(&[]).await;
        let moved = e.l.with_extension("away");
        std::fs::rename(&e.l, &moved).unwrap();
        e.wait("error", |e| e.last().1.is_some()).await;
        std::fs::rename(&moved, &e.l).unwrap();
        std::fs::write(e.l.join("back"), b"x").unwrap();
        e.wait("upload after the error", |e| e.r.join("back").exists()).await;
        e.wait("error cleared", |e| e.last().1.is_none()).await;
    }

    #[tokio::test]
    async fn start_fails_without_a_connection_or_a_readable_remote_dir() {
        let e = env();
        let emit: KeepEmit = Arc::new(|_| {});
        let missing = p(&e.r.join("missing"));
        let bad = keep_up_to_date(e.queue.clone(), p(&e.l), missing, vec![], Duration::from_secs(1), emit.clone());
        assert!(bad.await.is_err());
        e.queue.set_remote(None);
        let offline = keep_up_to_date(e.queue.clone(), p(&e.l), p(&e.r), vec![], Duration::from_secs(1), emit);
        assert!(matches!(offline.await, Err(CoreError::Network { .. })));
    }

    fn no_conflict(rec: &Recorder) {
        assert!(rec.jobs().values().all(|j| j.state != JobState::WaitingDecision), "{:?}", rec.jobs());
    }

    #[tokio::test]
    async fn a_second_save_during_the_upload_goes_up_afterwards_without_a_conflict() {
        let mut e = env();
        let slow = FaultySide::new();
        slow.delay_renames(300);
        e.queue = TransferQueue::new(
            Arc::new(LocalSide),
            TransferOptions { parallel: 2, upload_mode: None, bandwidth: None },
            e.rec.emit(),
        );
        e.queue.set_remote(Some(slow));
        std::fs::write(e.l.join("f"), b"start").unwrap();
        std::fs::write(e.r.join("f"), b"start").unwrap();
        let _keep = e.start(&[]).await;
        std::fs::write(e.l.join("f"), b"v1").unwrap();
        e.rec.wait("first upload runs", |r| r.jobs().values().any(|j| j.state == JobState::Running)).await;
        std::fs::write(e.l.join("f"), b"second version").unwrap();
        e.wait("second version", |e| std::fs::read(e.r.join("f")).ok().as_deref() == Some(b"second version")).await;
        e.rec.wait_all_final().await;
        no_conflict(&e.rec);
        e.wait("count", |e| e.last().0 == 2).await;
    }

    #[tokio::test]
    async fn a_dir_link_on_the_server_counts_as_the_dir_it_points_to() {
        let e = env();
        std::fs::create_dir_all(e.r.join("releases/x")).unwrap();
        std::os::unix::fs::symlink(e.r.join("releases/x"), e.r.join("current")).unwrap();
        std::fs::create_dir_all(e.l.join("current")).unwrap();
        for side in [e.l.join("current/a"), e.r.join("releases/x/a")] {
            std::fs::write(&side, b"same").unwrap();
            LocalSide.set_mtime(&p(&side), 1_000_000).await.unwrap();
        }
        let _keep = e.start(&[]).await;
        std::fs::write(e.l.join("current/a"), b"edited").unwrap();
        e.wait("upload through the link", |e| std::fs::read(e.r.join("releases/x/a")).unwrap() == b"edited").await;
        e.rec.wait_all_final().await;
        no_conflict(&e.rec);
        assert!(e.r.join("current").is_symlink());
    }

    #[tokio::test]
    async fn every_start_has_its_own_run_id() {
        let e = env();
        let (a, b) = (e.start(&[]).await, e.start(&[]).await);
        drop((a, b));
        let runs: Vec<u64> = e.events.lock().unwrap().iter().map(|KeepEvent::State { run, .. }| *run).collect();
        assert_eq!(runs.len(), 2);
        assert_ne!(runs[0], runs[1]);
    }
}
