# LinaSCP TP2d (Auto-Reconnect) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A lost SSH connection reconnects by itself in the same session (same session id, same xterm, history
kept). A regular shell exit never reconnects.

**Architecture:**
- **Detection:** `SshSession` reports a new internal `SessionEvent::Lost` when its channel ends without `Close` or a
  write fails on a dead link.
- **Reconnect:** a core wrapper, `ReconnectingSession`, holds the current inner session. It intercepts `Lost` and runs
  a backoff loop through a `Connector`. In the app the `Connector` is the normal pipeline with a **non-interactive**
  prompt broker.
- **Stable identity:** the outer sink, output pump and session id never change.
- **UI:** shows the new events `reconnecting`, `reconnected` and `lost` as pane status, a reconnect bar and a
  "Verbindung verloren" card.

**Tech Stack:** Rust (tokio, russh, async-trait), Tauri 2 + specta, React 19 + zustand + xterm.js, Vitest, bun.

**Spec:** `docs/superpowers/specs/2026-10-15-linascp-tp2d-auto-reconnect-design.md`

## Global Constraints

- **Branch and commits:**
  - Work on `feat/tp2d-auto-reconnect`, which exists and holds the spec.
  - Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Toolchain:**
  - Always run `export PATH=$HOME/.cargo/bin:$PATH` first.
  - src-tauri tests need `SHELL=/bin/sh`.
  - Regenerate bindings with `SHELL=/bin/sh cargo test -p linascp exports_bindings`.
- **Language:** code and identifiers in English; UI strings in German, only in `ui/src/i18n/de.ts`.
- **Backoff:**
  - `BACKOFF_SECS = [1, 2, 4, 8, 16, 30, 30, 30]`, so `MAX_ATTEMPTS = 8`.
  - The wait comes before each attempt.
  - `reconnect_now` after `lost` starts a round without the first wait.
- **Fatal reasons:** stop immediately, and `reconnect_now` does nothing for them. They are `inputRequired`, `config`
  (host, profile or jump host gone: the spec's "notFound") and `auth`.
- **Secrets:**
  - Reconnect attempts never prompt. Every prompt makes the attempt fail with `CoreError::InputRequired`, and no
    prompt event leaves the core (fail closed).
  - Secrets come only from the keyring.
- **Events:**
  - The outer sink sees, per round, exactly one terminal event: `Exit`, `Error` or `Lost`.
  - `close()` always yields exactly one `Exit { code: None }` unless the session already ended. This keeps the
    `Session::close` contract that cleanup relies on.
  - `Lost` does **not** trigger cleanup: the session stays registered so "Erneut versuchen" works.
- **Setting:**
  - Settings key `terminal_auto_reconnect`, default `true`.
  - It is read when a connection is lost, not when it connects.
- **Test output stays free of warnings**, Rust and UI.

## Review Focus

1. **Closing a session yourself must never trigger a reconnect.**
   - `SshSession::close` sets `closing` first, so the reader reports `Exit`, not `Lost`.
   - `ReconnectingSession::close` bumps the generation and cancels the round.
   - Tests in Tasks 1 and 2.
2. **Events from an old connection must not reach the new one.**
   - Each connection has a generation; stale output, `Lost` and `Exit` are dropped.
   - Test in Task 2 (`stale_events_of_an_old_connection_are_ignored`).
3. **Stop and close racing a successful attempt.**
   - A late success is closed and discarded, and exactly one result is reported.
   - Test in Task 2 (`stop_during_an_attempt_discards_a_late_success`).
4. **Shell output of a fresh connection arrives before `reconnected`.**
   - The core holds the output back until it emits `Reconnected`.
   - The UI injects the separator into the same output stream synchronously, so it is printed before the new shell's
     banner.
   - Tests in Task 2 (`output_of_an_attempt_is_held_until_reconnected`) and Task 4.
5. **Acks must stay correct with injected bytes.**
   - The separator is written to xterm but was never sent by the backend, so it must not be acked. Over-acking would
     break the 1 MiB flow control.
   - Test in Task 4.

---

### Task 1: Session event contract (core events, loss detection, non-interactive broker, DTO, UI types)

**Files:**
- Modify:
  - `crates/core/src/error.rs`
  - `crates/core/src/session/mod.rs`
  - `crates/core/src/connect/ssh_session.rs`
  - `crates/core/src/connect/prompt.rs`
  - `crates/core/src/connect/mod.rs` (`SessionInfo` derives `Deserialize`)
  - `crates/core/tests/support/mod.rs`
  - `src-tauri/src/commands/sessions.rs`
  - `ui/src/ipc/bindings.ts` (generated)
  - `ui/src/ipc/errors.ts`
  - `ui/src/stores/sessions.ts`
  - `ui/src/stores/sessionEvents.ts`
- Test: in-file Rust tests, `ui/src/stores/sessions.test.ts`

**Interfaces:**
- Produces:
  - `CoreError::InputRequired`, serialized as `{"type":"inputRequired"}`.
  - New `SessionEvent` variants, and the enum now derives `PartialEq`:
    - `Lost(CoreError)`
    - `Reconnecting { attempt: u32, max_attempts: u32, retry_in: Duration, reason: CoreError }`
    - `Reconnected { info: SessionInfo }`
  - `Session` trait default methods `fn reconnect_now(&self) {}` and `fn stop_reconnect(&self) {}`.
  - Non-interactive prompt broker: `PromptBroker::non_interactive()` and `PromptBroker::refused_input(&self) -> bool`.
  - New `SessionEventDto` variants:
    - `Reconnecting { attempt, max_attempts, retry_in_ms: u32, reason }`
    - `Reconnected { info }`
    - `Lost { reason }`
    - Wire names are camelCase (`retryInMs`).
  - UI:
    - `TabStatus` gains `"reconnecting" | "lost"`.
    - `Tab.reconnect?: ReconnectState`, where `ReconnectState = { attempt: number; maxAttempts: number; retryAt: number; reason: CoreError }`.

- [ ] **Step 1: Write the failing Rust tests**

In `crates/core/src/connect/ssh_session.rs` add at the end:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;

    #[test]
    fn channel_end_without_close_is_a_lost_connection() {
        assert_eq!(end_event(true, false, Some(0)), SessionEvent::Exit { code: Some(0) });
        // We closed it ourselves: never a reconnect, even if the server's Close never arrived.
        assert_eq!(end_event(false, true, None), SessionEvent::Exit { code: None });
        assert!(matches!(end_event(false, false, None), SessionEvent::Lost(CoreError::Network { .. })));
    }
}
```

In `crates/core/src/connect/prompt.rs` `mod tests` add:

```rust
    #[tokio::test]
    async fn non_interactive_broker_refuses_every_prompt_without_emitting() {
        let broker = PromptBroker::non_interactive();
        let emitted = Arc::new(Mutex::new(0));
        let counter = emitted.clone();
        let emit: EmitFn = Arc::new(move |_| *counter.lock().unwrap() += 1);
        assert!(!broker.refused_input());
        let answer = broker.ask_cancellable(&emit, passphrase_prompt(), &CancellationToken::new()).await;
        assert!(matches!(answer, PromptAnswer::Cancel));
        assert_eq!(*emitted.lock().unwrap(), 0, "no prompt may reach the UI");
        assert!(broker.refused_input());
    }
```

In `src-tauri/src/commands/sessions.rs` `mod tests` add:

```rust
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
```

In `crates/core/src/error.rs`, extend its serialization test module, or add a test if it has none:

```rust
    #[test]
    fn input_required_serializes_as_a_unit_type() {
        assert_eq!(serde_json::to_value(CoreError::InputRequired).unwrap(), serde_json::json!({ "type": "inputRequired" }));
    }
```

- [ ] **Step 2: Run and verify failure**

Run: `export PATH=$HOME/.cargo/bin:$PATH && cargo test -p linascp-core 2>&1 | grep -E "^error|cannot find" | head`
Expected: compile errors (`end_event`, `non_interactive`, `InputRequired` and `SessionEvent::Lost` do not exist).

- [ ] **Step 3: Implement the core**

`crates/core/src/error.rs`, after `Cancelled`:

```rust
    /// A reconnect attempt would have needed user input (password, passphrase, 2FA, host key).
    #[error("input required")]
    InputRequired,
```

`crates/core/src/session/mod.rs`:

```rust
use std::time::Duration;

use crate::connect::SessionInfo;

/// Events a session pushes to its consumer.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    /// Raw terminal output bytes.
    Output(Vec<u8>),
    /// The session ended; `code` is the exit status if known.
    Exit { code: Option<u32> },
    /// A fatal error inside the session task.
    Error(CoreError),
    /// The connection broke (no regular end). From `SshSession` it is internal: `ReconnectingSession`
    /// turns it into a reconnect round; it reaches the app sink only when the session gives up.
    Lost(CoreError),
    /// A reconnect attempt follows after `retry_in`.
    Reconnecting { attempt: u32, max_attempts: u32, retry_in: Duration, reason: CoreError },
    /// A new connection replaced the lost one; output continues on the same session.
    Reconnected { info: SessionInfo },
}
```

Add to the `Session` trait:

```rust
    /// Reconnecting sessions: skip the current wait, or start a new round after giving up.
    fn reconnect_now(&self) {}
    /// Reconnecting sessions: stop the running round (reports `Lost(Cancelled)`).
    fn stop_reconnect(&self) {}
```

`crates/core/src/connect/mod.rs`: change the `SessionInfo` derive to
`#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]`. Import `Deserialize` if
needed. `SessionEventDto` derives `Deserialize`, so its `info` field needs it. If a field type of `SessionInfo` (for
example `AuthMethod`) lacks `Deserialize`, derive it there as well.

`crates/core/src/connect/ssh_session.rs`:
- Add `use std::sync::Arc;`, `use std::sync::atomic::{AtomicBool, Ordering};`,
  `use crate::error::{CoreError, NetworkKind};` and `use super::classify::network_error;`.
- Add the fields:
  ```rust
      sink: OutputSink,
      /// Set by `close`: the end of the channel is then an `Exit`, never a lost connection.
      closing: Arc<AtomicBool>,
      /// A terminal event (`Exit` / `Lost`) was sent; guards against a second one.
      ended: Arc<AtomicBool>,
  ```
- `start` creates both flags and passes clones to `forward_output(read_half, sink.clone(), closing, ended)`.
- `forward_output`:

```rust
/// Forward channel messages to the sink until the channel closes.
/// Exactly one terminal event is emitted at the end: `Exit` for a regular end, `Lost` otherwise.
async fn forward_output(mut read_half: ChannelReadHalf, sink: OutputSink, closing: Arc<AtomicBool>, ended: Arc<AtomicBool>) {
    let mut code = None;
    let mut saw_close = false;
    while let Some(msg) = read_half.wait().await {
        match msg {
            ChannelMsg::Data { data } => sink(SessionEvent::Output(data.to_vec())),
            ChannelMsg::ExtendedData { data, .. } => sink(SessionEvent::Output(data.to_vec())),
            ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
            ChannelMsg::Close => {
                saw_close = true;
                break;
            }
            _ => {}
        }
    }
    if !ended.swap(true, Ordering::SeqCst) {
        sink(end_event(saw_close, closing.load(Ordering::SeqCst), code));
    }
}

/// How a channel ended: the server's `Close` or our own `close` is a regular end; anything else
/// (connection reset, keepalive timeout, server gone) is a lost connection.
fn end_event(saw_close: bool, closing: bool, code: Option<u32>) -> SessionEvent {
    if saw_close || closing {
        SessionEvent::Exit { code }
    } else {
        SessionEvent::Lost(network_error(NetworkKind::Other, "connection lost"))
    }
}
```

- `write` reports a dead link once:

```rust
    async fn write(&self, data: &[u8]) -> Result<()> {
        let result = self.writer.data(data).await.map_err(map_russh_error);
        if let Err(error @ CoreError::Network { .. }) = &result
            && !self.closing.load(Ordering::SeqCst)
            && !self.ended.swap(true, Ordering::SeqCst)
        {
            // Hosts without keepalive notice a dead link only when typing.
            (self.sink)(SessionEvent::Lost(error.clone()));
        }
        result
    }
```

- `close` begins with `self.closing.store(true, Ordering::SeqCst);`.

`crates/core/src/connect/prompt.rs`:
- Add the fields `interactive: bool` and `refused: AtomicBool` (import `AtomicBool`).
- `with_timeout` sets `interactive: true, refused: AtomicBool::new(false)`.
- Add:

```rust
    /// Broker for automatic reconnects: every prompt is refused at once and never published.
    /// `refused_input` then tells the caller to report `CoreError::InputRequired`.
    pub fn non_interactive() -> Self {
        Self { interactive: false, ..Self::with_timeout(Arc::new(|_| {}), PROMPT_TIMEOUT) }
    }

    /// Whether a prompt was refused because the broker is non-interactive.
    pub fn refused_input(&self) -> bool {
        self.refused.load(Ordering::SeqCst)
    }
```

  At the top of `ask_cancellable`:

```rust
        if !self.interactive {
            self.refused.store(true, Ordering::SeqCst);
            return PromptAnswer::Cancel;
        }
```

`crates/core/tests/support/mod.rs`: add the arm `_ => {}` to the `Output` sink match.

- [ ] **Step 4: Implement the DTO**

`src-tauri/src/commands/sessions.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionEventDto {
    Exit { code: Option<u32> },
    Error { error: CoreError },
    Reconnecting { attempt: u32, max_attempts: u32, retry_in_ms: u32, reason: CoreError },
    Reconnected { info: linascp_core::connect::SessionInfo },
    Lost { reason: CoreError },
}
```

Add these arms to `make_sink`. They have no `pump.close()` and no `cleanup()`:

```rust
        SessionEvent::Lost(reason) => send_event(SessionEventDto::Lost { reason }),
        SessionEvent::Reconnecting { attempt, max_attempts, retry_in, reason } => {
            let retry_in_ms = u32::try_from(retry_in.as_millis()).unwrap_or(u32::MAX);
            send_event(SessionEventDto::Reconnecting { attempt, max_attempts, retry_in_ms, reason })
        }
        SessionEvent::Reconnected { info } => send_event(SessionEventDto::Reconnected { info }),
```

Update the `make_sink` doc: "`Lost` and reconnect events keep the session registered (it can reconnect again)".

- [ ] **Step 5: Run the Rust tests and regenerate bindings**

Run: `cargo test -p linascp-core 2>&1 | grep -E "test result|FAILED|panicked" ; SHELL=/bin/sh cargo test -p linascp 2>&1 | grep -E "test result|FAILED"`
Expected: all pass. `bindings.ts` is regenerated and now contains `inputRequired`, `reconnecting` and `retryInMs`.

- [ ] **Step 6: Write the failing UI tests**

Append to `ui/src/stores/sessions.test.ts`. Import `sessionEventPatch` from `./sessionEvents`:

```ts
describe("session event patches for reconnects", () => {
  const reason = { type: "network", kind: "other", message: "connection lost" } as const;

  it("maps reconnecting, reconnected and lost onto the pane", () => {
    vi.spyOn(Date, "now").mockReturnValue(10_000);
    expect(sessionEventPatch({ kind: "reconnecting", attempt: 2, maxAttempts: 8, retryInMs: 2000, reason })).toEqual({
      status: "reconnecting",
      reconnect: { attempt: 2, maxAttempts: 8, retryAt: 12_000, reason },
    });
    const info = { hostKeyAlgorithm: "ssh-ed25519", hostKeySha256: "x", kex: "k", cipher: "c", mac: "m", authMethods: [] };
    expect(sessionEventPatch({ kind: "reconnected", info })).toEqual({
      status: "connected",
      info,
      connectedAt: 10_000,
      reconnect: undefined,
      error: undefined,
    });
    expect(sessionEventPatch({ kind: "lost", reason })).toEqual({ status: "lost", error: reason, reconnect: undefined });
    vi.restoreAllMocks();
  });
});
```

- [ ] **Step 7: Run and verify failure**

Run: `bun run typecheck 2>&1 | tail -5; bun run test ui/src/stores/sessions.test.ts 2>&1 | grep -E "×|Tests"`
Expected: typecheck fails in `sessionEvents.ts`, because new event kinds have no `error`. The new test fails.

- [ ] **Step 8: Implement the UI types**

`ui/src/ipc/errors.ts`: add `"inputRequired"` to `CORE_ERROR_TYPES`.

`ui/src/stores/sessions.ts`:

```ts
/** `reconnecting`: the connection broke and is being restored; `lost`: reconnecting gave up. */
export type TabStatus = "picking" | "connecting" | "connected" | "reconnecting" | "lost" | "closed" | "error";

/** Progress of an automatic reconnect (status `reconnecting`). */
export interface ReconnectState {
  attempt: number;
  maxAttempts: number;
  /** Client time (ms) of the next attempt. */
  retryAt: number;
  reason: CoreError;
}
```

Add to `Tab`: `reconnect?: ReconnectState;` with the doc `/** While status is "reconnecting". */`.

`ui/src/stores/sessionEvents.ts`:

```ts
/** The tab fields a session event sets (ended, failed, reconnecting, restored, lost). */
export function sessionEventPatch(event: SessionEventDto): Partial<Omit<Tab, "id">> {
  switch (event.kind) {
    case "exit":
      return { status: "closed" };
    case "error":
      return { status: "error", error: event.error };
    case "reconnecting":
      return {
        status: "reconnecting",
        reconnect: { attempt: event.attempt, maxAttempts: event.maxAttempts, retryAt: Date.now() + event.retryInMs, reason: event.reason },
      };
    case "reconnected":
      return { status: "connected", info: event.info, connectedAt: Date.now(), reconnect: undefined, error: undefined };
    case "lost":
      return { status: "lost", error: event.reason, reconnect: undefined };
  }
}
```

- [ ] **Step 9: Run everything**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && bun run typecheck && bun run test 2>&1 | tail -4`
Expected: all green. If `cargo fmt --check` complains, run `cargo fmt --all` and re-check.

- [ ] **Step 10: Commit**

```bash
git add crates src-tauri ui/src/ipc ui/src/stores
git commit -m "feat: session events for lost connections and reconnects

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: `ReconnectingSession` in core

**Files:**
- Create: `crates/core/src/connect/reconnect.rs`
- Modify:
  - `crates/core/src/connect/mod.rs` (`pub mod reconnect;` and re-exports)
  - `crates/core/Cargo.toml` (dev-dependency `tokio` with `test-util`)

**Interfaces:**
- Consumes: Task 1 `SessionEvent::{Lost, Reconnecting, Reconnected}`, `CoreError::InputRequired`, the `Session`
  default methods.
- Produces (re-exported from `linascp_core::connect`):
  - Constants: `BACKOFF_SECS: [u64; 8]` and `MAX_ATTEMPTS: u32`.
  - Types:
    - `ConnectFuture = Pin<Box<dyn Future<Output = Result<(Arc<dyn Session>, SessionInfo)>> + Send>>`.
    - `Connector = Arc<dyn Fn(u16, u16, OutputSink, CancellationToken) -> ConnectFuture + Send + Sync>`.
    - `Enabled = Arc<dyn Fn() -> bool + Send + Sync>`.
  - `is_fatal(&CoreError) -> bool`.
  - `ReconnectingSession`:
    - `::new(sink, cols, rows, connector, enabled)`
    - `.first_sink() -> OutputSink`
    - `.attach(Arc<dyn Session>)`
    - It implements `Session`, including `reconnect_now` and `stop_reconnect`.

- [ ] **Step 1: Add the test dependency**

`crates/core/Cargo.toml`, under `[dev-dependencies]`:

```toml
tokio = { version = "1", features = ["test-util", "macros", "rt-multi-thread"] }
```

- [ ] **Step 2: Write the failing tests** (bottom of the new `crates/core/src/connect/reconnect.rs`)

```rust
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
        SessionEvent::Reconnecting { attempt, max_attempts: MAX_ATTEMPTS, retry_in: Duration::from_secs(secs), reason: net() }
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
        assert_eq!(h.events(), vec![reconnecting(1, 1), reconnecting(2, 2), reconnecting(3, 4), SessionEvent::Reconnected { info }]);
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
}
```

- [ ] **Step 3: Run and verify failure**

Run: `cargo test -p linascp-core reconnect 2>&1 | grep -E "^error" | head -3`
Expected: compile errors, because `ReconnectingSession` and the other types are missing. Before running, add
`pub mod reconnect;` to `connect/mod.rs`.

- [ ] **Step 4: Implement `crates/core/src/connect/reconnect.rs`** (above the tests)

```rust
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

/// Reasons a retry cannot fix: input needed, host/profile/jump host gone, credentials rejected.
pub fn is_fatal(error: &CoreError) -> bool {
    matches!(error, CoreError::InputRequired | CoreError::Config { .. } | CoreError::Auth { .. })
}

enum Phase {
    /// A connection is being set up (first connect or an attempt); its output waits in `pending`.
    Attempting { pending: Vec<u8>, lost: Option<CoreError> },
    Connected(Arc<dyn Session>),
    /// Between attempts.
    Waiting,
    /// Gave up; `reconnect_now` may start a new round unless `reason` is fatal.
    Lost { reason: CoreError },
    /// The shell ended, failed, or `close` was called.
    Ended,
}

struct State {
    phase: Phase,
    /// Current connection; events of older ones are ignored.
    generation: u64,
    cols: u16,
    rows: u16,
    /// Cancels the running round (wait and attempt).
    round: CancellationToken,
}

struct Shared {
    state: Mutex<State>,
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
            phase: Phase::Attempting { pending: Vec::new(), lost: None },
            generation: 1,
            cols,
            rows,
            round: CancellationToken::new(),
        };
        Self { shared: Arc::new(Shared { state: Mutex::new(state), sink, connector, enabled, now: Notify::new() }) }
    }

    /// Sink for the first connection.
    pub fn first_sink(&self) -> OutputSink {
        inner_sink(Arc::downgrade(&self.shared), 1)
    }

    /// Hands over the first connection's session.
    pub fn attach(&self, session: Arc<dyn Session>) {
        let (cols, rows) = {
            let state = self.shared.lock();
            (state.cols, state.rows)
        };
        if let Attached::LostMeanwhile(reason) = self.shared.attached(1, session, (cols, rows), None) {
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

    /// Events of the inner connection `generation`. The sink is cheap and non-blocking, so it is
    /// called under the lock: that keeps the order of events strict.
    fn on_inner(self: &Arc<Self>, generation: u64, event: SessionEvent) {
        let mut state = self.lock();
        if state.generation != generation {
            return;
        }
        match event {
            SessionEvent::Output(bytes) => match &mut state.phase {
                Phase::Attempting { pending, .. } => {
                    if pending.len() + bytes.len() <= MAX_PENDING_OUTPUT {
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
                    state.phase = Phase::Ended;
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
        let Phase::Attempting { pending, lost } = std::mem::replace(&mut state.phase, Phase::Connected(session.clone()))
        else {
            unreachable!("checked above");
        };
        if let Some(reason) = lost {
            state.phase = Phase::Attempting { pending: Vec::new(), lost: None };
            drop(state);
            close_in_background(session);
            return Attached::LostMeanwhile(reason);
        }
        if let Some(info) = info {
            (self.sink)(SessionEvent::Reconnected { info });
        }
        if !pending.is_empty() {
            (self.sink)(SessionEvent::Output(pending));
        }
        let size = (state.cols, state.rows);
        drop(state);
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
            () = shared.now.notified() => {}
            () = round.cancelled() => return,
        }
        let (generation, cols, rows) = {
            let mut state = shared.lock();
            if round.is_cancelled() {
                return;
            }
            state.generation += 1;
            state.phase = Phase::Attempting { pending: Vec::new(), lost: None };
            (state.generation, state.cols, state.rows)
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
        let session = match &self.shared.lock().phase {
            Phase::Connected(session) => Some(session.clone()),
            _ => None,
        };
        match session {
            Some(session) => session.write(data).await,
            None => Ok(()),
        }
    }

    async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let session = {
            let mut state = self.shared.lock();
            state.cols = cols;
            state.rows = rows;
            match &state.phase {
                Phase::Connected(session) => Some(session.clone()),
                _ => None,
            }
        };
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
```

Add to `connect/mod.rs`:
`pub use reconnect::{BACKOFF_SECS, ConnectFuture, Connector, Enabled, MAX_ATTEMPTS, ReconnectingSession, is_fatal};`.

Notes for the implementer:
- `stop_reconnect` must not act during the **first** connect. That is unreachable anyway: the session is registered only
  after `attach`, and `Attempting` with `generation == 1` is the first connect. If clippy or a test shows otherwise,
  guard with `state.generation > 1`.
- `lose` is called with the lock held and calls `enabled()`, which reads SQLite. That is fine because it is rare.
- The `debug_assert!` in `run_round` documents the invariant. Remove it if it ever fires in a legitimate interleaving,
  and add a ledger ruling.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p linascp-core reconnect 2>&1 | grep -E "test result|FAILED|panicked"`
Expected: 15 passed. If a test fails because tasks have not run yet, increase the `yield_now` loop in `advance`,
not the production code. A real ordering bug must be fixed in the code.

- [ ] **Step 6: Run core fully, then commit**

Run: `cargo test -p linascp-core 2>&1 | grep -E "test result|FAILED" && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: green.

```bash
git add crates/core
git commit -m "feat(core): reconnecting session with backoff, stop and reconnect-now

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Wire it into Tauri: connector, commands, setting

**Files:**
- Modify:
  - `crates/core/src/store/mod.rs` (key constant)
  - `src-tauri/src/state.rs`
  - `src-tauri/src/commands/connect.rs`
  - `src-tauri/src/commands/sessions.rs`
  - `src-tauri/src/commands/settings.rs`
  - `src-tauri/src/dto.rs`
  - `src-tauri/src/lib.rs`
  - `ui/src/ipc/bindings.ts` (generated)
  - `ui/src/ipc/api.ts`
  - `ui/src/stores/settings.ts`
- Test: in-file Rust tests, `ui/src/ipc/api.test.ts`

**Interfaces:**
- Consumes: Task 2 `ReconnectingSession`, `Connector` and `Enabled`; Task 1 `PromptBroker::non_interactive` and
  `refused_input`.
- Produces:
  - Rust:
    - `ConnectEnv` (Clone), with `find_host`, `build_request` and `deps`.
    - Commands `session_reconnect_now(session_id)` and `session_reconnect_stop(session_id)`.
    - `Settings.terminal_auto_reconnect`.
  - UI:
    - `sessionReconnectNow(id)` and `sessionReconnectStop(id)` in `ipc/api.ts`.
    - `DEFAULT_SETTINGS.terminalAutoReconnect = true`.

- [ ] **Step 1: Write the failing Rust tests**

`src-tauri/src/commands/settings.rs` tests, or `dto.rs` next to the camelCase test. Update the expected JSON in
`settings_serialize_camel_case` to include `"terminalAutoReconnect": true`, and add to the settings tests:

```rust
    #[test]
    fn auto_reconnect_defaults_on_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_tests(dir.path());
        assert!(load_settings(&state.store).unwrap().terminal_auto_reconnect);
        let next = Settings { terminal_auto_reconnect: false, ..load_settings(&state.store).unwrap() };
        assert!(!save_settings(&state, next).unwrap().terminal_auto_reconnect);
        assert!(!auto_reconnect_enabled(&state.store));
    }
```

If `settings.rs` has no test module yet, create `#[cfg(test)] mod tests { use super::*; … }`.

`src-tauri/src/commands/sessions.rs` tests:

```rust
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
```

Use the module's existing test setup if it builds `AppState` differently (for example a `setup()` helper).

`src-tauri/src/commands/connect.rs` tests:

```rust
    #[tokio::test]
    async fn reconnect_connector_reports_a_deleted_host_as_config_error() {
        let (_dir, state) = setup();
        let connector = reconnect_connector(state.connect_env(), Uuid::new_v4());
        let sink: linascp_core::session::OutputSink = Arc::new(|_| {});
        let result = connector(80, 24, sink, CancellationToken::new()).await;
        assert!(matches!(result, Err(CoreError::Config { .. })));
    }
```

- [ ] **Step 2: Run and verify failure**

Run: `SHELL=/bin/sh cargo test -p linascp 2>&1 | grep -E "^error" | head -5`
Expected: compile errors (`terminal_auto_reconnect`, `auto_reconnect_enabled`, `reconnect_now`, `reconnect_connector`
and `connect_env` are missing).

- [ ] **Step 3: Implement**

`crates/core/src/store/mod.rs`, next to `TERMINAL_SIDEBAR_OPEN`:

```rust
/// Setting key: `"0"` turns automatic SSH reconnects off (default on).
pub const TERMINAL_AUTO_RECONNECT: &str = "terminal_auto_reconnect";
```

`src-tauri/src/dto.rs`:
- Add `pub terminal_auto_reconnect: bool,` after `terminal_sidebar_open`.
- In the default, add `terminal_auto_reconnect: true,`.

`src-tauri/src/commands/settings.rs`:
- Import `TERMINAL_AUTO_RECONNECT`.
- In `load_settings` add
  `terminal_auto_reconnect: parse_flag(store.get_setting(TERMINAL_AUTO_RECONNECT)?).unwrap_or(d.terminal_auto_reconnect),`.
- In `save_settings` add
  `store.set_setting(TERMINAL_AUTO_RECONNECT, flag_str(next.terminal_auto_reconnect))?;`.
- Add:

```rust
/// Read when a connection is lost; a store error keeps the default (on).
pub(crate) fn auto_reconnect_enabled(store: &Store) -> bool {
    load_settings(store).map(|settings| settings.terminal_auto_reconnect).unwrap_or(true)
}
```

`src-tauri/src/state.rs`: a cheap, owned handle for code that outlives a command (the reconnect connector):

```rust
/// What a connect needs from the app state, owned and cheap to clone (reconnects run long after
/// the command that started the session returned).
#[derive(Clone)]
pub struct ConnectEnv {
    pub paths: AppPaths,
    pub store: Arc<Store>,
    pub secrets: Arc<dyn SecretStore>,
}

impl ConnectEnv {
    /// Stored hosts merged with the read-only `~/.ssh/config` import (stored wins on equal name).
    /// A broken ssh config never blocks the app: it is logged and ignored.
    pub fn merged_hosts(&self) -> Result<Vec<Host>> {
        // body moved here unchanged from `AppState::merged_hosts`, using `self.store` / `self.paths`
    }
}
```

Move the body of `AppState::merged_hosts` into `ConnectEnv::merged_hosts` (the same code with
`self.store`/`self.paths`). Then make `AppState::merged_hosts` delegate:

```rust
    pub fn connect_env(&self) -> ConnectEnv {
        ConnectEnv { paths: self.paths.clone(), store: self.store.clone(), secrets: self.secrets.clone() }
    }

    pub fn merged_hosts(&self) -> Result<Vec<Host>> {
        self.connect_env().merged_hosts()
    }
```

`src-tauri/src/commands/connect.rs`:
- Turn `find_host`, `target_for`, `build_request` and `connect_deps` into functions over `&ConnectEnv`, keeping thin
  `&AppState` wrappers so existing callers and tests stay unchanged:

```rust
pub(crate) fn find_host(state: &AppState, id: Id) -> Result<Host> {
    find_host_in(&state.connect_env(), id)
}

fn find_host_in(env: &ConnectEnv, id: Id) -> Result<Host> {
    env.merged_hosts()?
        .into_iter()
        .find(|h| h.id == id)
        .ok_or_else(|| CoreError::Config { message: format!("host {id} not found") })
}
```

  Do the same for `target_for_in(env, host)`, `build_request_in(env, host, cols, rows)` (it calls `find_host_in` and
  `target_for_in`) and `connect_deps_in(env)`. The existing `build_request` and `connect_deps` wrappers call them with
  `&state.connect_env()`.

- The connector:

```rust
/// Reconnect attempts: request rebuilt from the current store/keyring, pipeline without prompts.
/// A refused prompt becomes `CoreError::InputRequired` (fail closed, nothing reaches the UI).
fn reconnect_connector(env: ConnectEnv, host_id: Id) -> Connector {
    Arc::new(move |cols, rows, sink, cancel| {
        let env = env.clone();
        Box::pin(async move {
            let host = find_host_in(&env, host_id)?;
            let request = build_request_in(&env, host, cols, rows)?;
            let broker = Arc::new(PromptBroker::non_interactive());
            let quiet: EmitFn = Arc::new(|_| {});
            match run_pipeline(request, connect_deps_in(&env), broker.clone(), quiet, sink, cancel).await {
                Ok(session) => {
                    let info = session.info().clone();
                    Ok((Arc::new(session) as Arc<dyn Session>, info))
                }
                Err(_) if broker.refused_input() => Err(CoreError::InputRequired),
                Err(error) => Err(error),
            }
        })
    })
}
```

- In `connect_ssh_registered`, wrap the session. Replace the block from `let session_id = …` through the match:

```rust
    let session_id = Uuid::new_v4();
    let pump = OutputPump::for_channel(output);
    let sink = make_sink(pump.clone(), channel_event_sender(session_events), cleanup_fn(state, session_id));
    let store = state.store.clone();
    let enabled: Enabled = Arc::new(move || auto_reconnect_enabled(&store));
    let reconnecting =
        Arc::new(ReconnectingSession::new(sink, cols, rows, reconnect_connector(state.connect_env(), host_id), enabled));
    let cancel = guard.token();
    let result = run_pipeline(
        request,
        connect_deps(state),
        state.broker.clone(),
        emit.clone(),
        reconnecting.first_sink(),
        cancel.clone(),
    )
    .await;
    drop(guard);

    match result {
        Ok(session) if cancel.is_cancelled() => {
            pump.close();
            let _ = session.close().await;
            fail(&emit, CoreError::Cancelled)
        }
        Ok(session) => {
            let info = session.info().clone();
            reconnecting.attach(Arc::new(session));
            register(state, session_id, reconnecting, pump.clone());
            heal_if_already_exited(state, session_id, &pump);
            emit(ConnectEvent::Connected { session_id });
            Ok(ConnectedSession { session_id, info })
        }
        Err(error) => {
            pump.close();
            Err(error)
        }
    }
```

  Keep the existing comments for the cancelled branch. Imports: `ConnectEnv` from `crate::state`;
  `Connector`, `Enabled`, `PromptBroker` and `ReconnectingSession` from `linascp_core::connect`;
  `auto_reconnect_enabled` from `super::settings`.

`src-tauri/src/commands/sessions.rs`:

```rust
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
```

`src-tauri/src/lib.rs`: register `sessions::session_reconnect_now` and `sessions::session_reconnect_stop` after
`sessions::session_close`.

- [ ] **Step 4: Run the Rust tests, regenerate bindings**

Run: `SHELL=/bin/sh cargo test -p linascp 2>&1 | grep -E "test result|FAILED"; cargo clippy --workspace --all-targets -- -D warnings`
Expected: green; `bindings.ts` now has `sessionReconnectNow`, `sessionReconnectStop` and `terminalAutoReconnect`.

- [ ] **Step 5: UI wrappers and defaults, failing test first**

Append to the `describe` of commands in `ui/src/ipc/api.test.ts`. Use the file's `invoke` mock style:

```ts
  it("wrap the reconnect commands", async () => {
    invoke.mockResolvedValue(null);
    await sessionReconnectNow("s1");
    expect(invoke).toHaveBeenLastCalledWith("session_reconnect_now", { sessionId: "s1" });
    await sessionReconnectStop("s1");
    expect(invoke).toHaveBeenLastCalledWith("session_reconnect_stop", { sessionId: "s1" });
  });
```

Run `bun run test ui/src/ipc/api.test.ts` and expect it to fail. Then add to `ui/src/ipc/api.ts` after `sessionClose`:

```ts
/** Skip the reconnect wait, or start a new round after giving up (status `lost`). */
export function sessionReconnectNow(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sessionReconnectNow(sessionId));
}
/** Stop reconnecting (the pane shows "Verbindung verloren"). */
export function sessionReconnectStop(sessionId: SessionId): Promise<void> {
  return unwrapVoid(commands.sessionReconnectStop(sessionId));
}
```

In `ui/src/stores/settings.ts`, add `terminalAutoReconnect: true,` to `DEFAULT_SETTINGS`. Run the UI tests again and
fix fixtures that spell out a full `Settings` object; `bun run typecheck` names them.

- [ ] **Step 6: Run everything and commit**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && SHELL=/bin/sh cargo test --workspace 2>&1 | grep -E "test result|FAILED" && bun run typecheck && bun run test 2>&1 | tail -4`
Expected: green.

```bash
git add crates src-tauri ui/src
git commit -m "feat: reconnect SSH sessions in place via a non-interactive connector

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Reconnect bar, separator line, broadcast and sidebar

**Files:**
- Create:
  - `ui/src/screens/terminal/ReconnectBar.tsx`
  - `ui/src/screens/terminal/ReconnectBar.module.css`
  - `ui/src/screens/terminal/ReconnectBar.test.tsx`
- Modify:
  - `ui/src/i18n/de.ts`
  - `ui/src/ipc/api.ts` (`OutputStream.inject`)
  - `ui/src/stores/sessionEvents.ts`
  - `ui/src/screens/terminal/PaneLayout.tsx`
  - `ui/src/screens/terminal/sidebar/SessionPanel.tsx`
- Test: `ui/src/ipc/api.test.ts`, `ui/src/stores/sessions.test.ts`, `ui/src/screens/terminal/PaneLayout.test.tsx`,
  `ui/src/screens/terminal/sidebar/SessionPanel.test.tsx` (existing)

**Interfaces:**
- Consumes: Task 1 `ReconnectState` and `sessionEventPatch`; Task 3 `sessionReconnectNow` and `sessionReconnectStop`.
- Produces:
  - `ReconnectBar({ tab })`.
  - `OutputStream.inject(bytes: Uint8Array)`: local bytes for xterm that are excluded from acks.
  - `t.terminal.reconnecting.*` strings.

Ruling to record in the ledger: the spec says `TerminalView` writes the separator. It is injected into the pane's
output stream from the session event handler instead. React effects run after a render, so new shell output could
otherwise land above the separator (Review Focus 4).

- [ ] **Step 1: Strings**

`ui/src/i18n/de.ts`, inside `terminal`, add after `reconnect`:

```ts
    reconnecting: {
      interrupted: (attempt: number, max: number, seconds: number) =>
        `Verbindung unterbrochen · Versuch ${attempt} von ${max} in ${seconds} s`,
      connecting: (attempt: number, max: number) => `Verbindung unterbrochen · Versuch ${attempt} von ${max} · Verbinde …`,
      now: "Jetzt versuchen",
      stop: "Abbrechen",
      restored: "— Verbindung wiederhergestellt —",
      lost: "Verbindung verloren",
      lostText: (title: string) => `Die Verbindung zu »${title}« konnte nicht wiederhergestellt werden. Der Verlauf bleibt sichtbar.`,
      retry: "Erneut versuchen",
      inputRequired: "Für die Verbindung ist eine Eingabe nötig (Passwort, Host-Key …).",
      cancelled: "Neuverbindung abgebrochen.",
      suffixReconnecting: "Verbindung unterbrochen",
      suffixLost: "Verbindung verloren",
      sidebar: (attempt: number, max: number) => `Neuverbindung · Versuch ${attempt} von ${max}`,
    },
```

- [ ] **Step 2: Write the failing tests**

`ui/src/ipc/api.test.ts`, inside `describe("createOutputStream")`:

```ts
  it("delivers injected bytes in order and never acks them (Review Focus 5)", () => {
    const stream = createOutputStream();
    stream.bind("s1");
    const received: number[][] = [];
    stream.attach((bytes) => received.push(Array.from(bytes)));
    stream.inject(new Uint8Array([7, 7]));
    stream.channel.onmessage(new Uint8Array([1]).buffer);
    expect(received).toEqual([[7, 7], [1]]);
    stream.ack(2); // xterm consumed the injected bytes
    expect(invoke).not.toHaveBeenCalled();
    stream.ack(1);
    expect(invoke).toHaveBeenLastCalledWith("session_ack", { sessionId: "s1", bytes: 1 });
  });
```

`ui/src/stores/sessions.test.ts`, appended to the reconnect `describe`:

```ts
  it("writes the separator into the pane's stream before the new shell output (Review Focus 4)", () => {
    const injected: string[] = [];
    registerStream("p", { ...stream, inject: (bytes: Uint8Array) => injected.push(new TextDecoder().decode(bytes)) } as never);
    useSessionsStore.getState().addTab({ id: "p", title: "web", kind: "ssh", sessionId: "s", status: "reconnecting" });
    const info = { hostKeyAlgorithm: "a", hostKeySha256: "b", kex: "c", cipher: "d", mac: "e", authMethods: [] };
    sessionEventHandler("p")({ kind: "reconnected", info });
    expect(injected).toEqual(["\r\n\u001b[2m— Verbindung wiederhergestellt —\u001b[0m\r\n"]);
    expect(useSessionsStore.getState().tabs[0]?.status).toBe("connected");
  });
```

Import `registerStream` from `./streams` and `sessionEventHandler` from `./sessionEvents` if they are not imported
yet.

`ui/src/screens/terminal/ReconnectBar.test.tsx`:

```tsx
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../ipc/api";
import type { Tab } from "../../stores/sessions";
import { ReconnectBar } from "./ReconnectBar";

vi.mock("../../ipc/api");

const reason = { type: "network", kind: "other", message: "connection lost" } as const;
const tab = (retryAt: number): Tab => ({
  id: "p",
  title: "web",
  kind: "ssh",
  sessionId: "s-1",
  status: "reconnecting",
  reconnect: { attempt: 3, maxAttempts: 8, retryAt, reason },
});

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(100_000);
  vi.mocked(api.sessionReconnectNow).mockResolvedValue(undefined);
  vi.mocked(api.sessionReconnectStop).mockResolvedValue(undefined);
});
afterEach(() => vi.useRealTimers());

describe("ReconnectBar", () => {
  it("counts down to the next attempt and then shows that it connects", () => {
    render(<ReconnectBar tab={tab(104_000)} />);
    expect(screen.getByRole("status")).toHaveTextContent("Verbindung unterbrochen · Versuch 3 von 8 in 4 s");
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.getByRole("status")).toHaveTextContent("in 3 s");
    act(() => vi.advanceTimersByTime(3000));
    expect(screen.getByRole("status")).toHaveTextContent("Versuch 3 von 8 · Verbinde …");
  });

  it("retries now and stops", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<ReconnectBar tab={tab(104_000)} />);
    await user.click(screen.getByRole("button", { name: "Jetzt versuchen" }));
    expect(api.sessionReconnectNow).toHaveBeenCalledWith("s-1");
    await user.click(screen.getByRole("button", { name: "Abbrechen" }));
    expect(api.sessionReconnectStop).toHaveBeenCalledWith("s-1");
  });
});
```

`ui/src/screens/terminal/PaneLayout.test.tsx`, appended inside the `describe`:

```tsx
  it("shows the reconnect bar over a reconnecting pane and keeps its terminal", () => {
    const reason = { type: "network", kind: "other", message: "x" } as const;
    act(() =>
      useSessionsStore.getState().updateTab("a", {
        status: "reconnecting",
        reconnect: { attempt: 1, maxAttempts: 8, retryAt: Date.now() + 1000, reason },
      }),
    );
    render(<PaneLayout group={group()} visible />);
    expect(screen.getByRole("status")).toHaveTextContent("Versuch 1 von 8");
    expect(screen.getByTestId("term-a")).toBeInTheDocument();
  });
```

In `ui/src/screens/terminal/sidebar/SessionPanel.test.tsx`, add a test that follows the file's existing render
helper:

```tsx
  it("shows the reconnect progress instead of the connection time", () => {
    // Use the file's existing helper to render SessionPanel for an SSH tab, with:
    //   status: "reconnecting", reconnect: { attempt: 2, maxAttempts: 8, retryAt: Date.now() + 2000,
    //   reason: { type: "network", kind: "other", message: "x" } }
    expect(screen.getByText("Neuverbindung · Versuch 2 von 8")).toBeInTheDocument();
  });
```

Write this test fully with the file's helper. The comment above only marks where its tab literal goes.

- [ ] **Step 3: Run and verify failure**

Run: `bun run test ui/src/ipc ui/src/stores ui/src/screens/terminal 2>&1 | grep -E "×|FAIL|Tests"`
Expected: the new tests fail. `inject` and `ReconnectBar` are missing, and there is no separator or sidebar text.

- [ ] **Step 4: Implement**

`ui/src/ipc/api.ts`:
- `OutputStream` gets:

```ts
  /** Local bytes for the terminal (e.g. the reconnect separator), in order with the output, never acked. */
  inject(bytes: Uint8Array): void;
```

- `createOutputStream`:
  - Add `let localBytes = 0;`.
  - Make `deliver` the shared path of the channel and of `inject`:

```ts
  const deliver = (bytes: Uint8Array) => {
    if (consumer) consumer(bytes);
    else pending.push(bytes);
  };
  const channel = createOutputChannel(deliver);
```

  - Return object additions:

```ts
    inject(bytes) {
      localBytes += bytes.length;
      deliver(bytes);
    },
```

  - `ack` then subtracts the local bytes first:

```ts
    ack(bytes) {
      // xterm acks everything it wrote; injected bytes never came from the backend.
      const local = Math.min(localBytes, bytes);
      localBytes -= local;
      unsentAck += bytes - local;
      flushAck();
    },
```

`ui/src/stores/sessionEvents.ts`: import `t` from `../i18n/de` and `getStream` from `./streams`. In the handler,
before applying the patch to an existing (non-picking) tab:

```ts
    if (event.kind === "reconnected") {
      // Synchronously, in stream order: before the new shell's first output.
      getStream(tabId)?.inject(new TextEncoder().encode(`\r\n\u001b[2m${t.terminal.reconnecting.restored}\u001b[0m\r\n`));
    }
```

  Only inject when the tab exists and is not picking, in the same branch that calls `updateTab`.

`ui/src/screens/terminal/ReconnectBar.tsx`:

```tsx
import { useEffect, useState } from "react";
import { Button } from "../../components";
import { t } from "../../i18n/de";
import { sessionReconnectNow, sessionReconnectStop } from "../../ipc/api";
import { fireAndForget } from "../../stores/connectProgress";
import type { Tab } from "../../stores/sessions";
import styles from "./ReconnectBar.module.css";

/** Strip over a pane whose connection is being restored: countdown, "Jetzt versuchen", "Abbrechen". */
export function ReconnectBar({ tab }: { tab: Tab }) {
  const texts = t.terminal.reconnecting;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const state = tab.reconnect;
  const sessionId = tab.sessionId;
  if (!state || !sessionId) return null;
  const seconds = Math.max(0, Math.ceil((state.retryAt - now) / 1000));
  return (
    <div className={styles.bar}>
      <span role="status" className={styles.text}>
        {seconds > 0 ? texts.interrupted(state.attempt, state.maxAttempts, seconds) : texts.connecting(state.attempt, state.maxAttempts)}
      </span>
      <Button size="sm" variant="primary" onClick={() => fireAndForget(() => sessionReconnectNow(sessionId))}>
        {texts.now}
      </Button>
      <Button size="sm" variant="ghost" onClick={() => fireAndForget(() => sessionReconnectStop(sessionId))}>
        {texts.stop}
      </Button>
    </div>
  );
}
```

If `role="status"` re-announces every second in screen readers, that is acceptable for now (spec: "Ansage
beschränkt sich auf den Versuch"). An improvement is `aria-live="off"` on the seconds part. Ledger it as a minor
if you do not do it.

`ui/src/screens/terminal/ReconnectBar.module.css`:

```css
.bar {
  position: absolute;
  top: 6px;
  left: 50%;
  z-index: 2;
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: calc(100% - 48px);
  padding: 6px 8px 6px 12px;
  border: 1px solid var(--amber-bd);
  border-radius: 8px;
  background: var(--amber-soft);
  color: var(--text);
  font-size: 13px;
  transform: translateX(-50%);
}

.text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
```

`ui/src/screens/terminal/PaneLayout.tsx`, in `Pane`, after the `TerminalView` line:

```tsx
      {pane.status === "reconnecting" ? <ReconnectBar tab={pane} /> : null}
```

  Extend `ended` with `|| pane.status === "lost"`. `SessionEnded` handles `lost` in Task 5. Import `ReconnectBar`.

`ui/src/screens/terminal/sidebar/SessionPanel.tsx`: extend `ended` with `lost`, and compute `since` as:

```tsx
  const since =
    tab.status === "reconnecting" && tab.reconnect
      ? t.terminal.reconnecting.sidebar(tab.reconnect.attempt, tab.reconnect.maxAttempts)
      : ended
        ? t.terminal.info.disconnected
        : tab.connectedAt
          ? formatSince(tab.connectedAt, now)
          : t.terminal.info.none;
```

- [ ] **Step 5: Run all UI tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -4`
Expected: green, no warnings. Broadcast already skips non-`connected` panes, and `broadcastPeers` is unchanged. Add a
one-line assertion to the existing broadcast store test (`useSessionsStore.getState().updateTab("t-1", { status:
"reconnecting" }); expect(broadcastPeers("a")).toEqual([]);`) to pin it.

- [ ] **Step 6: Commit**

```bash
git add ui/src
git commit -m "feat(ui): reconnect bar, separator line and reconnect progress in the sidebar

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: "Verbindung verloren" card, tab status, settings checkbox

**Files:**
- Modify:
  - `ui/src/screens/terminal/SessionEnded.tsx`
  - `ui/src/screens/terminal/TabBar.tsx` and `TabBar.module.css`
  - `ui/src/screens/settings/SettingsDialog.tsx`
  - `ui/src/i18n/de.ts`
- Test: `ui/src/screens/terminal/TerminalScreen.test.tsx`, `ui/src/screens/settings/SettingsDialog.test.tsx`

**Interfaces:**
- Consumes: Task 1 `is_fatal` semantics (mirrored in the UI as `FATAL_LOST`), Task 3 `sessionReconnectNow` and
  `terminalAutoReconnect`, Task 4 strings.

- [ ] **Step 1: Strings**

`ui/src/i18n/de.ts`, in `settings`, next to `compat`:

```ts
    autoReconnect: "Abgebrochene SSH-Verbindungen automatisch wiederherstellen",
```

- [ ] **Step 2: Write the failing tests**

`TerminalScreen.test.tsx`. `web` is the existing SSH tab fixture with `id: "a"`, `hostId: "h-web"` and
`sessionId: "s-a"`:

```tsx
  it("offers retry, reconnect and close when the connection was lost", async () => {
    vi.mocked(api.sessionReconnectNow).mockResolvedValue(undefined);
    seed([{ ...web, status: "lost", error: { type: "network", kind: "timeout", message: "timed out" } }], "a");
    render(<TerminalScreen />);
    expect(screen.getByRole("heading", { name: "Verbindung verloren" })).toBeInTheDocument();
    expect(screen.getByText("timed out")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /web-prod-01 – Verbindung verloren/ })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Erneut versuchen" }));
    expect(api.sessionReconnectNow).toHaveBeenCalledWith("s-a");
    expect(screen.getByRole("button", { name: "Neu verbinden" })).toBeInTheDocument();
  });

  it("only offers an interactive reconnect when input is required", () => {
    seed([{ ...web, status: "lost", error: { type: "inputRequired" } }], "a");
    render(<TerminalScreen />);
    expect(screen.getByText("Für die Verbindung ist eine Eingabe nötig (Passwort, Host-Key …).")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Erneut versuchen" })).toBeNull();
    expect(screen.getByRole("button", { name: "Neu verbinden" })).toBeInTheDocument();
  });

  it("only offers close when the host is gone", () => {
    seed([{ ...web, status: "lost", error: { type: "config", message: "host h-web not found" } }], "a");
    render(<TerminalScreen />);
    expect(screen.queryByRole("button", { name: "Erneut versuchen" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Neu verbinden" })).toBeNull();
    expect(screen.getByRole("button", { name: "Schließen" })).toBeInTheDocument();
  });

  it("marks a reconnecting tab for screen readers", () => {
    const reason = { type: "network", kind: "other", message: "x" } as const;
    seed([{ ...web, status: "reconnecting", reconnect: { attempt: 1, maxAttempts: 8, retryAt: Date.now() + 1000, reason } }], "a");
    render(<TerminalScreen />);
    expect(screen.getByRole("tab", { name: /web-prod-01 – Verbindung unterbrochen/ })).toBeInTheDocument();
  });
```

If the existing close button label is not "Schließen" (`t.common.close`), use that label.

`SettingsDialog.test.tsx`, appended to "saves renderer, font size and compatibility rendering":

```tsx
    await userEvent.click(screen.getByRole("checkbox", { name: "Abgebrochene SSH-Verbindungen automatisch wiederherstellen" }));
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ terminalAutoReconnect: false }));
```

- [ ] **Step 3: Run and verify failure**

Run: `bun run test ui/src/screens 2>&1 | grep -E "×|Tests"`
Expected: the five new expectations fail.

- [ ] **Step 4: Implement**

`SessionEnded.tsx`:

```tsx
import { RefreshCw, RotateCw } from "lucide-react";
import { useId } from "react";
import { Button, cx } from "../../components";
import { t } from "../../i18n/de";
import { type CoreError, sessionReconnectNow } from "../../ipc/api";
import { errorMessage } from "../../ipc/errors";
import { useConnectStore } from "../../stores/connect";
import { fireAndForget } from "../../stores/connectProgress";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import styles from "./SessionEnded.module.css";

/** Reasons a silent retry cannot fix (mirrors core `is_fatal`). */
const FATAL_LOST: ReadonlySet<CoreError["type"]> = new Set(["inputRequired", "config", "auth"]);

function lostReason(error: CoreError | undefined): string | null {
  if (!error) return null;
  if (error.type === "inputRequired") return t.terminal.reconnecting.inputRequired;
  if (error.type === "cancelled") return t.terminal.reconnecting.cancelled;
  return errorMessage(error);
}

/**
 * Overlay on a pane whose session ended or whose connection was lost: close it, retry silently
 * (history kept), or reconnect SSH sessions interactively in the same pane.
 */
export function SessionEnded({ tab }: { tab: Tab }) {
  const closeSession = useSessionsStore((state) => state.closeSession);
  const replacePane = useSessionsStore((state) => state.replacePane);
  const titleId = useId();
  const lost = tab.status === "lost";
  const hostGone = lost && tab.error?.type === "config";
  const hostId = tab.kind === "ssh" && !hostGone ? tab.hostId : undefined;
  const canRetry = lost && !!tab.sessionId && !(tab.error && FATAL_LOST.has(tab.error.type));

  const reconnect = () => {
    if (!hostId) return;
    const paneId = replacePane(tab.id);
    void useConnectStore.getState().start(hostId, "ssh", { paneId });
  };
  const retry = () => {
    const sessionId = tab.sessionId;
    if (sessionId) fireAndForget(() => sessionReconnectNow(sessionId));
  };

  const title = lost ? t.terminal.reconnecting.lost : tab.status === "error" ? t.terminal.endedError : t.terminal.ended;
  const detail = lost ? lostReason(tab.error) : tab.status === "error" && tab.error ? errorMessage(tab.error) : null;

  return (
    <div className={styles.overlay}>
      <section className={styles.card} aria-labelledby={titleId}>
        <h2 id={titleId} className={styles.title}>
          {title}
        </h2>
        <p className={styles.text}>{lost ? t.terminal.reconnecting.lostText(tab.title) : t.terminal.endedText(tab.title)}</p>
        {detail ? <p className={cx(styles.error, "mono")}>{detail}</p> : null}
        <div className={styles.actions}>
          <Button onClick={() => void closeSession(tab.id)}>{t.common.close}</Button>
          {canRetry ? (
            <Button icon={RotateCw} onClick={retry}>
              {t.terminal.reconnecting.retry}
            </Button>
          ) : null}
          {hostId ? (
            <Button variant="primary" icon={RefreshCw} onClick={reconnect}>
              {t.terminal.reconnect}
            </Button>
          ) : null}
        </div>
      </section>
    </div>
  );
}
```

Ruling to record in the ledger: the spec's `notFound` reason is the existing `CoreError::Config` ("host … not
found"). `config` and `auth` are fatal, like `inputRequired`. A wrong saved password cannot heal by retrying, but
"Neu verbinden" can prompt for it.

`TabBar.tsx`, inside the tab button, replace the `ended` suffix line with:

```tsx
                {suffix ? <span className="visually-hidden">{` – ${suffix}`}</span> : null}
```

and compute before the `return` of the map callback:

```tsx
          const suffix =
            pane?.status === "reconnecting"
              ? t.terminal.reconnecting.suffixReconnecting
              : pane?.status === "lost"
                ? t.terminal.reconnecting.suffixLost
                : ended
                  ? t.terminal.endedSuffix
                  : null;
```

`TabBar.module.css`, after the `error` status rule:

```css
.tab[data-status="reconnecting"] {
  --tab-tone: var(--amber);
}

.tab[data-status="reconnecting"] .dot {
  animation: pulse 1.2s ease-in-out infinite;
}

.tab[data-status="lost"] {
  --tab-tone: var(--dim);
}

@keyframes pulse {
  50% {
    opacity: 0.3;
  }
}

@media (prefers-reduced-motion: reduce) {
  .tab[data-status="reconnecting"] .dot {
    animation: none;
  }
}
```

`SettingsDialog.tsx`, after the compat `Checkbox`:

```tsx
          <Checkbox
            label={t.settings.autoReconnect}
            checked={settings.terminalAutoReconnect}
            onChange={(terminalAutoReconnect) => persist({ terminalAutoReconnect })}
          />
```

- [ ] **Step 5: Run all UI tests**

Run: `bun run typecheck && bun run test 2>&1 | tail -4`
Expected: green, no warnings.

- [ ] **Step 6: Commit**

```bash
git add ui/src
git commit -m "feat(ui): connection-lost card, reconnect tab status and auto-reconnect setting

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Docker integration test, acceptance checklist, full verification

**Files:**
- Modify: `crates/core/tests/ssh_integration.rs`, `CLAUDE.md`
- Create: `docs/acceptance-tp2d.md`

**Interfaces:**
- Consumes: Task 1 loss detection and Task 2 `ReconnectingSession`.

- [ ] **Step 1: Integration test**

Append to `crates/core/tests/ssh_integration.rs`. It needs docker; it is `#[ignore]` like the others:

```rust
#[tokio::test]
#[ignore = "requires docker"]
async fn lost_connection_reconnects_on_the_same_session() {
    use linascp_core::connect::{Connector, Enabled, ReconnectingSession};
    use linascp_core::session::{OutputSink, SessionEvent};
    use std::sync::Mutex;

    let c = Container::start(None);
    let env = Arc::new(Env::new());
    let h = host(c.port, "pwuser", AuthMethod::Password, None);
    env.store_password(&h, "pwpass");
    let p = prompter(trust_all);

    let events: Arc<Mutex<Vec<SessionEvent>>> = Arc::default();
    let (output, collect) = Output::new();
    let sink: OutputSink = {
        let events = events.clone();
        Arc::new(move |event: SessionEvent| {
            if !matches!(event, SessionEvent::Output(_)) {
                events.lock().unwrap().push(event.clone());
            }
            collect(event);
        })
    };
    let connector: Connector = {
        let (env, h) = (env.clone(), h.clone());
        Arc::new(move |cols, rows, sink, cancel| {
            let (env, h) = (env.clone(), h.clone());
            Box::pin(async move {
                let mut req = request(h, None);
                req.cols = cols;
                req.rows = rows;
                let broker = Arc::new(linascp_core::connect::PromptBroker::non_interactive());
                let session = connect_ssh(req, env.deps(None), broker, Arc::new(|_| {}), sink, cancel).await?;
                let info = session.info().clone();
                Ok((Arc::new(session) as Arc<dyn Session>, info))
            })
        })
    };
    let enabled: Enabled = Arc::new(|| true);
    let session = ReconnectingSession::new(sink, 80, 24, connector, enabled);
    // First connect: interactive broker (trusts the unknown host key), wrapper's sink.
    let initial = connect_ssh(request(h, None), env.deps(None), p.broker.clone(), p.emit.clone(), session.first_sink(), CancellationToken::new())
        .await
        .expect("connected");
    session.attach(Arc::new(initial));

    session.write(b"echo before-$((1+1))\n").await.unwrap();
    output.wait_for("before-2").await;
    // Kill the server side of this login without a regular channel close.
    let _ = std::process::Command::new("docker")
        .args(["exec", &c.id, "sh", "-c", "pkill -KILL -f 'sshd: pwuser' || true"])
        .output();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !events.lock().unwrap().iter().any(|e| matches!(e, SessionEvent::Reconnected { .. })) {
        assert!(std::time::Instant::now() < deadline, "no reconnect: {:?}", events.lock().unwrap());
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    session.write(b"echo after-$((2+2))\n").await.unwrap();
    output.wait_for("after-4").await;
    session.close().await.unwrap();
}
```

The reconnect attempts are non-interactive, but the host key is already in the temp `known_hosts` after the first
connect and the password is stored, so no prompt is needed. If the image has no
`pkill`, use `kill -9 $(pgrep -f 'sshd: pwuser')` or `ps | grep`; ledger what you used.

Run: `cargo test -p linascp-core --test ssh_integration lost_connection -- --ignored --test-threads=1 2>&1 | tail -5`
Expected: passes when docker is available. Without docker it may be skipped; say so in the ledger and in the final
report.

- [ ] **Step 2: `docs/acceptance-tp2d.md`**

```markdown
# TP2d acceptance: Auto-reconnect

No DB migration in TP2d (new settings key `terminal_auto_reconnect`).

## Reconnect
- [ ] SSH tab running `top`; disable Wi-Fi / pull the cable until the keepalive trips (≤ 90 s with defaults): the pane shows "Verbindung unterbrochen · Versuch 1 von 8 in 1 s", the terminal stays visible and scrollable, typing does nothing.
- [ ] Network back: "— Verbindung wiederhergestellt —" appears under the old output, the new prompt follows; tab dot back to green; sidebar "Sitzung" shows the new connection time.
- [ ] Laptop standby for several minutes, wake up: the session reconnects (or shows the bar, then reconnects).
- [ ] On the server, kill the session's sshd process (`sudo pkill -f "sshd: <user>"`; `systemctl restart ssh` alone keeps existing sessions): reconnect within seconds.
- [ ] Host with `keepalive_s = 0`: kill the network, type a key: the bar appears.
- [ ] "Jetzt versuchen" skips the countdown; "Abbrechen" shows "Verbindung verloren" with "Erneut versuchen" (history kept), "Neu verbinden" (fresh pane) and "Schließen".
- [ ] Server stays down: after 8 attempts (~2 min) "Verbindung verloren" with the network error.

## No reconnect
- [ ] `exit` in the shell: "Sitzung beendet", no reconnect.
- [ ] Settings → Terminal → "Abgebrochene SSH-Verbindungen automatisch wiederherstellen" off: a dropped connection shows "Verbindung verloren" at once.
- [ ] Password host without a saved password: drop the connection: "Verbindung verloren" with "Für die Verbindung ist eine Eingabe nötig …"; only "Neu verbinden" / "Schließen"; "Neu verbinden" opens the normal prompt.
- [ ] Change the server's host key, drop the connection: input required, never accepted silently.
- [ ] Delete the host while it is reconnecting: "Verbindung verloren", only "Schließen".

## Split panes and broadcast
- [ ] Two panes, broadcast on, one pane reconnecting: typing reaches only the connected pane.
- [ ] Close a pane while it reconnects: no error, no later event.
```

- [ ] **Step 3: `CLAUDE.md`**

Add after the TP2c spec bullet:

```markdown
- `docs/superpowers/specs/2026-10-15-linascp-tp2d-auto-reconnect-design.md`: TP2d spec (auto-reconnect).
```

Change the acceptance bullet to:

```markdown
- `docs/acceptance-tp1.md`: manual acceptance checklist (`docs/acceptance-tp2a.md` … `docs/acceptance-tp2d.md` for
  TP2a–TP2d).
```

Add to the Invariants "Connections" bullet:

```markdown
  Automatic reconnects never prompt: any prompt fails the attempt with `inputRequired` (fail closed).
```

- [ ] **Step 4: Full verification**

```bash
export PATH=$HOME/.cargo/bin:$PATH
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings \
  && SHELL=/bin/sh cargo test --workspace 2>&1 | grep -E "test result|FAILED" \
  && bun run typecheck && bun run test 2>&1 | tail -4
```

Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add crates/core/tests docs/acceptance-tp2d.md CLAUDE.md
git commit -m "test: reconnect against a real sshd; docs: TP2d acceptance checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
