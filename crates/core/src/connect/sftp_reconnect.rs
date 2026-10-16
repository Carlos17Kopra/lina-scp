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
    Attempting {
        lost: Option<CoreError>,
    },
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
        let st =
            St { phase: Phase::Lost, generation: 0, round: cancelled, reason: not_connected(), ever_connected: false };
        Self { shared: Arc::new(Shared { st: Mutex::new(st), connector, enabled, status, change, now: Notify::new() }) }
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
async fn run_round<T: Connection>(
    shared: Arc<Shared<T>>,
    round: CancellationToken,
    mut reason: CoreError,
    wait_first: bool,
) {
    for (index, secs) in BACKOFF_SECS.iter().enumerate() {
        let attempt = index as u32 + 1;
        let delay = if index == 0 && !wait_first { Duration::ZERO } else { Duration::from_secs(*secs) };
        {
            let _st = shared.lock();
            if round.is_cancelled() {
                return;
            }
            (shared.status)(ConnStatus::Reconnecting {
                attempt,
                max_attempts: MAX_ATTEMPTS,
                retry_in: delay,
                reason: reason.clone(),
            });
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
            {
                let e = enabled.clone();
                Arc::new(move || e.load(Ordering::SeqCst))
            },
            {
                let s = statuses.clone();
                Arc::new(move |st| s.lock().unwrap().push(st))
            },
            {
                let c = changes.clone();
                Arc::new(move |conn: Option<Arc<Fake>>| c.lock().unwrap().push(conn.map(|c| c.id)))
            },
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
        assert!(matches!(
            r.statuses.lock().unwrap().last(),
            Some(ConnStatus::Lost { error: CoreError::InputRequired })
        ));
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
        assert!(matches!(
            r.statuses.lock().unwrap().last(),
            Some(ConnStatus::Lost { error: CoreError::Network { .. } })
        ));
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
        settle().await;
        assert!(stale.closed.load(Ordering::SeqCst));
    }
}
