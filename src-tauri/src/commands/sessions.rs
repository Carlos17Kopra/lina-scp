use std::sync::Arc;

use linascp_core::error::{CoreError, Result};
use linascp_core::model::Id;
use linascp_core::session::{LocalPtySession, OutputSink, Session, SessionEvent, SessionId};
use linascp_core::tunnel::{TunnelEvent, Tunnels};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use crate::output::{OutputPump, RawChunk};
use crate::state::AppState;
use crate::util::lock;

/// Non-output session events, sent on the `session_events` channel.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionEventDto {
    Exit { code: Option<u32> },
    Error { error: CoreError },
    Reconnecting { attempt: u32, max_attempts: u32, retry_in_ms: u32, reason: CoreError },
    Reconnected { info: linascp_core::connect::SessionInfo },
    Lost { reason: CoreError },
    Tunnel { event: TunnelEvent },
}

pub(crate) type EventSender = Arc<dyn Fn(SessionEventDto) + Send + Sync>;

/// Removes a session from `AppState` once it is no longer running. Captured by that session's
/// own output sink (see `make_sink`), which can be invoked from a background thread/task long
/// after the command that created it returned — so this closure owns cheap, `Arc`-cloned
/// handles to just the two maps it needs, never a borrow of `AppState` itself or a Tauri
/// `AppHandle` (which would make it easy to accidentally create a reference cycle back into
/// the session it is attached to).
pub(crate) type Cleanup = Arc<dyn Fn() + Send + Sync>;

pub(crate) fn channel_event_sender(channel: Channel<SessionEventDto>) -> EventSender {
    Arc::new(move |event: SessionEventDto| {
        if let Err(error) = channel.send(event) {
            tracing::debug!(%error, "session event dropped: channel closed");
        }
    })
}

/// Core `OutputSink` that feeds the pump, reports exit/error after flushing pending output,
/// and removes the session from `AppState` (self-cleanup) so a session that ends on its own —
/// the shell exits, the SSH connection drops — is not kept alive until an explicit
/// `session_close` call. `Lost` and reconnect events keep the session registered (it can
/// reconnect again).
pub(crate) fn make_sink(pump: Arc<OutputPump>, send_event: EventSender, cleanup: Cleanup) -> OutputSink {
    Arc::new(move |event: SessionEvent| match event {
        SessionEvent::Output(bytes) => pump.push(&bytes),
        SessionEvent::Exit { code } => {
            pump.close();
            cleanup();
            send_event(SessionEventDto::Exit { code });
        }
        SessionEvent::Error(error) => {
            pump.close();
            cleanup();
            send_event(SessionEventDto::Error { error });
        }
        SessionEvent::Lost(reason) => send_event(SessionEventDto::Lost { reason }),
        SessionEvent::Reconnecting { attempt, max_attempts, retry_in, reason } => {
            let retry_in_ms = u32::try_from(retry_in.as_millis()).unwrap_or(u32::MAX);
            send_event(SessionEventDto::Reconnecting { attempt, max_attempts, retry_in_ms, reason })
        }
        SessionEvent::Reconnected { info } => send_event(SessionEventDto::Reconnected { info }),
    })
}

/// Builds the self-cleanup closure for session `id` ahead of time (the id is minted before the
/// session itself exists, precisely so this closure — handed to the session's sink — can
/// already know what to remove once it eventually stops running).
pub(crate) fn cleanup_fn(state: &AppState, id: SessionId) -> Cleanup {
    let sessions = state.sessions.clone();
    let outputs = state.outputs.clone();
    let tunnels = state.tunnels.clone();
    Arc::new(move || {
        sessions.remove(id);
        lock(&outputs).remove(&id);
        // Dropping them closes their listeners and connections.
        lock(&tunnels).remove(&id);
    })
}

pub(crate) fn register(state: &AppState, id: SessionId, session: Arc<dyn Session>, pump: Arc<OutputPump>) {
    state.sessions.insert(id, session);
    lock(&state.outputs).insert(id, pump);
}

/// Removes session `id` from both maps, returning whatever was present. Shared by the explicit
/// `session_close` path and by `heal_if_already_exited`'s race-closing re-check.
pub(crate) fn remove_session(state: &AppState, id: SessionId) -> (Option<Arc<dyn Session>>, Option<Arc<OutputPump>>) {
    lock(&state.tunnels).remove(&id);
    (state.sessions.remove(id), lock(&state.outputs).remove(&id))
}

/// Closes the narrow window between spawning/connecting a session and registering it: if the
/// session already exited by the time we get here (the sink already ran `cleanup`, which found
/// nothing yet to remove because `register` had not run), the entry `register` just inserted
/// would otherwise never be cleaned up again. `OutputPump::close` is always the first thing the
/// sink does on `Exit`/`Error`, strictly before `cleanup`, so `pump.is_closed()` is a reliable
/// (mutex-synchronized) signal that cleanup has run or is about to.
pub(crate) fn heal_if_already_exited(state: &AppState, id: SessionId, pump: &OutputPump) {
    if pump.is_closed() {
        remove_session(state, id);
    }
}

fn get_session(state: &AppState, id: SessionId) -> Result<Arc<dyn Session>> {
    state.sessions.get(id).ok_or_else(|| CoreError::Internal { message: format!("unknown session {id}") })
}

pub(crate) fn open_local(
    state: &AppState,
    cols: u16,
    rows: u16,
    pump: Arc<OutputPump>,
    send_event: EventSender,
) -> Result<SessionId> {
    let id = Uuid::new_v4();
    let sink = make_sink(pump.clone(), send_event, cleanup_fn(state, id));
    let session = match &state.local_shell {
        Some(shell) => LocalPtySession::spawn_shell(shell, cols, rows, sink)?,
        None => LocalPtySession::spawn(cols, rows, sink)?,
    };
    register(state, id, Arc::new(session), pump.clone());
    heal_if_already_exited(state, id, &pump);
    Ok(id)
}

pub(crate) async fn write(state: &AppState, id: SessionId, data: &[u8]) -> Result<()> {
    get_session(state, id)?.write(data).await
}

pub(crate) async fn resize(state: &AppState, id: SessionId, cols: u16, rows: u16) -> Result<()> {
    get_session(state, id)?.resize(cols, rows).await
}

/// Unknown ids are ignored (the session may just have been closed).
pub(crate) fn ack(state: &AppState, id: SessionId, bytes: u32) {
    let pump = lock(&state.outputs).get(&id).cloned();
    if let Some(pump) = pump {
        pump.ack(u64::from(bytes));
    }
}

/// Closing an unknown (or already self-closed) session is not an error.
pub(crate) async fn close(state: &AppState, id: SessionId) -> Result<()> {
    let (session, pump) = remove_session(state, id);
    if let Some(pump) = pump {
        pump.close();
    }
    match session {
        Some(session) => session.close().await,
        None => Ok(()),
    }
}

/// Unknown or local sessions: nothing to do.
pub(crate) fn reconnect_now(state: &AppState, id: SessionId) {
    if let Some(session) = state.sessions.get(id) {
        session.reconnect_now();
    }
}

pub(crate) fn reconnect_stop(state: &AppState, id: SessionId) {
    if let Some(session) = state.sessions.get(id) {
        session.stop_reconnect();
    }
}

fn get_tunnels(state: &AppState, id: SessionId) -> Result<Arc<Tunnels>> {
    lock(&state.tunnels)
        .get(&id)
        .cloned()
        .ok_or_else(|| CoreError::Internal { message: format!("unknown session {id}") })
}

pub(crate) async fn tunnel_start_inner(state: &AppState, id: SessionId, rule_id: Id) -> Result<()> {
    get_tunnels(state, id)?.start(rule_id).await
}

pub(crate) async fn tunnel_stop_inner(state: &AppState, id: SessionId, rule_id: Id) -> Result<()> {
    get_tunnels(state, id)?.stop(rule_id).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn tunnel_start(state: State<'_, AppState>, session_id: SessionId, rule_id: Id) -> Result<()> {
    tunnel_start_inner(&state, session_id, rule_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn tunnel_stop(state: State<'_, AppState>, session_id: SessionId, rule_id: Id) -> Result<()> {
    tunnel_stop_inner(&state, session_id, rule_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn session_open_local(
    state: State<'_, AppState>,
    cols: u16,
    rows: u16,
    output: Channel<RawChunk>,
    session_events: Channel<SessionEventDto>,
) -> Result<SessionId> {
    open_local(&state, cols, rows, OutputPump::for_channel(output), channel_event_sender(session_events))
}

#[tauri::command]
#[specta::specta]
pub async fn session_write(state: State<'_, AppState>, session_id: SessionId, data: Vec<u8>) -> Result<()> {
    write(&state, session_id, &data).await
}

#[tauri::command]
#[specta::specta]
pub async fn session_resize(state: State<'_, AppState>, session_id: SessionId, cols: u16, rows: u16) -> Result<()> {
    resize(&state, session_id, cols, rows).await
}

#[tauri::command]
#[specta::specta]
pub async fn session_ack(state: State<'_, AppState>, session_id: SessionId, bytes: u32) -> Result<()> {
    ack(&state, session_id, bytes);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn session_close(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    close(&state, session_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn session_reconnect_now(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    reconnect_now(&state, session_id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn session_reconnect_stop(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    reconnect_stop(&state, session_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    type Bytes = Arc<Mutex<Vec<u8>>>;
    type Events = Arc<Mutex<Vec<SessionEventDto>>>;

    fn collecting_pump() -> (Arc<OutputPump>, Bytes) {
        let bytes: Bytes = Arc::default();
        let sink = bytes.clone();
        let pump = OutputPump::new(Arc::new(move |chunk: Vec<u8>| sink.lock().unwrap().extend_from_slice(&chunk)));
        (pump, bytes)
    }

    fn collecting_events() -> (EventSender, Events) {
        let events: Events = Arc::default();
        let sink = events.clone();
        (Arc::new(move |event: SessionEventDto| sink.lock().unwrap().push(event)), events)
    }

    /// A `Cleanup` that just counts its calls, for tests that only care whether `make_sink`
    /// invokes it (the end-to-end removal itself is covered by
    /// `a_session_that_exits_on_its_own_is_removed_from_both_maps`).
    fn counting_cleanup() -> (Cleanup, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        (
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
            calls,
        )
    }

    #[tokio::test]
    async fn reconnect_commands_ignore_unknown_and_local_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        reconnect_now(&state, Uuid::new_v4());
        reconnect_stop(&state, Uuid::new_v4());
        let (pump, _bytes) = collecting_pump();
        let (send, _events) = collecting_events();
        let id = open_local(&state, 80, 24, pump, send).unwrap();
        reconnect_now(&state, id);
        reconnect_stop(&state, id);
        close(&state, id).await.unwrap();
    }

    #[test]
    fn sink_reports_reconnect_events_without_cleanup() {
        let (pump, _bytes) = collecting_pump();
        let (send, events) = collecting_events();
        let (cleanup, calls) = counting_cleanup();
        let sink = make_sink(pump.clone(), send, cleanup);
        let reason = CoreError::Network { kind: linascp_core::error::NetworkKind::Other, message: "gone".into() };
        sink(SessionEvent::Reconnecting {
            attempt: 2,
            max_attempts: 8,
            retry_in: std::time::Duration::from_millis(2000),
            reason: reason.clone(),
        });
        sink(SessionEvent::Reconnected { info: linascp_core::connect::SessionInfo::default() });
        sink(SessionEvent::Lost(reason.clone()));
        assert!(!pump.is_closed(), "the session stays usable for \"Erneut versuchen\"");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "lost must not unregister the session");
        let events = events.lock().unwrap();
        assert!(matches!(
            events.as_slice(),
            [
                SessionEventDto::Reconnecting { attempt: 2, max_attempts: 8, retry_in_ms: 2000, .. },
                SessionEventDto::Reconnected { .. },
                SessionEventDto::Lost { .. },
            ]
        ));
        let json = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(json["kind"], "reconnecting");
        assert_eq!(json["retryInMs"], 2000);
        assert_eq!(json["maxAttempts"], 8);
    }

    #[test]
    fn sink_routes_output_and_flushes_before_exit() {
        let (pump, bytes) = collecting_pump();
        let (send, events) = collecting_events();
        let (cleanup, calls) = counting_cleanup();
        let sink = make_sink(pump.clone(), send, cleanup);

        sink(SessionEvent::Output(b"hello".to_vec()));
        assert!(bytes.lock().unwrap().is_empty(), "small output waits for the flush tick");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "plain output must not trigger cleanup");

        sink(SessionEvent::Exit { code: Some(0) });
        assert_eq!(*bytes.lock().unwrap(), b"hello".to_vec());
        assert!(pump.is_closed());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "exit must trigger cleanup exactly once");
        assert!(matches!(events.lock().unwrap().as_slice(), [SessionEventDto::Exit { code: Some(0) }]));
    }

    #[test]
    fn sink_reports_session_errors_and_cleans_up() {
        let (pump, _bytes) = collecting_pump();
        let (send, events) = collecting_events();
        let (cleanup, calls) = counting_cleanup();
        make_sink(pump, send, cleanup)(SessionEvent::Error(CoreError::Cancelled));
        assert!(matches!(events.lock().unwrap().as_slice(), [SessionEventDto::Error { error: CoreError::Cancelled }]));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn session_event_dto_uses_kind_tag() {
        let exit = serde_json::to_value(SessionEventDto::Exit { code: Some(3) }).unwrap();
        assert_eq!(exit, serde_json::json!({ "kind": "exit", "code": 3 }));
        let error = serde_json::to_value(SessionEventDto::Error { error: CoreError::Cancelled }).unwrap();
        assert_eq!(error, serde_json::json!({ "kind": "error", "error": { "type": "cancelled" } }));
    }

    #[tokio::test]
    async fn unknown_sessions_are_reported_and_closing_them_is_harmless() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let id = Uuid::new_v4();
        assert!(matches!(write(&state, id, b"x").await, Err(CoreError::Internal { .. })));
        assert!(matches!(resize(&state, id, 80, 24).await, Err(CoreError::Internal { .. })));
        ack(&state, id, 10);
        close(&state, id).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_pty_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let (pump, bytes) = collecting_pump();
        pump.start_ticker();
        let (send, _events) = collecting_events();

        let id = open_local(&state, 80, 24, pump, send).unwrap();
        write(&state, id, b"echo linascp-$((40+2))\n").await.unwrap();

        let mut found = false;
        for _ in 0..200 {
            if String::from_utf8_lossy(&bytes.lock().unwrap()).contains("linascp-42") {
                found = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(found, "shell output not received");

        resize(&state, id, 100, 30).await.unwrap();
        ack(&state, id, 64);
        close(&state, id).await.unwrap();
        assert!(state.sessions.get(id).is_none());
        assert!(state.outputs.lock().unwrap().get(&id).is_none());
    }

    /// I2: a session that exits on its own (nobody ever calls `session_close`) must still be
    /// removed from both `AppState` maps, so the (for SSH: `russh`) connection is not kept
    /// alive forever.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_session_that_exits_on_its_own_is_removed_from_both_maps() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let (pump, _bytes) = collecting_pump();
        pump.start_ticker();
        let (send, events) = collecting_events();

        let id = open_local(&state, 80, 24, pump, send).unwrap();
        write(&state, id, b"exit 0\n").await.unwrap();

        let mut removed = false;
        for _ in 0..200 {
            if state.sessions.get(id).is_none() && state.outputs.lock().unwrap().get(&id).is_none() {
                removed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(removed, "a session that exited on its own must be removed from both maps");
        assert!(matches!(events.lock().unwrap().as_slice(), [SessionEventDto::Exit { .. }]));
    }

    #[tokio::test]
    async fn tunnel_commands_need_a_known_session_and_tunnels_end_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        let (id, rule) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        assert!(matches!(tunnel_start_inner(&state, id, rule).await, Err(CoreError::Internal { .. })));
        assert!(matches!(tunnel_stop_inner(&state, id, rule).await, Err(CoreError::Internal { .. })));

        let tunnels = || Arc::new(Tunnels::new(Vec::new(), Arc::new(|_| {})));
        lock(&state.tunnels).insert(id, tunnels());
        cleanup_fn(&state, id)();
        assert!(lock(&state.tunnels).is_empty(), "a session that ends on its own takes its tunnels along");
        lock(&state.tunnels).insert(id, tunnels());
        close(&state, id).await.unwrap();
        assert!(lock(&state.tunnels).is_empty(), "closing the session ends its tunnels");
    }
}
