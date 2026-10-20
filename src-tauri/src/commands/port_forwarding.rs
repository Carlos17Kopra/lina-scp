//! Port forwards without a terminal (TP6b): one shell-less SSH connection per host carries its rules.

use std::sync::{Arc, Weak};

use linascp_core::connect::{
    ChangeFn, ConnConnector, ConnStatus, ConnectEvent, EmitFn, PromptBroker, Reconnecting, StatusFn, TunnelSession,
    connect_tunnel as run_pipeline,
};
use linascp_core::error::{CoreError, Result};
use linascp_core::model::{Id, PortForward};
use linascp_core::session::SessionId;
use linascp_core::tunnel::{TunnelEvent, Tunnels};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use uuid::Uuid;

use super::connect::{
    build_request, build_request_in, channel_emit, connect_deps, connect_deps_in, fail, find_host, find_host_in,
    register_connect,
};
use super::settings::auto_reconnect_enabled;
use crate::state::{AppState, ConnectEnv};
use crate::util::lock;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelsConnected {
    pub session_id: SessionId,
    /// The rules as they were stored at connect time; they apply until the next connect.
    pub forwards: Vec<PortForward>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PortForwardEvent {
    Reconnecting {
        attempt: u32,
        max_attempts: u32,
        retry_in_ms: u32,
        reason: CoreError,
    },
    Reconnected,
    /// Reconnecting gave up (or is off); every tunnel is off.
    Lost {
        error: CoreError,
    },
    Tunnel {
        event: TunnelEvent,
    },
}

pub(crate) type PortForwardEmit = Arc<dyn Fn(PortForwardEvent) + Send + Sync>;

/// One card of the port-forwarding screen: the self-healing connection and the tunnels on it.
pub struct PortForwardEntry {
    pub conn: Reconnecting<TunnelSession>,
    pub tunnels: Arc<Tunnels>,
}

/// Giving up stops every tunnel; the rest is passed on.
fn status_fn(tunnels: Weak<Tunnels>, emit: PortForwardEmit) -> StatusFn {
    Arc::new(move |status| {
        let event = match status {
            ConnStatus::Reconnecting { attempt, max_attempts, retry_in, reason } => PortForwardEvent::Reconnecting {
                attempt,
                max_attempts,
                retry_in_ms: u32::try_from(retry_in.as_millis()).unwrap_or(u32::MAX),
                reason,
            },
            ConnStatus::Reconnected { .. } => PortForwardEvent::Reconnected,
            ConnStatus::Lost { error } => {
                if let Some(tunnels) = tunnels.upgrade() {
                    tunnels.end();
                }
                PortForwardEvent::Lost { error }
            }
        };
        emit(event)
    })
}

/// A lost connection detaches the tunnels (listeners stay, nothing is routed); the next connection attaches them.
fn change_fn(tunnels: Weak<Tunnels>) -> ChangeFn<TunnelSession> {
    Arc::new(move |session| {
        if let (None, Some(tunnels)) = (session, tunnels.upgrade()) {
            tunnels.detach();
        }
    })
}

/// Reconnect attempts: pipeline without prompts (a refused prompt is `InputRequired`); the tunnels move over.
fn tunnel_connector(env: ConnectEnv, host_id: Id, tunnels: Weak<Tunnels>) -> ConnConnector<TunnelSession> {
    Arc::new(move |on_lost, cancel| {
        let env = env.clone();
        let tunnels = tunnels.clone();
        Box::pin(async move {
            let host = find_host_in(&env, host_id)?;
            let request = build_request_in(&env, host, 80, 24)?;
            let broker = Arc::new(PromptBroker::non_interactive());
            let quiet: EmitFn = Arc::new(|_| {});
            let mut deps = connect_deps_in(&env);
            let routes = tunnels.upgrade().map(|t| t.routes());
            deps.forwarded = routes.as_ref().map(|(_, routes)| routes.clone());
            match run_pipeline(request, deps, broker.clone(), quiet, on_lost, cancel).await {
                Ok(session) => {
                    if let (Some(tunnels), Some((epoch, _))) = (tunnels.upgrade(), routes) {
                        let link = session.link();
                        tokio::spawn(async move { tunnels.attach(epoch, link).await });
                    }
                    Ok(Arc::new(session))
                }
                Err(_) if broker.refused_input() => Err(CoreError::InputRequired),
                Err(error) => Err(error),
            }
        })
    })
}

fn new_entry(
    state: &AppState,
    host_id: Id,
    forwards: Vec<PortForward>,
    emit: PortForwardEmit,
) -> Arc<PortForwardEntry> {
    let tunnel_emit = emit.clone();
    let tunnels =
        Arc::new(Tunnels::new(forwards, Arc::new(move |event| tunnel_emit(PortForwardEvent::Tunnel { event }))));
    let store = state.store.clone();
    let conn = Reconnecting::new(
        tunnel_connector(state.connect_env(), host_id, Arc::downgrade(&tunnels)),
        Arc::new(move || auto_reconnect_enabled(&store)),
        status_fn(Arc::downgrade(&tunnels), emit),
        change_fn(Arc::downgrade(&tunnels)),
    );
    Arc::new(PortForwardEntry { conn, tunnels })
}

pub(crate) async fn connect_tunnels_inner(
    state: &AppState,
    connect_id: Uuid,
    host_id: Id,
    emit: EmitFn,
    pf_emit: PortForwardEmit,
) -> Result<TunnelsConnected> {
    // Register first: a cancel issued while the host is looked up must not be lost.
    let guard = register_connect(state, connect_id);
    let host = match find_host(state, host_id) {
        Ok(host) => host,
        Err(error) => return fail(&emit, error),
    };
    let forwards = host.forwards.clone();
    let request = match build_request(state, host, 80, 24) {
        Ok(request) => request,
        Err(error) => return fail(&emit, error),
    };
    if guard.is_cancelled() {
        return fail(&emit, CoreError::Cancelled);
    }
    let session_id = Uuid::new_v4();
    let entry = new_entry(state, host_id, forwards.clone(), pf_emit);
    let cancel = guard.token();
    let (generation, on_lost) = entry.conn.begin();
    let mut deps = connect_deps(state);
    let (epoch, routes) = entry.tunnels.routes();
    deps.forwarded = Some(routes);
    let result = run_pipeline(request, deps, state.broker.clone(), emit.clone(), on_lost, cancel.clone()).await;
    drop(guard);
    match result {
        Ok(session) if cancel.is_cancelled() => {
            session.close().await;
            fail(&emit, CoreError::Cancelled)
        }
        Ok(session) => {
            let link = session.link();
            entry.conn.attach(generation, Arc::new(session));
            lock(&state.tunnels).insert(session_id, entry.tunnels.clone());
            lock(&state.port_forwards).insert(session_id, entry.clone());
            emit(ConnectEvent::Connected { session_id });
            // Autostart after `Connected`: its events belong to a card that exists by then.
            let tunnels = entry.tunnels.clone();
            tokio::spawn(async move { tunnels.attach(epoch, link).await });
            Ok(TunnelsConnected { session_id, forwards })
        }
        Err(error) => Err(error),
    }
}

/// Stops every tunnel and disconnects; unknown ids are fine (already closed).
pub(crate) async fn tunnels_close_inner(state: &AppState, id: SessionId) {
    lock(&state.tunnels).remove(&id);
    let removed = lock(&state.port_forwards).remove(&id);
    if let Some(entry) = removed {
        entry.tunnels.end();
        entry.conn.close().await;
    }
}

#[tauri::command]
#[specta::specta]
pub async fn connect_tunnels(
    state: State<'_, AppState>,
    connect_id: Uuid,
    host_id: Id,
    events: Channel<ConnectEvent>,
    tunnel_events: Channel<PortForwardEvent>,
) -> Result<TunnelsConnected> {
    let pf_emit: PortForwardEmit = Arc::new(move |event| {
        if let Err(e) = tunnel_events.send(event) {
            tracing::debug!(error = %e, "port-forward event dropped: channel closed");
        }
    });
    connect_tunnels_inner(&state, connect_id, host_id, channel_emit(events), pf_emit).await
}

#[tauri::command]
#[specta::specta]
pub async fn tunnels_close(state: State<'_, AppState>, session_id: SessionId) -> Result<()> {
    tunnels_close_inner(&state, session_id).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::sessions::tunnel_start_inner;
    use linascp_core::model::ForwardKind;
    use linascp_core::tunnel::{TunnelLink, TunnelState, TunnelStream};
    use std::sync::Mutex;
    use std::time::Duration;

    fn setup() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        (dir, state)
    }

    struct NoLink;

    #[async_trait::async_trait]
    impl TunnelLink for NoLink {
        async fn open_direct(&self, _: &str, _: u16, _: std::net::SocketAddr) -> Result<TunnelStream> {
            Err(CoreError::internal("unused"))
        }
        async fn request_remote(&self, _: &str, port: u16) -> Result<u16> {
            Ok(port)
        }
        async fn cancel_remote(&self, _: &str, _: u16) -> Result<()> {
            Ok(())
        }
    }

    fn remote_rule() -> PortForward {
        PortForward {
            id: Uuid::new_v4(),
            kind: ForwardKind::Remote,
            bind: "9".into(),
            target: "a:1".into(),
            autostart: true,
        }
    }

    #[tokio::test]
    async fn connect_tunnels_with_an_unknown_host_fails_once() {
        let (_d, state) = setup();
        let events: Arc<Mutex<Vec<ConnectEvent>>> = Arc::default();
        let emit: EmitFn = {
            let events = events.clone();
            Arc::new(move |e| events.lock().unwrap().push(e))
        };
        let result = connect_tunnels_inner(&state, Uuid::new_v4(), Uuid::new_v4(), emit, Arc::new(|_| {})).await;
        assert!(result.is_err());
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ConnectEvent::Failed { .. }));
    }

    #[tokio::test]
    async fn tunnels_close_with_an_unknown_id_is_ok() {
        let (_d, state) = setup();
        tunnels_close_inner(&state, Uuid::new_v4()).await;
    }

    #[tokio::test]
    async fn a_loss_detaches_the_tunnels_and_giving_up_ends_them() {
        let rule = remote_rule();
        let events = Arc::new(Mutex::new(Vec::<PortForwardEvent>::new()));
        let seen = events.clone();
        let emit: PortForwardEmit = Arc::new(move |e| seen.lock().unwrap().push(e));
        let tunnel_emit = emit.clone();
        let tunnels = Arc::new(Tunnels::new(
            vec![rule.clone()],
            Arc::new(move |event| tunnel_emit(PortForwardEvent::Tunnel { event })),
        ));
        let (epoch, routes) = tunnels.routes();
        tunnels.attach(epoch, Arc::new(NoLink)).await;
        assert!(routes("localhost", 9).is_some());

        change_fn(Arc::downgrade(&tunnels))(None);
        assert!(routes("localhost", 9).is_none(), "nothing is routed while reconnecting");
        let status = status_fn(Arc::downgrade(&tunnels), emit);
        status(ConnStatus::Reconnecting {
            attempt: 1,
            max_attempts: 3,
            retry_in: Duration::from_secs(2),
            reason: CoreError::internal("x"),
        });
        let last_state = |events: &[PortForwardEvent]| {
            events.iter().rev().find_map(|e| match e {
                PortForwardEvent::Tunnel { event } => Some(event.state.clone()),
                _ => None,
            })
        };
        assert_ne!(last_state(&events.lock().unwrap()), Some(TunnelState::Off), "reconnecting keeps the rule on");
        status(ConnStatus::Lost { error: CoreError::internal("x") });
        let events = events.lock().unwrap();
        assert_eq!(last_state(&events), Some(TunnelState::Off));
        assert!(matches!(events.last(), Some(PortForwardEvent::Lost { .. })));
    }

    #[tokio::test]
    async fn a_closed_entry_is_gone_from_the_session_tunnels() {
        let (_d, state) = setup();
        let rule = remote_rule();
        let id = Uuid::new_v4();
        let entry = new_entry(&state, Uuid::new_v4(), vec![rule.clone()], Arc::new(|_| {}));
        lock(&state.tunnels).insert(id, entry.tunnels.clone());
        lock(&state.port_forwards).insert(id, entry);
        tunnels_close_inner(&state, id).await;
        assert!(lock(&state.port_forwards).is_empty());
        assert!(lock(&state.tunnels).is_empty(), "tunnel_start must no longer find it");
        assert!(tunnel_start_inner(&state, id, rule.id).await.is_err());
    }
}
