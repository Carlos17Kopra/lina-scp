//! A session that restores a lost SSH connection in place (TP2d spec §1): same session id, same
//! output sink, so the terminal keeps its history and simply continues.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::SessionInfo;
use crate::error::{CoreError, Result};
use crate::session::{OutputSink, Session, SessionEvent};

/// Wait before attempt n (1-based) is `BACKOFF_SECS[n - 1]`.
pub const BACKOFF_SECS: [u64; 8] = [1, 2, 4, 8, 16, 30, 30, 30];
pub const MAX_ATTEMPTS: u32 = BACKOFF_SECS.len() as u32;
/// ponytail: output of a connection still being set up is held back up to this size, the rest is dropped.
const MAX_PENDING_OUTPUT: usize = 1 << 20;

pub type ConnectFuture = Pin<Box<dyn Future<Output = Result<(Arc<dyn Session>, SessionInfo)>> + Send>>;
/// Opens a fresh shell without prompting, `cols`×`rows`, feeding `sink`.
pub type Connector = Arc<dyn Fn(u16, u16, OutputSink, CancellationToken) -> ConnectFuture + Send + Sync>;
/// Whether auto-reconnect is on; read when a connection is lost.
pub type Enabled = Arc<dyn Fn() -> bool + Send + Sync>;

/// Reasons a retry cannot fix: input needed, host/profile/jump host gone, credentials or host key rejected.
pub fn is_fatal(error: &CoreError) -> bool {
    matches!(
        error,
        CoreError::InputRequired
            | CoreError::Config { .. }
            | CoreError::Auth { .. }
            | CoreError::HostKeyRejected { .. }
    )
}

enum Phase {
    /// A connection is being set up (first connect or an attempt); its output waits in `pending`.
    Attempting {
        pending: Vec<u8>,
        /// The cap was hit: later output is dropped too, so the held output has no holes.
        full: bool,
        lost: Option<CoreError>,
    },
    Connected(Arc<dyn Session>),
    /// Between attempts.
    Waiting,
    /// Gave up; `reconnect_now` may start a new round unless `reason` is fatal.
    Lost {
        reason: CoreError,
    },
    /// The shell ended, failed, or `close` was called.
    Ended,
}

struct State {
    phase: Phase,
    /// Current connection; events of older ones are ignored.
    generation: u64,
    /// Cancels the running round (wait and attempt).
    round: CancellationToken,
}

struct Shared {
    /// Taken by the event paths, which call the app sink under it to keep events in order. That
    /// sink may block (output flow control), so input and resizes never take this lock.
    state: Mutex<State>,
    /// The connected session (`Phase::Connected`); set and cleared under `state`, read alone by input.
    live: Mutex<Option<Arc<dyn Session>>>,
    /// Last terminal size; read alone by `resize`, and under `state` by attempts.
    size: Mutex<(u16, u16)>,
    sink: OutputSink,
    connector: Connector,
    enabled: Enabled,
    /// `reconnect_now` while waiting.
    now: Notify,
}

pub struct ReconnectingSession {
    shared: Arc<Shared>,
}

enum Attached {
    Done,
    /// The new connection broke while it was set up.
    LostMeanwhile(CoreError),
}

impl ReconnectingSession {
    /// Starts like an attempt: hand `first_sink()` to the first connect, then `attach` its session.
    pub fn new(sink: OutputSink, cols: u16, rows: u16, connector: Connector, enabled: Enabled) -> Self {
        let state = State {
            phase: Phase::Attempting { pending: Vec::new(), full: false, lost: None },
            generation: 1,
            round: CancellationToken::new(),
        };
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(state),
                live: Mutex::new(None),
                size: Mutex::new((cols, rows)),
                sink,
                connector,
                enabled,
                now: Notify::new(),
            }),
        }
    }

    /// Sink for the first connection.
    pub fn first_sink(&self) -> OutputSink {
        inner_sink(Arc::downgrade(&self.shared), 1)
    }

    /// Hands over the first connection's session.
    pub fn attach(&self, session: Arc<dyn Session>) {
        let size = *self.shared.size();
        if let Attached::LostMeanwhile(reason) = self.shared.attached(1, session, size, None) {
            let mut state = self.shared.lock();
            if state.generation == 1 {
                self.shared.lose(&mut state, reason);
            }
        }
    }
}

fn inner_sink(shared: Weak<Shared>, generation: u64) -> OutputSink {
    // Weak: the inner session owns this sink, and `Shared` owns the inner session.
    Arc::new(move |event| {
        if let Some(shared) = shared.upgrade() {
            shared.on_inner(generation, event);
        }
    })
}

fn close_in_background(session: Arc<dyn Session>) {
    tokio::spawn(async move {
        let _ = session.close().await;
    });
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn live(&self) -> MutexGuard<'_, Option<Arc<dyn Session>>> {
        self.live.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn size(&self) -> MutexGuard<'_, (u16, u16)> {
        self.size.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Events of the inner connection `generation`. The sink is called under the state lock: that
    /// keeps the order of events strict.
    fn on_inner(self: &Arc<Self>, generation: u64, event: SessionEvent) {
        let mut state = self.lock();
        if state.generation != generation {
            return;
        }
        match event {
            SessionEvent::Output(bytes) => match &mut state.phase {
                Phase::Attempting { pending, full, .. } => {
                    *full = *full || pending.len() + bytes.len() > MAX_PENDING_OUTPUT;
                    if !*full {
                        pending.extend_from_slice(&bytes);
                    }
                }
                Phase::Connected(_) => (self.sink)(SessionEvent::Output(bytes)),
                _ => {}
            },
            SessionEvent::Lost(reason) => {
                if let Phase::Attempting { lost, .. } = &mut state.phase {
                    *lost = Some(reason);
                } else if matches!(state.phase, Phase::Connected(_)) {
                    self.lose(&mut state, reason);
                }
            }
            end @ (SessionEvent::Exit { .. } | SessionEvent::Error(_)) => {
                if matches!(state.phase, Phase::Attempting { .. } | Phase::Connected(_)) {
                    // A shell that ends while it is set up (forced command, nologin) keeps its message.
                    if let Phase::Attempting { pending, .. } = std::mem::replace(&mut state.phase, Phase::Ended)
                        && !pending.is_empty()
                    {
                        (self.sink)(SessionEvent::Output(pending));
                    }
                    *self.live() = None;
                    state.round.cancel();
                    (self.sink)(end);
                }
            }
            // Only this wrapper creates reconnect events.
            SessionEvent::Reconnecting { .. } | SessionEvent::Reconnected { .. } => {}
        }
    }

    /// The connection broke: start a round, or report `Lost` when auto-reconnect is off.
    fn lose(self: &Arc<Self>, state: &mut State, reason: CoreError) {
        state.generation += 1;
        if let Phase::Connected(old) = std::mem::replace(&mut state.phase, Phase::Waiting) {
            *self.live() = None;
            close_in_background(old);
        }
        if (self.enabled)() {
            self.start_round(state, reason, true);
        } else {
            state.phase = Phase::Lost { reason: reason.clone() };
            (self.sink)(SessionEvent::Lost(reason));
        }
    }

    fn start_round(self: &Arc<Self>, state: &mut State, reason: CoreError, wait_first: bool) {
        state.phase = Phase::Waiting;
        state.round = CancellationToken::new();
        tokio::spawn(run_round(self.clone(), state.round.clone(), reason, wait_first));
    }

    /// A connection of `generation` is up. Stale (stopped, closed, superseded) ones are closed.
    fn attached(
        self: &Arc<Self>,
        generation: u64,
        session: Arc<dyn Session>,
        used: (u16, u16),
        info: Option<SessionInfo>,
    ) -> Attached {
        let mut state = self.lock();
        if state.generation != generation || !matches!(state.phase, Phase::Attempting { .. }) {
            drop(state);
            close_in_background(session);
            return Attached::Done;
        }
        let Phase::Attempting { pending, lost, .. } =
            std::mem::replace(&mut state.phase, Phase::Connected(session.clone()))
        else {
            unreachable!("checked above");
        };
        if let Some(reason) = lost {
            state.phase = Phase::Attempting { pending: Vec::new(), full: false, lost: None };
            drop(state);
            close_in_background(session);
            return Attached::LostMeanwhile(reason);
        }
        *self.live() = Some(session.clone());
        if let Some(info) = info {
            (self.sink)(SessionEvent::Reconnected { info });
        }
        if !pending.is_empty() {
            (self.sink)(SessionEvent::Output(pending));
        }
        drop(state);
        // A resize that ran before `live` was set changed only `size`: apply it here.
        let size = *self.size();
        if size != used {
            tokio::spawn(async move {
                let _ = session.resize(size.0, size.1).await;
            });
        }
        Attached::Done
    }

    fn give_up(&self, round: &CancellationToken, reason: CoreError) {
        let mut state = self.lock();
        if round.is_cancelled() {
            return;
        }
        state.phase = Phase::Lost { reason: reason.clone() };
        (self.sink)(SessionEvent::Lost(reason));
    }
}

/// One reconnect round: up to `MAX_ATTEMPTS` attempts, each after its backoff wait.
async fn run_round(shared: Arc<Shared>, round: CancellationToken, mut reason: CoreError, wait_first: bool) {
    for (index, secs) in BACKOFF_SECS.iter().enumerate() {
        let attempt = index as u32 + 1;
        let delay = if index == 0 && !wait_first { Duration::ZERO } else { Duration::from_secs(*secs) };
        {
            let state = shared.lock();
            if round.is_cancelled() {
                return;
            }
            debug_assert!(matches!(state.phase, Phase::Waiting));
            (shared.sink)(SessionEvent::Reconnecting {
                attempt,
                max_attempts: MAX_ATTEMPTS,
                retry_in: delay,
                reason: reason.clone(),
            });
        }
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = shared.now.notified() => {
                // "Jetzt versuchen": the UI stops its countdown.
                let _state = shared.lock();
                if round.is_cancelled() {
                    return;
                }
                (shared.sink)(SessionEvent::Reconnecting {
                    attempt,
                    max_attempts: MAX_ATTEMPTS,
                    retry_in: Duration::ZERO,
                    reason: reason.clone(),
                });
            }
            () = round.cancelled() => return,
        }
        let (generation, cols, rows) = {
            let mut state = shared.lock();
            if round.is_cancelled() {
                return;
            }
            state.generation += 1;
            state.phase = Phase::Attempting { pending: Vec::new(), full: false, lost: None };
            let (cols, rows) = *shared.size();
            (state.generation, cols, rows)
        };
        let sink = inner_sink(Arc::downgrade(&shared), generation);
        let result = tokio::select! {
            result = (shared.connector)(cols, rows, sink, round.child_token()) => result,
            () = round.cancelled() => return,
        };
        match result {
            Ok((session, info)) => match shared.attached(generation, session, (cols, rows), Some(info)) {
                Attached::Done => return,
                Attached::LostMeanwhile(error) => reason = error,
            },
            Err(error) if is_fatal(&error) => {
                shared.give_up(&round, error);
                return;
            }
            Err(error) => reason = error,
        }
        let mut state = shared.lock();
        if round.is_cancelled() {
            return;
        }
        state.phase = Phase::Waiting;
    }
    shared.give_up(&round, reason);
}

#[async_trait::async_trait]
impl Session for ReconnectingSession {
    /// Input while the connection is being restored is dropped.
    async fn write(&self, data: &[u8]) -> Result<()> {
        let session = self.shared.live().clone();
        match session {
            Some(session) => session.write(data).await,
            None => Ok(()),
        }
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        *self.shared.size() = (cols, rows);
        let session = self.shared.live().clone();
        match session {
            Some(session) => session.resize(cols, rows).await,
            None => Ok(()),
        }
    }

    /// Always ends with exactly one `Exit` (unless the session had already ended).
    async fn close(&self) -> Result<()> {
        let inner = {
            let mut state = self.shared.lock();
            state.round.cancel();
            state.generation += 1;
            let previous = std::mem::replace(&mut state.phase, Phase::Ended);
            *self.shared.live() = None;
            if !matches!(previous, Phase::Ended) {
                (self.shared.sink)(SessionEvent::Exit { code: None });
            }
            match previous {
                Phase::Connected(session) => Some(session),
                _ => None,
            }
        };
        if let Some(session) = inner {
            let _ = session.close().await;
        }
        Ok(())
    }

    fn reconnect_now(&self) {
        let mut state = self.shared.lock();
        let restart = match &state.phase {
            Phase::Waiting => {
                self.shared.now.notify_one();
                None
            }
            Phase::Lost { reason } if !is_fatal(reason) => Some(reason.clone()),
            _ => None,
        };
        if let Some(reason) = restart {
            self.shared.start_round(&mut state, reason, false);
        }
    }

    fn stop_reconnect(&self) {
        let mut state = self.shared.lock();
        if matches!(state.phase, Phase::Waiting | Phase::Attempting { .. }) {
            state.round.cancel();
            state.generation += 1;
            state.phase = Phase::Lost { reason: CoreError::Cancelled };
            (self.shared.sink)(SessionEvent::Lost(CoreError::Cancelled));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};

    use tokio::sync::Notify;

    use super::*;
    use crate::error::NetworkKind;

    fn net() -> CoreError {
        CoreError::Network { kind: NetworkKind::Other, message: "connection lost".into() }
    }

    #[derive(Default)]
    struct FakeSession {
        writes: Mutex<Vec<Vec<u8>>>,
        sizes: Mutex<Vec<(u16, u16)>>,
        closed: AtomicBool,
    }

    #[async_trait::async_trait]
    impl Session for FakeSession {
        async fn write(&self, data: &[u8]) -> Result<()> {
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(())
        }
        async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
            self.sizes.lock().unwrap().push((cols, rows));
            Ok(())
        }
        async fn close(&self) -> Result<()> {
            self.closed.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Harness {
        session: ReconnectingSession,
        events: Arc<Mutex<Vec<SessionEvent>>>,
        /// Sink of every connection: index 0 is the first connect, then one per attempt.
        sinks: Arc<Mutex<Vec<OutputSink>>>,
        /// Fake session of every successful connection (index 0 = first connect).
        sessions: Arc<Mutex<Vec<Arc<FakeSession>>>>,
        /// Terminal size each attempt was started with.
        attempts: Arc<Mutex<Vec<(u16, u16)>>>,
        enabled: Arc<AtomicBool>,
        /// When set, attempts wait for `notify_one` before resolving.
        gate: Arc<Mutex<Option<Arc<Notify>>>>,
    }

    /// `outcomes`: result of each reconnect attempt in order (`Ok(())` = connected); missing ones fail with `net()`.
    fn harness(outcomes: Vec<Result<()>>) -> Harness {
        let events: Arc<Mutex<Vec<SessionEvent>>> = Arc::default();
        let sinks: Arc<Mutex<Vec<OutputSink>>> = Arc::default();
        let sessions: Arc<Mutex<Vec<Arc<FakeSession>>>> = Arc::default();
        let attempts: Arc<Mutex<Vec<(u16, u16)>>> = Arc::default();
        let enabled = Arc::new(AtomicBool::new(true));
        let gate: Arc<Mutex<Option<Arc<Notify>>>> = Arc::default();
        let plan = Arc::new(Mutex::new(VecDeque::from(outcomes)));

        let connector: Connector = {
            let (sinks, sessions, attempts, gate) = (sinks.clone(), sessions.clone(), attempts.clone(), gate.clone());
            Arc::new(move |cols, rows, sink, _cancel| {
                attempts.lock().unwrap().push((cols, rows));
                sinks.lock().unwrap().push(sink);
                let outcome = plan.lock().unwrap().pop_front().unwrap_or_else(|| Err(net()));
                let gate = gate.lock().unwrap().clone();
                let sessions = sessions.clone();
                Box::pin(async move {
                    if let Some(gate) = gate {
                        gate.notified().await;
                    }
                    outcome.map(|()| {
                        let session = Arc::new(FakeSession::default());
                        sessions.lock().unwrap().push(session.clone());
                        (session as Arc<dyn Session>, SessionInfo { kex: "k2".into(), ..SessionInfo::default() })
                    })
                })
            })
        };
        let sink: OutputSink = {
            let events = events.clone();
            Arc::new(move |event| events.lock().unwrap().push(event))
        };
        let is_enabled: Enabled = {
            let enabled = enabled.clone();
            Arc::new(move || enabled.load(Ordering::SeqCst))
        };
        let session = ReconnectingSession::new(sink, 80, 24, connector, is_enabled);
        sinks.lock().unwrap().push(session.first_sink());
        let first = Arc::new(FakeSession::default());
        sessions.lock().unwrap().push(first.clone());
        session.attach(first);
        Harness { session, events, sinks, sessions, attempts, enabled, gate }
    }

    impl Harness {
        fn emit(&self, connection: usize, event: SessionEvent) {
            let sink = self.sinks.lock().unwrap()[connection].clone();
            sink(event);
        }
        fn events(&self) -> Vec<SessionEvent> {
            self.events.lock().unwrap().clone()
        }
        fn attempts(&self) -> usize {
            self.attempts.lock().unwrap().len()
        }
    }

    /// Lets spawned tasks run; with paused time, sleeping also advances the clock.
    async fn advance(secs: u64) {
        tokio::time::sleep(Duration::from_secs(secs)).await;
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    fn reconnecting(attempt: u32, secs: u64) -> SessionEvent {
        SessionEvent::Reconnecting {
            attempt,
            max_attempts: MAX_ATTEMPTS,
            retry_in: Duration::from_secs(secs),
            reason: net(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn output_of_the_first_connection_flows_through() {
        let h = harness(vec![]);
        h.emit(0, SessionEvent::Output(b"motd".to_vec()));
        assert_eq!(h.events(), vec![SessionEvent::Output(b"motd".to_vec())]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_connection_reconnects_with_backoff() {
        let h = harness(vec![Err(net()), Err(net()), Ok(())]);
        h.emit(0, SessionEvent::Lost(net()));
        advance(1 + 2 + 4).await;
        let info = SessionInfo { kex: "k2".into(), ..SessionInfo::default() };
        assert_eq!(
            h.events(),
            vec![reconnecting(1, 1), reconnecting(2, 2), reconnecting(3, 4), SessionEvent::Reconnected { info }]
        );
        h.session.write(b"ls\n").await.unwrap();
        assert_eq!(*h.sessions.lock().unwrap()[1].writes.lock().unwrap(), vec![b"ls\n".to_vec()]);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_eight_attempts() {
        let h = harness(vec![]);
        h.emit(0, SessionEvent::Lost(net()));
        advance(BACKOFF_SECS.iter().sum::<u64>() + 1).await;
        let events = h.events();
        assert_eq!(h.attempts(), 8);
        assert_eq!(events.iter().filter(|e| matches!(e, SessionEvent::Reconnecting { .. })).count(), 8);
        assert_eq!(events.last(), Some(&SessionEvent::Lost(net())));
        advance(120).await;
        assert_eq!(h.attempts(), 8, "no attempt after giving up");
    }

    #[tokio::test(start_paused = true)]
    async fn fatal_errors_stop_at_once() {
        let h = harness(vec![Err(CoreError::InputRequired)]);
        h.emit(0, SessionEvent::Lost(net()));
        advance(1).await;
        assert_eq!(h.events().last(), Some(&SessionEvent::Lost(CoreError::InputRequired)));
        h.session.reconnect_now();
        advance(60).await;
        assert_eq!(h.attempts(), 1, "reconnect_now does nothing after a fatal reason");
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_reports_lost_without_attempts() {
        let h = harness(vec![]);
        h.enabled.store(false, Ordering::SeqCst);
        h.emit(0, SessionEvent::Lost(net()));
        advance(60).await;
        assert_eq!(h.events(), vec![SessionEvent::Lost(net())]);
        assert_eq!(h.attempts(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_regular_exit_never_reconnects() {
        let h = harness(vec![]);
        h.emit(0, SessionEvent::Exit { code: Some(0) });
        advance(60).await;
        assert_eq!(h.events(), vec![SessionEvent::Exit { code: Some(0) }]);
        assert_eq!(h.attempts(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_now_skips_the_wait_and_restarts_after_giving_up() {
        let h = harness(vec![Err(net()), Ok(())]);
        h.emit(0, SessionEvent::Lost(net()));
        advance(0).await;
        h.session.reconnect_now();
        advance(0).await;
        assert_eq!(h.attempts(), 1, "the first wait was skipped");
        h.session.stop_reconnect();
        assert_eq!(h.events().last(), Some(&SessionEvent::Lost(CoreError::Cancelled)));
        h.session.reconnect_now();
        advance(0).await;
        assert_eq!(h.attempts(), 2, "a new round starts without waiting");
        let events = h.events();
        assert!(events.contains(&SessionEvent::Reconnecting {
            attempt: 1,
            max_attempts: MAX_ATTEMPTS,
            retry_in: Duration::ZERO,
            reason: CoreError::Cancelled,
        }));
        assert!(matches!(events.last(), Some(SessionEvent::Reconnected { .. })));
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_now_during_an_attempt_does_not_skip_the_next_wait() {
        let h = harness(vec![]);
        let gate = Arc::new(Notify::new());
        *h.gate.lock().unwrap() = Some(gate.clone());
        h.emit(0, SessionEvent::Lost(net()));
        advance(1).await;
        assert_eq!(h.attempts(), 1);
        h.session.reconnect_now();
        gate.notify_one();
        advance(0).await;
        assert_eq!(h.attempts(), 1, "the 2 s wait before the second attempt still runs");
        advance(2).await;
        assert_eq!(h.attempts(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn stop_during_an_attempt_discards_a_late_success() {
        let h = harness(vec![Ok(())]);
        let gate = Arc::new(Notify::new());
        *h.gate.lock().unwrap() = Some(gate.clone());
        h.emit(0, SessionEvent::Lost(net()));
        advance(1).await;
        assert_eq!(h.attempts(), 1);
        h.session.stop_reconnect();
        gate.notify_one();
        advance(1).await;
        let events = h.events();
        assert_eq!(events.last(), Some(&SessionEvent::Lost(CoreError::Cancelled)));
        assert!(!events.iter().any(|e| matches!(e, SessionEvent::Reconnected { .. })));
    }

    #[tokio::test(start_paused = true)]
    async fn close_reports_one_exit_and_stops_the_round() {
        let h = harness(vec![]);
        h.emit(0, SessionEvent::Lost(net()));
        advance(0).await;
        h.session.close().await.unwrap();
        h.session.close().await.unwrap();
        advance(120).await;
        assert_eq!(h.events(), vec![reconnecting(1, 1), SessionEvent::Exit { code: None }]);
        assert_eq!(h.attempts(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn closing_a_connected_session_closes_the_inner_one() {
        let h = harness(vec![]);
        h.session.close().await.unwrap();
        assert!(h.sessions.lock().unwrap()[0].closed.load(Ordering::SeqCst));
        // The inner session's own Exit after close is stale and not reported twice.
        h.emit(0, SessionEvent::Exit { code: None });
        assert_eq!(h.events(), vec![SessionEvent::Exit { code: None }]);
    }

    #[tokio::test(start_paused = true)]
    async fn input_is_dropped_and_the_last_size_is_used_while_reconnecting() {
        let h = harness(vec![Ok(())]);
        h.emit(0, SessionEvent::Lost(net()));
        h.session.write(b"lost").await.unwrap();
        h.session.resize(120, 40).await.unwrap();
        advance(1).await;
        assert_eq!(*h.attempts.lock().unwrap(), vec![(120, 40)]);
        assert!(h.sessions.lock().unwrap()[0].writes.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_resize_during_an_attempt_is_applied_after_it() {
        let h = harness(vec![Ok(())]);
        let gate = Arc::new(Notify::new());
        *h.gate.lock().unwrap() = Some(gate.clone());
        h.emit(0, SessionEvent::Lost(net()));
        advance(1).await;
        h.session.resize(100, 30).await.unwrap();
        gate.notify_one();
        advance(0).await;
        assert_eq!(*h.sessions.lock().unwrap()[1].sizes.lock().unwrap(), vec![(100, 30)]);
    }

    #[tokio::test(start_paused = true)]
    async fn stale_events_of_an_old_connection_are_ignored() {
        let h = harness(vec![Ok(())]);
        h.emit(0, SessionEvent::Lost(net()));
        advance(1).await;
        let before = h.events().len();
        h.emit(0, SessionEvent::Output(b"old".to_vec()));
        h.emit(0, SessionEvent::Lost(net()));
        h.emit(0, SessionEvent::Exit { code: Some(1) });
        advance(60).await;
        assert_eq!(h.events().len(), before);
        assert_eq!(h.attempts(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn output_of_an_attempt_is_held_until_reconnected() {
        let h = harness(vec![Ok(())]);
        let gate = Arc::new(Notify::new());
        *h.gate.lock().unwrap() = Some(gate.clone());
        h.emit(0, SessionEvent::Lost(net()));
        advance(1).await;
        h.emit(1, SessionEvent::Output(b"banner".to_vec()));
        gate.notify_one();
        advance(0).await;
        let events = h.events();
        let n = events.len();
        assert!(matches!(events[n - 2], SessionEvent::Reconnected { .. }));
        assert_eq!(events[n - 1], SessionEvent::Output(b"banner".to_vec()));
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_loss_after_success_starts_again_at_attempt_one() {
        let h = harness(vec![Ok(())]);
        h.emit(0, SessionEvent::Lost(net()));
        advance(1).await;
        h.emit(1, SessionEvent::Lost(net()));
        advance(0).await;
        assert_eq!(h.events().last(), Some(&reconnecting(1, 1)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn input_is_not_blocked_while_the_output_sink_waits() {
        // The app sink blocks while output flow control is paused (`OutputPump::push`).
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let sink: OutputSink = Arc::new(move |event| {
            if matches!(event, SessionEvent::Output(_)) {
                let _ = wait.lock().unwrap().recv();
            }
        });
        let connector: Connector = Arc::new(|_, _, _, _| Box::pin(async { Err(CoreError::Cancelled) }));
        let session = Arc::new(ReconnectingSession::new(sink, 80, 24, connector, Arc::new(|| true)));
        let first = session.first_sink();
        let inner = Arc::new(FakeSession::default());
        session.attach(inner.clone());
        let reader = std::thread::spawn(move || first(SessionEvent::Output(b"flood".to_vec())));
        std::thread::sleep(Duration::from_millis(50));
        let writer = {
            let session = session.clone();
            tokio::spawn(async move {
                session.write(b"\x03").await.unwrap();
                session.resize(100, 30).await.unwrap();
            })
        };
        let done = tokio::time::timeout(Duration::from_secs(1), writer).await;
        release.send(()).unwrap();
        reader.join().unwrap();
        assert!(done.is_ok(), "Ctrl+C must not wait for the output flow control");
        assert_eq!(*inner.writes.lock().unwrap(), vec![b"\x03".to_vec()]);
        assert_eq!(*inner.sizes.lock().unwrap(), vec![(100, 30)]);
    }

    #[tokio::test(start_paused = true)]
    async fn output_of_a_shell_that_ends_during_its_setup_is_kept() {
        let events: Arc<Mutex<Vec<SessionEvent>>> = Arc::default();
        let sink: OutputSink = {
            let events = events.clone();
            Arc::new(move |event| events.lock().unwrap().push(event))
        };
        let connector: Connector = Arc::new(|_, _, _, _| Box::pin(async { Err(CoreError::Cancelled) }));
        let session = ReconnectingSession::new(sink, 80, 24, connector, Arc::new(|| true));
        let first = session.first_sink();
        first(SessionEvent::Output(b"no shell access".to_vec()));
        first(SessionEvent::Exit { code: Some(1) });
        assert_eq!(
            *events.lock().unwrap(),
            vec![SessionEvent::Output(b"no shell access".to_vec()), SessionEvent::Exit { code: Some(1) }]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_now_reports_the_attempt_as_starting() {
        let h = harness(vec![]);
        h.emit(0, SessionEvent::Lost(net()));
        advance(0).await;
        h.session.reconnect_now();
        advance(0).await;
        // The bar must not keep counting down while the attempt already runs.
        assert_eq!(h.events()[1], reconnecting(1, 0));
    }

    #[test]
    fn a_rejected_host_key_is_fatal() {
        assert!(is_fatal(&CoreError::HostKeyRejected { message: "revoked".into() }));
        assert!(!is_fatal(&net()));
    }

    #[tokio::test(start_paused = true)]
    async fn held_output_stops_at_the_cap_without_holes() {
        let events: Arc<Mutex<Vec<SessionEvent>>> = Arc::default();
        let sink: OutputSink = {
            let events = events.clone();
            Arc::new(move |event| events.lock().unwrap().push(event))
        };
        let connector: Connector = Arc::new(|_, _, _, _| Box::pin(async { Err(CoreError::Cancelled) }));
        let session = ReconnectingSession::new(sink, 80, 24, connector, Arc::new(|| true));
        let first = session.first_sink();
        first(SessionEvent::Output(vec![b'a'; MAX_PENDING_OUTPUT - 1]));
        first(SessionEvent::Output(b"bc".to_vec()));
        first(SessionEvent::Output(b"d".to_vec()));
        session.attach(Arc::new(FakeSession::default()));
        assert_eq!(*events.lock().unwrap(), vec![SessionEvent::Output(vec![b'a'; MAX_PENDING_OUTPUT - 1])]);
    }
}
