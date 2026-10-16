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
