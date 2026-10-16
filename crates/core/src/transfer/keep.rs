//! Keeping a remote directory up to date (TP4c): the local directory is polled and every new or changed entry is
//! uploaded through the queue once it stopped changing. Nothing is ever deleted.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::task::AbortHandle;

use tokio_util::sync::CancellationToken;

use super::compare::{TreeEntry, walk};
use super::sync::{SyncItem, enqueue_sync};
use super::{Direction, FileStamp, LocalSide, TransferQueue};
use crate::connect::network_error;
use crate::error::{CoreError, NetworkKind, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum KeepEvent {
    /// On start and on every change.
    State { local_dir: String, remote_dir: String, uploaded: u32, error: Option<CoreError> },
}

pub type KeepEmit = Arc<dyn Fn(KeepEvent) + Send + Sync>;

fn stamp(entry: &TreeEntry) -> FileStamp {
    FileStamp { size: entry.size, mtime: entry.mtime }
}

/// What has been seen, uploaded and is known to be on the server, by path relative to the start directories.
#[derive(Clone)]
struct Tracker {
    /// The local tree at the previous tick: an entry that differs from it is still being written.
    seen: HashMap<String, TreeEntry>,
    /// Local state that was uploaded (or was there at the start).
    sent: HashMap<String, FileStamp>,
    /// Server state as last known: the start, then what was uploaded.
    server: HashMap<String, TreeEntry>,
}

impl Tracker {
    fn new(local: Vec<TreeEntry>, remote: Vec<TreeEntry>) -> Self {
        Self {
            sent: local.iter().map(|e| (e.rel.clone(), stamp(e))).collect(),
            seen: local.into_iter().map(|e| (e.rel.clone(), e)).collect(),
            server: remote.into_iter().map(|e| (e.rel.clone(), e)).collect(),
        }
    }

    /// The entries to upload now; they count as uploaded afterwards. A directory the server does not have goes up
    /// as one entry (the queue copies its contents), and only once nothing in it changed during the last tick.
    fn ready(&mut self, now: Vec<TreeEntry>) -> Vec<SyncItem> {
        let stable = |e: &TreeEntry| self.seen.get(&e.rel) == Some(e);
        let changed = |e: &TreeEntry| {
            if e.dir { !self.sent.contains_key(&e.rel) } else { self.sent.get(&e.rel) != Some(&stamp(e)) }
        };
        let under = |rel: &str, dir: &str| rel.strip_prefix(dir).is_some_and(|r| r.starts_with('/'));
        let (mut items, mut uploaded, mut handled) = (Vec::new(), Vec::new(), Vec::<&str>::new());
        for e in &now {
            if handled.iter().any(|d| under(&e.rel, d)) {
                continue;
            }
            if e.dir && !self.server.get(&e.rel).is_some_and(|s| s.dir) {
                // ponytail: scans the whole tree per missing dir, O(n·dirs); fine for project-sized trees.
                let tree: Vec<&TreeEntry> = now.iter().filter(|c| c.rel == e.rel || under(&c.rel, &e.rel)).collect();
                if tree.iter().any(|c| changed(c)) && tree.iter().all(|c| stable(c)) {
                    items.push(SyncItem { rel: e.rel.clone(), expected: None, tree: None });
                    uploaded.extend(tree.into_iter().cloned());
                }
                handled.push(&e.rel);
            } else if e.dir || (changed(e) && stable(e)) {
                if !e.dir {
                    let expected = self.server.get(&e.rel).filter(|s| !s.dir).map(stamp);
                    items.push(SyncItem { rel: e.rel.clone(), expected, tree: None });
                }
                uploaded.push(e.clone());
            }
        }
        for e in uploaded {
            self.sent.insert(e.rel.clone(), stamp(&e));
            // Uploads keep the mtime, so this is the server's state afterwards.
            self.server.insert(e.rel.clone(), e);
        }
        self.seen = now.into_iter().map(|e| (e.rel.clone(), e)).collect();
        items
    }
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
    let server = walk(&*remote, &remote_dir, &exclude, &never).await?.entries;
    let mut tracker = Tracker::new(local, server);
    let (local_name, remote_name) = (local_dir.clone(), remote_dir.clone());
    let state = move |uploaded, error| KeepEvent::State {
        local_dir: local_name.clone(),
        remote_dir: remote_name.clone(),
        uploaded,
        error,
    };
    let mut last = state(0, None);
    emit(last.clone());
    let task = tokio::spawn(async move {
        let mut uploaded = 0u32;
        loop {
            tokio::time::sleep(poll).await;
            // ponytail: polls a full walk like editing does (TP3c); inotify if trees get large. Times are seconds, so a
            // same-size rewrite within the same second is missed until the next change.
            let error = match walk(&LocalSide, &local_dir, &exclude, &never).await {
                Ok(now) => {
                    let before = tracker.clone();
                    let items = tracker.ready(now.entries);
                    let count = u32::try_from(items.len()).unwrap_or(u32::MAX);
                    if items.is_empty() {
                        None
                    } else {
                        match enqueue_sync(&queue, Direction::Upload, &local_dir, &remote_dir, items, &exclude).await {
                            Ok(_) => {
                                uploaded = uploaded.saturating_add(count);
                                None
                            }
                            Err(error) => {
                                tracker = before;
                                Some(error)
                            }
                        }
                    }
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
    use crate::transfer::test_support::{Recorder, p};
    use crate::transfer::{JobState, LocalSide, TransferOptions};
    use std::sync::Mutex;

    fn file(rel: &str, size: u64, mtime: i64) -> TreeEntry {
        TreeEntry { rel: rel.into(), dir: false, size, mtime: Some(mtime) }
    }

    fn dir(rel: &str, mtime: i64) -> TreeEntry {
        TreeEntry { rel: rel.into(), dir: true, size: 0, mtime: Some(mtime) }
    }

    fn rels(items: &[SyncItem]) -> Vec<(&str, Option<FileStamp>)> {
        items.iter().map(|i| (i.rel.as_str(), i.expected)).collect()
    }

    #[test]
    fn a_changed_file_goes_up_once_it_stood_still_for_a_tick() {
        let start = vec![file("a", 1, 10), file("b", 1, 10)];
        let mut t = Tracker::new(start.clone(), vec![file("a", 1, 10)]);
        assert!(t.ready(start.clone()).is_empty(), "the start state is not uploaded");
        let changed = vec![file("a", 2, 20), file("b", 1, 10), file("c", 3, 30)];
        assert!(t.ready(changed.clone()).is_empty(), "still being written");
        let items = t.ready(changed.clone());
        assert_eq!(rels(&items), [("a", Some(FileStamp { size: 1, mtime: Some(10) })), ("c", None)]);
        assert!(t.ready(changed.clone()).is_empty(), "uploaded once");
        let again = vec![file("a", 4, 40), file("b", 1, 10), file("c", 3, 30)];
        t.ready(again.clone());
        assert_eq!(rels(&t.ready(again)), [("a", Some(FileStamp { size: 2, mtime: Some(20) }))], "what was uploaded");
    }

    #[test]
    fn a_dir_missing_on_the_server_goes_up_as_one_entry_when_all_of_it_stood_still() {
        let start = vec![dir("site", 1), file("site/x", 1, 1), dir("old", 1), file("old/y", 1, 1)];
        let mut t = Tracker::new(start.clone(), vec![dir("site", 1), file("site/x", 1, 1)]);
        let mut now = start.clone();
        now.extend([dir("new", 5), dir("new/sub", 5), file("new/sub/f", 1, 5), file("site/n", 1, 5)]);
        now.sort_by(|a, b| a.rel.cmp(&b.rel));
        t.ready(now.clone());
        let mut later = now.clone();
        later.retain(|e| e.rel != "new/sub/f");
        later.push(file("new/sub/f", 2, 6));
        later.sort_by(|a, b| a.rel.cmp(&b.rel));
        assert_eq!(rels(&t.ready(later.clone())), [("site/n", None)], "new/ is still changing");
        assert_eq!(rels(&t.ready(later.clone())), [("new", None)]);
        assert!(t.ready(later.clone()).is_empty());
        // A change inside a start dir the server does not have uploads that dir.
        let mut edited = later.clone();
        edited.retain(|e| e.rel != "old/y");
        edited.push(file("old/y", 2, 9));
        edited.sort_by(|a, b| a.rel.cmp(&b.rel));
        t.ready(edited.clone());
        assert_eq!(rels(&t.ready(edited)), [("old", None)]);
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
}
