//! Helpers for engine tests: an event recorder and a side that fails on demand.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use super::LocalSide;
use super::side::{Child, Node, Reader, Side, Writer};
use super::{Job, JobId, TransferEmit, TransferEvent};
use crate::connect::network_error;
use crate::error::{CoreError, FsKind, NetworkKind, Result};

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

#[derive(Default)]
pub(crate) struct FaultySide {
    write_limit: AtomicU64,
    written: Arc<AtomicU64>,
    read: Arc<AtomicU64>,
    broken: Arc<AtomicBool>,
    children_fail: AtomicBool,
    /// Names a hostile server adds to every listing.
    fake_children: Mutex<Vec<String>>,
    denied: Mutex<HashSet<String>>,
    mode_denied: Mutex<HashSet<String>>,
    /// `stat` leaves out the permission bits, like a server that does not send them.
    hide_modes: AtomicBool,
    /// Slows `rename_over` down (the last step of every file), in milliseconds.
    rename_delay_ms: AtomicU64,
    /// Slows `open_read` down, in milliseconds.
    read_delay_ms: AtomicU64,
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
    pub(crate) fn add_fake_children(&self, names: &[&str]) {
        self.fake_children.lock().unwrap().extend(names.iter().map(|n| n.to_string()));
    }
    pub(crate) fn fail_children(&self) {
        self.children_fail.store(true, Ordering::SeqCst);
    }
    pub(crate) fn deny_read(&self, path: &str) {
        self.denied.lock().unwrap().insert(path.to_string());
    }
    pub(crate) fn deny_set_mode(&self, path: &str) {
        self.mode_denied.lock().unwrap().insert(path.to_string());
    }
    pub(crate) fn hide_modes(&self) {
        self.hide_modes.store(true, Ordering::SeqCst);
    }
    pub(crate) fn delay_reads(&self, ms: u64) {
        self.read_delay_ms.store(ms, Ordering::SeqCst);
    }
    pub(crate) fn delay_renames(&self, ms: u64) {
        self.rename_delay_ms.store(ms, Ordering::SeqCst);
    }
    pub(crate) fn heal(&self) {
        self.write_limit.store(u64::MAX, Ordering::SeqCst);
        self.broken.store(false, Ordering::SeqCst);
        self.children_fail.store(false, Ordering::SeqCst);
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
        let hide = self.hide_modes.load(Ordering::SeqCst);
        Ok(LocalSide.stat(path).await?.map(|node| Node { mode: if hide { None } else { node.mode }, ..node }))
    }
    async fn children(&self, path: &str) -> Result<Vec<Child>> {
        if self.children_fail.load(Ordering::SeqCst) {
            return Err(dead());
        }
        let mut children = LocalSide.children(path).await?;
        let fake = self.fake_children.lock().unwrap().clone();
        children.extend(fake.into_iter().map(|name| Child { name, lossy: false }));
        Ok(children)
    }
    async fn mkdir(&self, path: &str) -> Result<()> {
        LocalSide.mkdir(path).await
    }
    async fn open_read(&self, path: &str) -> Result<Box<dyn Reader>> {
        if self.denied.lock().unwrap().contains(path) {
            return Err(CoreError::fs(FsKind::PermissionDenied, path, "denied"));
        }
        let delay = self.read_delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
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
        if self.mode_denied.lock().unwrap().contains(path) {
            return Err(CoreError::fs(FsKind::PermissionDenied, path, "denied"));
        }
        LocalSide.set_mode(path, mode).await
    }
    async fn set_mtime(&self, path: &str, mtime: i64) -> Result<()> {
        LocalSide.set_mtime(path, mtime).await
    }
    async fn rename_over(&self, from: &str, to: &str) -> Result<()> {
        let delay = self.rename_delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        LocalSide.rename_over(from, to).await
    }
    async fn remove(&self, path: &str) -> Result<()> {
        LocalSide.remove(path).await
    }
    async fn realpath(&self, path: &str) -> Result<String> {
        LocalSide.realpath(path).await
    }
}
