use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use linascp_core::connect::{
    ConnectDeps, ConnectEvent, ConnectRequest, ConnectTarget, Connector, EmitFn, Enabled, PromptAnswer, PromptBroker,
    ReconnectingSession, connect_ssh as run_pipeline, test_connection,
};
use linascp_core::error::{CoreError, Result};
use linascp_core::known_hosts::KnownHosts;
use linascp_core::model::{Host, HostAuth, Id};
use linascp_core::session::Session;
use linascp_core::tunnel::Tunnels;
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::sessions::{SessionEventDto, channel_event_sender, cleanup_fn, heal_if_already_exited, make_sink, register};
use super::settings::auto_reconnect_enabled;
use crate::dto::ConnectedSession;
use crate::output::{OutputPump, RawChunk};
use crate::state::{AppState, ConnectEnv};
use crate::util::lock;

/// Looks a host up in the merged list (stored + ssh-config).
pub(crate) fn find_host(state: &AppState, id: Id) -> Result<Host> {
    find_host_in(&state.connect_env(), id)
}

pub(crate) fn find_host_in(env: &ConnectEnv, id: Id) -> Result<Host> {
    env.merged_hosts()?
        .into_iter()
        .find(|h| h.id == id)
        .ok_or_else(|| CoreError::Config { message: format!("host {id} not found") })
}

fn target_for_in(env: &ConnectEnv, host: Host) -> Result<ConnectTarget> {
    let profile = match &host.auth {
        HostAuth::Profile { profile_id } => Some(env.store.get_profile(*profile_id)?.ok_or_else(|| {
            CoreError::Config { message: format!("login profile {profile_id} of host {} not found", host.name) }
        })?),
        HostAuth::Custom { .. } => None,
    };
    Ok(ConnectTarget { host, profile })
}

/// Resolves the login profile and the (single-hop) jump host of `host`.
pub(crate) fn build_request(state: &AppState, host: Host, cols: u16, rows: u16) -> Result<ConnectRequest> {
    build_request_in(&state.connect_env(), host, cols, rows)
}

pub(crate) fn build_request_in(env: &ConnectEnv, host: Host, cols: u16, rows: u16) -> Result<ConnectRequest> {
    let jump = match host.jump_host_id {
        Some(jump_id) => {
            let jump_host = find_host_in(env, jump_id).map_err(|_| CoreError::Config {
                message: format!("jump host {jump_id} of host {} not found", host.name),
            })?;
            Some(target_for_in(env, jump_host)?)
        }
        None => None,
    };
    Ok(ConnectRequest { target: target_for_in(env, host)?, jump, cols, rows })
}

/// RAII guard for a registered connect attempt: removes the `connects` entry when dropped, but
/// only if it still holds the exact token this guard registered. That guards against two
/// problems a plain "register, then manually unregister after `.await`" pair has: a reused
/// `connect_id` racing a stale guard's cleanup (the earlier guard must not evict a *later*,
/// still-running registration under the same id), and the unregister step being skipped
/// entirely if the awaited future panics.
pub(crate) struct ConnectGuard<'a> {
    state: &'a AppState,
    connect_id: Uuid,
    token: CancellationToken,
}

impl ConnectGuard<'_> {
    /// A clone of the token this guard registered (cheap; `CancellationToken` is `Arc`-backed).
    pub(crate) fn token(&self) -> CancellationToken {
        self.token.clone()
    }
}

impl Deref for ConnectGuard<'_> {
    type Target = CancellationToken;

    fn deref(&self) -> &CancellationToken {
        &self.token
    }
}

impl Drop for ConnectGuard<'_> {
    fn drop(&mut self) {
        let mut connects = lock(&self.state.connects);
        // `CancellationToken`'s `PartialEq` is `Arc::ptr_eq`-based (see tokio-util), so this
        // only matches the registration this exact guard created.
        if connects.get(&self.connect_id) == Some(&self.token) {
            connects.remove(&self.connect_id);
        }
    }
}

const EARLY_CANCEL_TTL: Duration = Duration::from_secs(60);

/// Registers a cancellable connection attempt under the UI-generated `connect_id`. The
/// returned guard un-registers it again when dropped (success, error, or panic). If a cancel
/// for this id arrived first (see `cancel_connect`), the attempt starts out cancelled.
pub(crate) fn register_connect(state: &AppState, connect_id: Uuid) -> ConnectGuard<'_> {
    let mut connects = lock(&state.connects);
    let token = CancellationToken::new();
    if lock(&state.early_cancels).remove(&connect_id).is_some() {
        token.cancel();
    }
    connects.insert(connect_id, token.clone());
    drop(connects);
    ConnectGuard { state, connect_id, token }
}

/// Cancels the attempt. Tauri runs invokes concurrently, so the cancel can arrive before the
/// connect invoke registered its id: then it is noted for `register_connect`. Every attempt has
/// a fresh id, so such a note never hits a later attempt.
pub(crate) fn cancel_connect(state: &AppState, connect_id: Uuid) {
    let connects = lock(&state.connects);
    if let Some(token) = connects.get(&connect_id) {
        token.cancel();
        return;
    }
    let mut early = lock(&state.early_cancels);
    // Notes of cancels that came after their attempt ended are never claimed; an attempt that
    // is still to register does so within milliseconds, so a minute is plenty.
    early.retain(|_, noted| noted.elapsed() < EARLY_CANCEL_TTL);
    early.insert(connect_id, Instant::now());
}

pub(crate) fn connect_deps(state: &AppState) -> ConnectDeps {
    connect_deps_in(&state.connect_env())
}

pub(crate) fn connect_deps_in(env: &ConnectEnv) -> ConnectDeps {
    ConnectDeps {
        known_hosts: KnownHosts::new(env.paths.known_hosts()),
        secrets: env.secrets.clone(),
        agent_sock: std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from),
        forwarded: None,
    }
}

/// Reconnect attempts: request rebuilt from the current store/keyring, pipeline without prompts.
/// A refused prompt becomes `CoreError::InputRequired` (fail closed, nothing reaches the UI).
/// The session's tunnels move over to each new connection.
fn reconnect_connector(env: ConnectEnv, host_id: Id, tunnels: Weak<Tunnels>) -> Connector {
    Arc::new(move |cols, rows, sink, cancel| {
        let env = env.clone();
        let tunnels = tunnels.clone();
        Box::pin(async move {
            let host = find_host_in(&env, host_id)?;
            let request = build_request_in(&env, host, cols, rows)?;
            let broker = Arc::new(PromptBroker::non_interactive());
            let quiet: EmitFn = Arc::new(|_| {});
            let mut deps = connect_deps_in(&env);
            deps.forwarded = tunnels.upgrade().map(|t| t.routes());
            match run_pipeline(request, deps, broker.clone(), quiet, sink, cancel).await {
                Ok(session) => {
                    if let Some(tunnels) = tunnels.upgrade() {
                        tunnels.attach(session.link()).await;
                    }
                    let info = session.info().clone();
                    Ok((Arc::new(session) as Arc<dyn Session>, info))
                }
                Err(_) if broker.refused_input() => Err(CoreError::InputRequired),
                Err(error) => Err(error),
            }
        })
    })
}

pub(crate) fn channel_emit(channel: Channel<ConnectEvent>) -> EmitFn {
    Arc::new(move |event: ConnectEvent| {
        if let Err(error) = channel.send(event) {
            tracing::debug!(%error, "connect event dropped: channel closed");
        }
    })
}

/// Emits `Failed` and returns the same error. Used for failures the core pipeline never sees
/// (host lookup, profile/jump-host resolution) — without this, the UI's connect screen would
/// hang forever waiting for a terminal event that never comes, since the command just rejects
/// its promise and `events` stays silent.
pub(crate) fn fail<T>(emit: &EmitFn, error: CoreError) -> Result<T> {
    emit(ConnectEvent::Failed { error: error.clone() });
    Err(error)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn connect_ssh_inner(
    state: &AppState,
    connect_id: Uuid,
    host_id: Id,
    cols: u16,
    rows: u16,
    emit: EmitFn,
    output: Channel<RawChunk>,
    session_events: Channel<SessionEventDto>,
) -> Result<ConnectedSession> {
    // Register first: a cancel issued while the host is looked up must not be lost.
    let guard = register_connect(state, connect_id);
    connect_ssh_registered(state, guard, host_id, cols, rows, emit, output, session_events).await
}

#[allow(clippy::too_many_arguments)]
async fn connect_ssh_registered(
    state: &AppState,
    guard: ConnectGuard<'_>,
    host_id: Id,
    cols: u16,
    rows: u16,
    emit: EmitFn,
    output: Channel<RawChunk>,
    session_events: Channel<SessionEventDto>,
) -> Result<ConnectedSession> {
    let host = match find_host(state, host_id) {
        Ok(host) => host,
        Err(error) => return fail(&emit, error),
    };
    let request = match build_request(state, host, cols, rows) {
        Ok(request) => request,
        Err(error) => return fail(&emit, error),
    };
    if guard.is_cancelled() {
        // Cancelled during the lookup: never start the pipeline (one terminal event).
        return fail(&emit, CoreError::Cancelled);
    }

    let session_id = Uuid::new_v4();
    let pump = OutputPump::for_channel(output);
    let send_event = channel_event_sender(session_events);
    let forwards = request.target.host.forwards.clone();
    let tunnel_events = send_event.clone();
    let tunnels = Arc::new(Tunnels::new(
        forwards.clone(),
        Arc::new(move |event| tunnel_events(SessionEventDto::Tunnel { event })),
    ));
    let sink = make_sink(pump.clone(), send_event, cleanup_fn(state, session_id));
    let store = state.store.clone();
    let enabled: Enabled = Arc::new(move || auto_reconnect_enabled(&store));
    let reconnecting = Arc::new(ReconnectingSession::new(
        sink,
        cols,
        rows,
        reconnect_connector(state.connect_env(), host_id, Arc::downgrade(&tunnels)),
        enabled,
    ));
    let cancel = guard.token();
    let mut deps = connect_deps(state);
    deps.forwarded = Some(tunnels.routes());
    let result =
        run_pipeline(request, deps, state.broker.clone(), emit.clone(), reconnecting.first_sink(), cancel.clone())
            .await;
    drop(guard);

    match result {
        // The attempt was cancelled right as (or just after) the pipeline finished: the core
        // pipeline only checks cancellation between steps, so a cancel landing during the last
        // step can still produce `Ok`. Don't surface a session the UI already gave up on.
        Ok(session) if cancel.is_cancelled() => {
            pump.close();
            let _ = session.close().await;
            fail(&emit, CoreError::Cancelled)
        }
        Ok(session) => {
            let info = session.info().clone();
            let link = session.link();
            reconnecting.attach(Arc::new(session));
            lock(&state.tunnels).insert(session_id, tunnels.clone());
            register(state, session_id, reconnecting, pump.clone());
            heal_if_already_exited(state, session_id, &pump);
            // core emits `Failed` on errors; the success event is ours (exactly one terminal event).
            emit(ConnectEvent::Connected { session_id });
            // Autostart after `Connected`: its events belong to a tab that exists by then.
            tokio::spawn(async move { tunnels.attach(link).await });
            Ok(ConnectedSession { session_id, info, forwards })
        }
        Err(error) => {
            pump.close();
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
#[specta::specta]
pub async fn connect_ssh(
    state: State<'_, AppState>,
    connect_id: Uuid,
    host_id: Id,
    cols: u16,
    rows: u16,
    events: Channel<ConnectEvent>,
    output: Channel<RawChunk>,
    session_events: Channel<SessionEventDto>,
) -> Result<ConnectedSession> {
    connect_ssh_inner(&state, connect_id, host_id, cols, rows, channel_emit(events), output, session_events).await
}

/// Returns `false` when the prompt id is unknown (already answered, timed out or cancelled).
#[tauri::command]
#[specta::specta]
pub async fn connect_answer(state: State<'_, AppState>, prompt_id: u64, answer: PromptAnswer) -> Result<bool> {
    Ok(state.broker.answer(prompt_id, answer))
}

#[tauri::command]
#[specta::specta]
pub async fn connect_cancel(state: State<'_, AppState>, connect_id: Uuid) -> Result<()> {
    cancel_connect(&state, connect_id);
    Ok(())
}

pub(crate) async fn connect_test_inner(state: &AppState, connect_id: Uuid, host: Host, emit: EmitFn) -> Result<()> {
    // Register first: a cancel issued while the request is resolved must not be lost.
    let guard = register_connect(state, connect_id);
    connect_test_registered(state, guard, host, emit).await
}

async fn connect_test_registered(state: &AppState, guard: ConnectGuard<'_>, host: Host, emit: EmitFn) -> Result<()> {
    let request = match build_request(state, host, 80, 24) {
        Ok(request) => request,
        Err(error) => return fail(&emit, error),
    };
    if guard.is_cancelled() {
        return fail(&emit, CoreError::Cancelled);
    }
    let cancel = guard.token();
    // core's `test_connection` already emits `Failed` on error; nothing more to do here.
    let result = test_connection(request, connect_deps(state), state.broker.clone(), emit, cancel).await;
    drop(guard);
    result
}

/// "Verbindung testen": pipeline for an (unsaved) host up to Auth, then disconnect (core `test_connection`).
#[tauri::command]
#[specta::specta]
pub async fn connect_test(
    state: State<'_, AppState>,
    connect_id: Uuid,
    host: Host,
    events: Channel<ConnectEvent>,
) -> Result<()> {
    connect_test_inner(&state, connect_id, host, channel_emit(events)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::hosts::save_host;
    use crate::commands::profiles::{save_profile, tests::profile};
    use linascp_core::connect::Prompt;
    use std::sync::Mutex;
    use std::time::Duration;

    fn setup() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        (dir, state)
    }

    #[tokio::test]
    async fn reconnect_connector_reports_a_deleted_host_as_config_error() {
        let (_dir, state) = setup();
        let connector = reconnect_connector(state.connect_env(), Uuid::new_v4(), Weak::new());
        let sink: linascp_core::session::OutputSink = Arc::new(|_| {});
        let result = connector(80, 24, sink, CancellationToken::new()).await;
        assert!(matches!(result, Err(CoreError::Config { .. })));
    }

    #[test]
    fn build_request_resolves_profile_and_jump_host() {
        let (_dir, state) = setup();
        let p = save_profile(&state, profile("deploy")).unwrap();
        let bastion = save_host(&state, Host::new("bastion", "192.0.2.1")).unwrap();
        let mut web = Host::new("web-prod-01", "10.0.1.12");
        web.auth = HostAuth::Profile { profile_id: p.id };
        web.jump_host_id = Some(bastion.id);
        let web = save_host(&state, web).unwrap();

        let req = build_request(&state, find_host(&state, web.id).unwrap(), 140, 38).unwrap();
        assert_eq!(req.target.host.id, web.id);
        assert_eq!(req.target.profile.as_ref().map(|p| p.id), Some(p.id));
        let jump = req.jump.expect("jump target");
        assert_eq!(jump.host.id, bastion.id);
        assert!(jump.profile.is_none());
        assert_eq!((req.cols, req.rows), (140, 38));
    }

    #[test]
    fn missing_profile_or_jump_host_is_a_config_error() {
        let (_dir, state) = setup();
        let mut host = Host::new("orphan", "192.0.2.2");
        host.auth = HostAuth::Profile { profile_id: Uuid::new_v4() };
        assert!(matches!(build_request(&state, host, 80, 24), Err(CoreError::Config { .. })));

        let mut host = Host::new("lost-jump", "192.0.2.3");
        host.jump_host_id = Some(Uuid::new_v4());
        assert!(matches!(build_request(&state, host, 80, 24), Err(CoreError::Config { .. })));

        assert!(matches!(find_host(&state, Uuid::new_v4()), Err(CoreError::Config { .. })));
    }

    #[test]
    fn cancel_triggers_only_the_registered_token() {
        let (_dir, state) = setup();
        let connect_id = Uuid::new_v4();
        let guard = register_connect(&state, connect_id);

        cancel_connect(&state, Uuid::new_v4());
        assert!(!guard.is_cancelled(), "other ids must not cancel");
        cancel_connect(&state, connect_id);
        assert!(guard.is_cancelled());

        drop(guard);
        assert!(state.connects.lock().unwrap().is_empty());
    }

    #[test]
    fn many_late_cancels_never_evict_a_pending_early_cancel() {
        let (_dir, state) = setup();
        for _ in 0..100 {
            cancel_connect(&state, Uuid::new_v4());
        }
        let connect_id = Uuid::new_v4();
        cancel_connect(&state, connect_id);
        for _ in 0..100 {
            cancel_connect(&state, Uuid::new_v4());
        }
        assert!(register_connect(&state, connect_id).is_cancelled());
    }

    #[test]
    fn cancel_that_arrives_before_the_registration_is_kept() {
        let (_dir, state) = setup();
        let connect_id = Uuid::new_v4();
        cancel_connect(&state, connect_id);

        let guard = register_connect(&state, connect_id);
        assert!(guard.is_cancelled());
        drop(guard);
        assert!(!register_connect(&state, Uuid::new_v4()).is_cancelled(), "other ids stay unaffected");
    }

    /// M2: dropping a superseded guard (its `connect_id` was reused by a later registration)
    /// must not evict that later, still-live registration.
    #[test]
    fn dropping_a_superseded_guard_does_not_evict_the_new_registration() {
        let (_dir, state) = setup();
        let connect_id = Uuid::new_v4();

        let first = register_connect(&state, connect_id);
        let second = register_connect(&state, connect_id);
        drop(first);
        assert_eq!(
            state.connects.lock().unwrap().get(&connect_id),
            Some(&second.token),
            "the live registration must survive the stale guard's drop"
        );

        drop(second);
        assert!(state.connects.lock().unwrap().is_empty());
    }

    /// M2: a guard whose future panics must still clean up (RAII, not a manual post-`.await`
    /// unregister call).
    #[test]
    fn guard_unregisters_even_if_the_caller_panics() {
        let (_dir, state) = setup();
        let connect_id = Uuid::new_v4();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = register_connect(&state, connect_id);
            panic!("simulated failure while a connect attempt is in flight");
        }));
        assert!(result.is_err());
        assert!(state.connects.lock().unwrap().is_empty());
    }

    /// I1: a host lookup failure (before the core pipeline ever runs) must still produce
    /// exactly one terminal `events` message, or the UI's connect screen would hang forever.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_host_emits_exactly_one_failed_event() {
        let (_dir, state) = setup();
        let events: Arc<Mutex<Vec<ConnectEvent>>> = Arc::default();
        let sink = events.clone();
        let emit: EmitFn = Arc::new(move |e: ConnectEvent| sink.lock().unwrap().push(e));
        let output = Channel::new(|_| Ok(()));
        let session_events = Channel::new(|_| Ok(()));

        let result =
            connect_ssh_inner(&state, Uuid::new_v4(), Uuid::new_v4(), 80, 24, emit, output, session_events).await;

        assert!(matches!(result, Err(CoreError::Config { .. })));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1, "exactly one terminal event");
        assert!(matches!(events.as_slice(), [ConnectEvent::Failed { error: CoreError::Config { .. } }]));
    }

    /// I1, `connect_test` variant: an unresolvable jump host must also produce exactly one
    /// terminal event instead of silently rejecting the command's promise.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_test_with_unknown_jump_host_emits_exactly_one_failed_event() {
        let (_dir, state) = setup();
        let events: Arc<Mutex<Vec<ConnectEvent>>> = Arc::default();
        let sink = events.clone();
        let emit: EmitFn = Arc::new(move |e: ConnectEvent| sink.lock().unwrap().push(e));
        let mut host = Host::new("lost-jump", "192.0.2.9");
        host.jump_host_id = Some(Uuid::new_v4());

        let result = connect_test_inner(&state, Uuid::new_v4(), host, emit).await;

        assert!(matches!(result, Err(CoreError::Config { .. })));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1, "exactly one terminal event");
        assert!(matches!(events.as_slice(), [ConnectEvent::Failed { error: CoreError::Config { .. } }]));
    }

    fn recording_emit() -> (EmitFn, Arc<Mutex<Vec<ConnectEvent>>>) {
        let events: Arc<Mutex<Vec<ConnectEvent>>> = Arc::default();
        let sink = events.clone();
        (Arc::new(move |e: ConnectEvent| sink.lock().unwrap().push(e)), events)
    }

    /// F9: a cancel that lands while the host is still being looked up must not be lost: the
    /// pipeline never starts and exactly one terminal `Failed { Cancelled }` is emitted.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_before_lookup_emits_exactly_one_cancelled_failure() {
        let (_dir, state) = setup();
        let host = save_host(&state, Host::new("web", "192.0.2.10")).unwrap();
        let (emit, events) = recording_emit();
        let connect_id = Uuid::new_v4();

        let guard = register_connect(&state, connect_id);
        cancel_connect(&state, connect_id);
        let result = connect_ssh_registered(
            &state,
            guard,
            host.id,
            80,
            24,
            emit,
            Channel::new(|_| Ok(())),
            Channel::new(|_| Ok(())),
        )
        .await;

        assert!(matches!(result, Err(CoreError::Cancelled)));
        let events = events.lock().unwrap();
        assert!(
            matches!(events.as_slice(), [ConnectEvent::Failed { error: CoreError::Cancelled }]),
            "exactly one terminal event and no pipeline steps, got {events:?}"
        );
        assert!(state.connects.lock().unwrap().is_empty());
    }

    /// F9, `connect_test` variant.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_test_cancel_before_resolve_emits_exactly_one_cancelled_failure() {
        let (_dir, state) = setup();
        let (emit, events) = recording_emit();
        let connect_id = Uuid::new_v4();

        let guard = register_connect(&state, connect_id);
        cancel_connect(&state, connect_id);
        let result = connect_test_registered(&state, guard, Host::new("web", "192.0.2.11"), emit).await;

        assert!(matches!(result, Err(CoreError::Cancelled)));
        let events = events.lock().unwrap();
        assert!(
            matches!(events.as_slice(), [ConnectEvent::Failed { error: CoreError::Cancelled }]),
            "exactly one terminal event and no pipeline steps, got {events:?}"
        );
    }

    /// F9: the token is registered before the lookup, so `connect_ssh_inner` leaves no entry behind
    /// even when the lookup fails.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failed_lookup_unregisters_the_connect() {
        let (_dir, state) = setup();
        let (emit, _events) = recording_emit();
        let _ = connect_ssh_inner(
            &state,
            Uuid::new_v4(),
            Uuid::new_v4(),
            80,
            24,
            emit,
            Channel::new(|_| Ok(())),
            Channel::new(|_| Ok(())),
        )
        .await;
        assert!(state.connects.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn answers_reach_the_global_broker() {
        let (_dir, state) = setup();
        let state = Arc::new(state);
        let events: Arc<Mutex<Vec<ConnectEvent>>> = Arc::default();
        let sink = events.clone();
        let emit: EmitFn = Arc::new(move |e: ConnectEvent| sink.lock().unwrap().push(e));

        let asking = {
            let state = state.clone();
            tokio::spawn(async move {
                let prompt = Prompt::Passphrase { key_path: "/tmp/key".into() };
                state.broker.ask_cancellable(&emit, prompt, &CancellationToken::new()).await
            })
        };
        let prompt_id = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let found = events.lock().unwrap().iter().find_map(|e| match e {
                    ConnectEvent::Prompt { prompt_id, .. } => Some(*prompt_id),
                    _ => None,
                });
                if let Some(id) = found {
                    break id;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("prompt event");

        assert!(!state.broker.answer(prompt_id + 1000, PromptAnswer::Cancel), "unknown prompt id");
        assert!(state.broker.answer(prompt_id, PromptAnswer::Secret { value: "pp".into(), save: false }));
        let answer = asking.await.unwrap();
        assert!(matches!(answer, PromptAnswer::Secret { ref value, save: false } if value == "pp"));
    }
}
