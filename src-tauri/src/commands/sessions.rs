use std::sync::Arc;

use linascp_core::error::{CoreError, Result};
use linascp_core::session::{LocalPtySession, OutputSink, Session, SessionEvent, SessionId};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use crate::output::{OutputPump, RawChunk};
use crate::state::AppState;
use crate::util::lock;

/// Non-output session events, sent on the `session_events` channel.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionEventDto {
    Exit { code: Option<u32> },
    Error { error: CoreError },
}

pub(crate) type EventSender = Arc<dyn Fn(SessionEventDto) + Send + Sync>;

pub(crate) fn channel_event_sender(channel: Channel<SessionEventDto>) -> EventSender {
    Arc::new(move |event: SessionEventDto| {
        if let Err(error) = channel.send(event) {
            tracing::debug!(%error, "session event dropped: channel closed");
        }
    })
}

/// Core `OutputSink` that feeds the pump and reports exit/error after flushing pending output.
pub(crate) fn make_sink(pump: Arc<OutputPump>, send_event: EventSender) -> OutputSink {
    Arc::new(move |event: SessionEvent| match event {
        SessionEvent::Output(bytes) => pump.push(&bytes),
        SessionEvent::Exit { code } => {
            pump.close();
            send_event(SessionEventDto::Exit { code });
        }
        SessionEvent::Error(error) => {
            pump.close();
            send_event(SessionEventDto::Error { error });
        }
    })
}

pub(crate) fn register(state: &AppState, id: SessionId, session: Arc<dyn Session>, pump: Arc<OutputPump>) {
    state.sessions.insert(id, session);
    lock(&state.outputs).insert(id, pump);
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
    let session = LocalPtySession::spawn(cols, rows, make_sink(pump.clone(), send_event))?;
    let id = Uuid::new_v4();
    register(state, id, Arc::new(session), pump);
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

/// Closing an unknown session is not an error.
pub(crate) async fn close(state: &AppState, id: SessionId) -> Result<()> {
    let session = state.sessions.get(id);
    state.sessions.remove(id);
    let pump = lock(&state.outputs).remove(&id);
    if let Some(pump) = pump {
        pump.close();
    }
    match session {
        Some(session) => session.close().await,
        None => Ok(()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
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

    #[test]
    fn sink_routes_output_and_flushes_before_exit() {
        let (pump, bytes) = collecting_pump();
        let (send, events) = collecting_events();
        let sink = make_sink(pump.clone(), send);

        sink(SessionEvent::Output(b"hello".to_vec()));
        assert!(bytes.lock().unwrap().is_empty(), "small output waits for the flush tick");

        sink(SessionEvent::Exit { code: Some(0) });
        assert_eq!(*bytes.lock().unwrap(), b"hello".to_vec());
        assert!(pump.is_closed());
        assert!(matches!(events.lock().unwrap().as_slice(), [SessionEventDto::Exit { code: Some(0) }]));
    }

    #[test]
    fn sink_reports_session_errors() {
        let (pump, _bytes) = collecting_pump();
        let (send, events) = collecting_events();
        make_sink(pump, send)(SessionEvent::Error(CoreError::Cancelled));
        assert!(matches!(events.lock().unwrap().as_slice(), [SessionEventDto::Error { error: CoreError::Cancelled }]));
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
}
