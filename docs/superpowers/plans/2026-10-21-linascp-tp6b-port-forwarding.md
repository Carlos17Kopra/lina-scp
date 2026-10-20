# TP6b Port-Forwarding Screen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A rail screen „Port-Forwarding“ that runs a host's port-forward rules over its own shell-less SSH
connection.

**Architecture:** Core gets a `TunnelSession` (handle + loss watcher, like `SftpSession` without the SFTP channel)
and `connect_tunnel`. Tauri keeps one `PortForwardEntry { conn: Reconnecting<TunnelSession>, tunnels }` per
session and also registers the `Tunnels` in `state.tunnels`, so `tunnel_start`/`tunnel_stop` work unchanged. The
UI adds protocol `"tunnel"` to the connect store, a small `portForwarding` store and the screen, reusing the rule
rows from `TunnelPanel`.

**Tech Stack:** Rust (russh, tokio), Tauri 2 + specta, React 19 + zustand, vitest.

**Spec:** `docs/superpowers/specs/2026-10-20-linascp-tp6b-port-forwarding-design.md`

## Global Constraints

- Code, comments, identifiers, commits in English; UI strings German, only in `ui/src/i18n/de.ts`.
- `ui/src/ipc/bindings.ts` is generated: `cargo test -p linascp exports_bindings`, never edit by hand.
- TDD, test output free of warnings (act(), jsdom).
- Exactly one terminal `ConnectEvent` per connect id, including errors before the pipeline and a cancel.
- Automatic reconnects never prompt (`PromptBroker::non_interactive()`, refused prompt → `CoreError::InputRequired`).
- TP5a tunnel invariants unchanged; tunnels never outlive their connection.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Checks before the last commit: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `bun run typecheck && bun run test`.

## Review Focus

- Autostart `tunnel` events that arrive before `connectTunnels` resolves must not be lost (UI buffers them; Task 3
  test).
- „Verbinden“ again after „Verbindung verloren“ must close the old backend session (no leaked listener; Task 3 test).
- Cancel during connect: the session that still comes back is closed and no card turns „Verbunden“ (Task 3 test).
- A host deleted or its rules removed while connected: the card disappears from the list; its session is closed
  (Task 4 test).
- `tunnels_close` must remove the entry from `state.tunnels` too, so `tunnel_start` on it errors (Task 2 test).

---

### Task 1: Core `TunnelSession` and `connect_tunnel`

**Files:**
- Create: `crates/core/src/connect/tunnel_session.rs`
- Modify: `crates/core/src/connect/mod.rs` (module, `plan_tunnel_steps`, `connect_tunnel`, `Pipeline::open_tunnel`)
- Modify: `crates/core/src/connect/ssh_session.rs` (make `SshLink` reusable: `pub(crate) fn ssh_link(handle: &Arc<client::Handle<ClientHandler>>) -> Arc<dyn TunnelLink>`)
- Test: unit tests in `mod.rs`; `crates/core/tests/ssh_integration.rs`

**Interfaces:**
- Produces:
  - `pub fn plan_tunnel_steps(req: &ConnectRequest) -> Vec<Step>` = `plan_steps` without `Channel`, `Shell`.
  - `pub struct TunnelSession` with `pub fn link(&self) -> Arc<dyn TunnelLink>`, `pub fn info(&self) -> &SessionInfo`,
    `impl Connection for TunnelSession` (`info()` clone, `close()` disconnects handle and jump, aborts the watcher).
  - `pub async fn connect_tunnel(req: ConnectRequest, deps: ConnectDeps, broker: Arc<PromptBroker>, emit: EmitFn, on_lost: LostFn, cancel: CancellationToken) -> Result<TunnelSession>`;
    emits `Steps { plan_tunnel_steps }` first and `Failed` on error, like `connect_sftp`.
  - Re-export `TunnelSession` and `connect_tunnel` from `connect`.

- [ ] **Step 1: Failing unit test** `plan_tunnel_steps_skip_channel_and_shell`: without jump
  `[Tcp, HostKey, Auth]`; with jump `[JumpHost, Tcp, HostKey, Auth]`.
- [ ] **Step 2:** `cargo test -p linascp-core plan_tunnel_steps` → FAIL (not defined).
- [ ] **Step 3: Implement.** `TunnelSession::start(handle, jump, info, on_lost)` copies the 1 s `is_closed` watcher
  and the fire-once `Lost` guard from `SftpSession::start` (extract a shared `pub(crate) fn watch_closed(handle, lost)`
  in `sftp_session.rs` instead of duplicating it). `open_tunnel` just wraps `Established`; no extra step.
- [ ] **Step 4:** `cargo test -p linascp-core` → PASS.
- [ ] **Step 5: Docker test** `tunnel_connects_and_forwards_a_local_port` in `ssh_integration.rs` (reuse the
  existing sftp/tunnel test helpers): `connect_tunnel` emits steps `[Tcp, HostKey, Auth]`, then a `Tunnels` with one
  `local` rule to the server's sshd port attached via `session.link()` accepts a TCP connect and reads the
  `SSH-2.0` banner; `close()` then makes a second `open_direct` on the link fail.
  Run: `cargo test -p linascp-core --test ssh_integration tunnel_connects -- --ignored --test-threads=1` → PASS
  (if Docker is unavailable, say so in the report; do not skip silently).
- [ ] **Step 6: Commit** `feat(core): shell-less tunnel connection`.

### Task 2: Tauri commands `connect_tunnels` / `tunnels_close`

**Files:**
- Create: `src-tauri/src/commands/port_forwarding.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (register), `src-tauri/src/state.rs`
  (`pub port_forwards: Arc<Mutex<HashMap<SessionId, Arc<PortForwardEntry>>>>`), `src-tauri/src/dto.rs`
- Generated: `ui/src/ipc/bindings.ts`

**Interfaces:**
- Consumes: Task 1 `connect_tunnel`, `TunnelSession`; `Reconnecting`, `ConnStatus`, `Tunnels`, `register_connect`,
  `fail`, `find_host`, `build_request`, `connect_deps`, `auto_reconnect_enabled` (all existing).
- Produces:
  - `#[serde(tag = "kind", rename_all = "camelCase")] pub enum PortForwardEvent { Reconnecting { attempt: u32, max_attempts: u32, retry_in_ms: u32, reason: CoreError }, Reconnected, Lost { error: CoreError }, Tunnel { event: TunnelEvent } }` (camelCase fields, specta).
  - `pub struct TunnelsConnected { session_id: SessionId, forwards: Vec<PortForward> }` (camelCase).
  - Commands `connect_tunnels(connect_id: Uuid, host_id: Id, events: Channel<ConnectEvent>, tunnel_events: Channel<PortForwardEvent>) -> Result<TunnelsConnected>` and `tunnels_close(session_id: SessionId) -> Result<()>`.
  - TS: `commands.connectTunnels`, `commands.tunnelsClose`, types `PortForwardEvent`, `TunnelsConnected`.

Behaviour: mirror `connect_sftp_inner` (register guard, `fail` before the pipeline, cancel after `Ok` closes and
fails with `Cancelled`). The `Tunnels` emit wraps events in `PortForwardEvent::Tunnel`. Status callback:
`Reconnecting` → `tunnels.detach()`, `Lost` → `tunnels.end()`, then emit. Connector (no prompts) and the first
connect both set `deps.forwarded` from `tunnels.routes()` and, on success, `tokio::spawn(tunnels.attach(epoch, link))`
after `Connected` / after the reconnect returns. On success insert into both `port_forwards` and `state.tunnels`.
`tunnels_close` removes from both, calls `tunnels.end()` and `conn.close().await`; unknown id → `Ok(())`.

- [ ] **Step 1: Failing tests** in `port_forwarding.rs` (setup like `sftp.rs` tests):
  - `connect_tunnels_with_an_unknown_host_fails_once`: collected `ConnectEvent`s are exactly one `Failed`.
  - `tunnels_close_with_an_unknown_id_is_ok`.
  - `status_detaches_on_reconnecting_and_ends_on_lost`: a `Tunnels` with one autostart local rule attached to a
    `NoLink`-style test link (copy the one in `connect.rs` tests) → the status fn with `Reconnecting{..}` keeps the
    rule's state not `Off`; with `Lost{..}` the last tunnel event is `Off`.
  - `closed_entry_is_gone_from_state_tunnels`: insert an entry by hand, `tunnels_close_inner`, then
    `tunnel_start_inner(state, id, rule)` is `Err`.
- [ ] **Step 2:** `cargo test -p linascp port_forwarding` → FAIL.
- [ ] **Step 3: Implement** (inner fns `connect_tunnels_inner`, `tunnels_close_inner`, `fn status_fn(tunnels: Weak<Tunnels>, emit) -> StatusFn`).
- [ ] **Step 4:** `cargo test -p linascp` → PASS; `cargo test -p linascp exports_bindings` regenerates bindings.
- [ ] **Step 5: Commit** `feat(tauri): port-forwarding connections without a terminal`.

### Task 3: UI connect protocol `"tunnel"` and `portForwarding` store

**Files:**
- Create: `ui/src/stores/portForwarding.ts`, `ui/src/stores/portForwarding.test.ts`
- Modify: `ui/src/ipc/api.ts` (`connectTunnels`, `tunnelsClose`, re-export types), `ui/src/stores/ui.ts`
  (`View` += `"portForwarding"`), `ui/src/stores/connect.ts`, `ui/src/stores/connect.test.ts`

**Interfaces:**
- Consumes: Task 2 commands/types; `setTunnelState(key, event)` from `stores/tunnels.ts`.
- Produces:
  - `type PfStatus = { kind: "connected" } | { kind: "reconnecting"; attempt: number; maxAttempts: number } | { kind: "lost"; error: CoreError }`.
  - `interface PfEntry { sessionId: SessionId; forwards: PortForward[]; status: PfStatus }`.
  - `usePortForwardingStore` with `entries: Record<Id, PfEntry>` (by host id),
    `connected(hostId: Id, connected: TunnelsConnected): void` (replaces an existing entry and fire-and-forget closes
    its old session), `onEvent(hostId: Id, sessionId: SessionId, event: PortForwardEvent): void` (ignores a stale
    session id; `tunnel` → `setTunnelState(sessionId, event.event)`), `disconnect(hostId: Id): Promise<void>`
    (removes the entry, `tunnelsClose`).
  - `ConnectProtocol = "ssh" | "sftp" | "tunnel"`; `homeView` and the view set by `start` are `"portForwarding"` for
    `"tunnel"`.
  - Tunnel states of a port-forwarding entry live in `useTunnelsStore` under the key = its `sessionId`.

`runTunnel(flow)` in `connect.ts`: like `runSftp` without re-attach. Port-forward events that arrive before
`connectTunnels` resolves are queued in the closure and replayed after `connected(...)`. If the flow is no longer
current, `tunnelsClose` the returned session.

- [ ] **Step 1: Failing tests**
  - store: `connected` adds the entry; second `connected` for the same host closes the old session id
    (`tunnelsClose` mock called with it); `onEvent` `reconnecting`/`reconnected`/`lost` update `status`; an event with
    another session id is ignored; `tunnel` events land in `useTunnelsStore.states[sessionId]`; `disconnect` removes
    and calls `tunnelsClose`.
  - connect: `start(hostId, "tunnel")` sets view `"portForwarding"`, calls `connectTunnels`, on success closes the
    modal and the entry exists; a `tunnel` event emitted on the channel before the invoke resolves ends up in
    `useTunnelsStore`; cancel while pending → returned session closed, no entry; cancel returns to
    `"portForwarding"`.
- [ ] **Step 2:** `bun run test -- portForwarding connect` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** `bun run typecheck && bun run test` → PASS.
- [ ] **Step 5: Commit** `feat(ui): connect flow and store for port forwarding`.

### Task 4: Screen, rail, shared rule rows, texts

**Files:**
- Create: `ui/src/screens/portForwarding/PortForwardingScreen.tsx`, `PortForwarding.module.css`,
  `PortForwardingScreen.test.tsx`
- Modify: `ui/src/screens/terminal/sidebar/TunnelPanel.tsx` (extract and export
  `TunnelRules({ rules, states, ready, sessionId }: { rules: PortForward[]; states: Record<Id, TunnelState> | undefined; ready: boolean; sessionId: SessionId | undefined })`;
  `TunnelPanel` uses it, behaviour unchanged), `ui/src/shell/Rail.tsx` (item active, `LATER_ITEMS` removed),
  `ui/src/shell/AppShell.tsx` (section for `"portForwarding"`), `ui/src/i18n/de.ts`, Rail/App tests if they assert
  the disabled item.

**Interfaces:**
- Consumes: Task 3 store and `useConnectStore.start(hostId, "tunnel")`; `TunnelRules`.

Texts (`t.portForwarding`): `title: "Port-Forwarding"`, `empty: "Noch keine Weiterleitungen. Regeln legst du im
Host-Editor unter „Verbindung“ an."`, `disconnected: "Getrennt"`, `connecting: "Verbindet …"`, `connected:
"Verbunden"`, `reconnecting: (n, max) => \`Verbindet neu (Versuch ${n} von ${max}) …\``, `lost: (reason) =>
\`Verbindung verloren: ${reason}\``, `connect: "Verbinden"`, `disconnect: "Trennen"`, `editHost: "Host bearbeiten"`.
`editor.forwardsHint` becomes: „Regeln laufen über die Terminal-Verbindung (Reiter „Tunnel“) oder ohne Terminal
im Screen „Port-Forwarding“.“

Card = stored host (`source === "linascp"`) with `forwards.length > 0`, sorted by name (`localeCompare`, as the host
list). Header: name, `user@address:port` (reuse the formatter `HostCard` uses), state text (`lost` in a
`role="alert"` red line, reason via `errorMessage`), button per spec §1. „Verbindet …“ = connect store `active` with
`protocol === "tunnel"` and this `hostId` and no `failed`. Rules: the entry's `forwards` when connected, else the
host's current `forwards`; `ready` only when `status.kind === "connected"`. An entry whose host no longer appears in
the list is disconnected (effect).

- [ ] **Step 1: Failing tests** (`PortForwardingScreen.test.tsx`): lists only stored hosts with rules, sorted;
  empty text; „Verbinden“ calls `start(id, "tunnel")`; connected card shows „Verbunden“, „Trennen“ calls
  `disconnect`, rule „Starten“ enabled and calls `tunnelStart(sessionId, ruleId)`; reconnecting text and disabled
  rule switches; lost text in an alert with „Verbinden“; host removed from the hosts store → `tunnelsClose` called.
  Rail test: „Port-Forwarding“ is a normal item and sets view `"portForwarding"`. Existing `TunnelPanel.test.tsx`
  stays green unchanged.
- [ ] **Step 2:** `bun run test -- PortForwarding Rail TunnelPanel` → FAIL.
- [ ] **Step 3: Implement** (CSS modules with tokens only, layout like `KeychainScreen`).
- [ ] **Step 4:** `bun run typecheck && bun run test && bun run build` → PASS.
- [ ] **Step 5: Commit** `feat(ui): port-forwarding screen`.

### Task 5: Docs and full check

**Files:**
- Create: `docs/acceptance-tp6b.md` (manual checklist in the style of `docs/acceptance-tp5a.md`: connect with
  password prompt, autostart, L/R/D through the screen, kill the network → reconnect keeps L/D, R re-requested,
  give up → all off, Trennen, same rule in terminal and screen → „Adresse belegt“).
- Modify: `CLAUDE.md` (layout: `connect/tunnel_session.rs`, `commands/port_forwarding.rs`; specs list; acceptance
  list; invariant line for TP6b; roadmap „6b … (done)“ once merged).

- [ ] **Step 1:** Write the docs.
- [ ] **Step 2:** Run all checks from Global Constraints → all green.
- [ ] **Step 3: Commit** `docs: TP6b acceptance checklist and project notes`.
