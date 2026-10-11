use std::path::PathBuf;
use std::sync::Arc;

use linascp_core::connect::{
    ConnectDeps, ConnectEvent, ConnectRequest, ConnectTarget, EmitFn, PromptAnswer, connect_ssh as run_pipeline,
    test_connection,
};
use linascp_core::error::{CoreError, Result};
use linascp_core::known_hosts::KnownHosts;
use linascp_core::model::{Host, HostAuth, Id};
use linascp_core::session::SessionId;
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::sessions::{SessionEventDto, channel_event_sender, make_sink, register};
use crate::output::{OutputPump, RawChunk};
use crate::state::AppState;
use crate::util::lock;

/// Looks a host up in the merged list (stored + ssh-config).
pub(crate) fn find_host(state: &AppState, id: Id) -> Result<Host> {
    state
        .merged_hosts()?
        .into_iter()
        .find(|h| h.id == id)
        .ok_or_else(|| CoreError::Config { message: format!("host {id} not found") })
}

fn target_for(state: &AppState, host: Host) -> Result<ConnectTarget> {
    let profile = match &host.auth {
        HostAuth::Profile { profile_id } => Some(state.store.get_profile(*profile_id)?.ok_or_else(|| {
            CoreError::Config { message: format!("login profile {profile_id} of host {} not found", host.name) }
        })?),
        HostAuth::Custom { .. } => None,
    };
    Ok(ConnectTarget { host, profile })
}

/// Resolves the login profile and the (single-hop) jump host of `host`.
pub(crate) fn build_request(state: &AppState, host: Host, cols: u16, rows: u16) -> Result<ConnectRequest> {
    let jump = match host.jump_host_id {
        Some(jump_id) => {
            let jump_host = find_host(state, jump_id).map_err(|_| CoreError::Config {
                message: format!("jump host {jump_id} of host {} not found", host.name),
            })?;
            Some(target_for(state, jump_host)?)
        }
        None => None,
    };
    Ok(ConnectRequest { target: target_for(state, host)?, jump, cols, rows })
}

/// Registers a cancellable connection attempt under the UI-generated `connect_id`.
pub(crate) fn register_connect(state: &AppState, connect_id: Uuid) -> CancellationToken {
    let token = CancellationToken::new();
    lock(&state.connects).insert(connect_id, token.clone());
    token
}

pub(crate) fn unregister_connect(state: &AppState, connect_id: Uuid) {
    lock(&state.connects).remove(&connect_id);
}

/// Unknown ids are ignored (the attempt may just have finished).
pub(crate) fn cancel_connect(state: &AppState, connect_id: Uuid) {
    if let Some(token) = lock(&state.connects).get(&connect_id) {
        token.cancel();
    }
}

fn connect_deps(state: &AppState) -> ConnectDeps {
    ConnectDeps {
        known_hosts: KnownHosts::new(state.paths.known_hosts()),
        secrets: state.secrets.clone(),
        agent_sock: std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from),
    }
}

fn channel_emit(channel: Channel<ConnectEvent>) -> EmitFn {
    Arc::new(move |event: ConnectEvent| {
        if let Err(error) = channel.send(event) {
            tracing::debug!(%error, "connect event dropped: channel closed");
        }
    })
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
) -> Result<SessionId> {
    let request = build_request(&state, find_host(&state, host_id)?, cols, rows)?;
    let emit = channel_emit(events);
    let pump = OutputPump::for_channel(output);
    let sink = make_sink(pump.clone(), channel_event_sender(session_events));
    let cancel = register_connect(&state, connect_id);
    let result = run_pipeline(request, connect_deps(&state), state.broker.clone(), emit.clone(), sink, cancel).await;
    unregister_connect(&state, connect_id);
    match result {
        Ok(session) => {
            let session_id = Uuid::new_v4();
            register(&state, session_id, Arc::new(session), pump);
            // core emits `Failed` on errors; the success event is ours (exactly one terminal event).
            emit(ConnectEvent::Connected { session_id });
            Ok(session_id)
        }
        Err(error) => {
            pump.close();
            Err(error)
        }
    }
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

/// "Verbindung testen": pipeline for an (unsaved) host up to Auth, then disconnect (core `test_connection`).
#[tauri::command]
#[specta::specta]
pub async fn connect_test(
    state: State<'_, AppState>,
    connect_id: Uuid,
    host: Host,
    events: Channel<ConnectEvent>,
) -> Result<()> {
    let request = build_request(&state, host, 80, 24)?;
    let cancel = register_connect(&state, connect_id);
    let result =
        test_connection(request, connect_deps(&state), state.broker.clone(), channel_emit(events), cancel).await;
    unregister_connect(&state, connect_id);
    result
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
        let token = register_connect(&state, connect_id);

        cancel_connect(&state, Uuid::new_v4());
        assert!(!token.is_cancelled(), "other ids must not cancel");
        cancel_connect(&state, connect_id);
        assert!(token.is_cancelled());

        unregister_connect(&state, connect_id);
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
