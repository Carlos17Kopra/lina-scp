## Contract Adjustments (binding; supersede `00-contract.md` where they differ)

Collected from the verified plan parts 10/20/30.

### Serialization
- Every enum with struct variants: `rename_all_fields = "camelCase"` (e.g. `promptId`, `sessionId`, `keyPath`, `profileId`).
- `CoreError`: `#[serde(tag = "type")]` → `{"type":"network","kind":"timeout"|"refused"|"dns"|"other","message":..}`, `{"type":"hostKeyRejected","message":..}`, `{"type":"auth","rejectedMethods":[..],"message":..}`, `{"type":"cancelled"}`, `{"type":"keyring"|"store"|"config"|"io"|"internal","message":..}`. UI switches on `error.type`.
- `HostAuth` = `{"kind":"profile","profileId":..}` | `{"kind":"custom","auth":{method,keyPath,hasSecret}}`.
- `SecretOwner` adjacently tagged: `{"kind":"host"|"profile","id":"<uuid>"}`.
- i64 fields exported to TS as `number`.

### Crates / tooling
- Edition 2024. `specta = "=2.0.0-rc.25"`, `tauri-specta = "=2.0.0-rc.25"`, `specta-typescript = "=0.0.12"`, tauri 2.12, russh 0.64 (`russh::keys::*`), age 0.11 (0.12 conflicts with russh), keyring-core 1 + dbus-secret-service-keyring-store 1 (crypto-rust) — **blocking**, use `spawn_blocking`. Build deps: libdbus-1-dev, pkg-config.
- Vitest 4.1.11 + jsdom 29.1.1 (Node 20 on Debian 13).

### Core behaviour
- `connect_ssh` (core) does not emit `Connected`; the Tauri command registers the session, emits `Connected { sessionId }` and returns the `SessionId`. Core emits `Failed` on error.
- One global `PromptBroker`; prompts flow through the per-call `emit`. `connect_answer(promptId, answer) -> bool`.
- Cancelled password/passphrase prompt → `cancelled`; rejected/cancelled host key → `hostKeyRejected`.
- `effective_user`: host.user → profile.user → local `$USER`.
- `Store::save_*` don't mutate the argument; commands return the re-read value.

### Tauri commands (changes)
- Raw output: commands take `Channel<RawChunk>` (sends `InvokeResponseBody::Raw`); TS sees `Channel<ArrayBuffer>`.
- `connect_test(connect_id, host, events)` (extra `connect_id`, cancellable).
- New `session_ack(session_id, bytes: u32)`; backend pauses reading above 1 MiB unacked, resumes below 256 KiB; output flushed every 8 ms or at 64 KiB.
- `Settings` gains `compatRendering: boolean` (sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` at next start if on).
- Deleting host/profile removes its keyring secrets.

### UI api.ts
`hostsList, hostSave, hostDelete, groupsList, groupSave, groupDelete, profilesList (→ {profile, usedBy}[]), profileSave, profileDelete, secretSet(owner, kind, value), secretDelete, settingsGet, settingsSet, connectSsh(connectId, hostId, cols, rows, events, output, sessionEvents) → SessionId, connectAnswer(promptId, answer) → boolean, connectCancel, connectTest(connectId, host, events), sessionOpenLocal(cols, rows, output, sessionEvents), sessionWrite(id, string|Uint8Array), sessionResize, sessionAck(id, bytes), sessionClose, exportFile(path, includeSecrets, includeKeyFiles, passphrase|null), importPreview(path), importApply(path, passphrase|null, Record<Id, Resolution>), sshConfigReload`; helpers `newId(), createEventChannel<T>(fn), createOutputChannel(onData), createOutputStream() → {channel, attach(onData) → detach, bind(sessionId), ack(bytes)}`. Errors helpers in `ipc/errors.ts`: `isCoreError, errorMessage, errorDetails`. `SessionEventDto = {kind:"exit", code} | {kind:"error", error}`.

### UI stores
- `useUiStore`: `{view, setView, modal, openModal, closeModal, theme, setTheme, searchFocusToken, focusSearch}`; `ModalState = {type:"editor"; target:{kind:"host"|"profile"; id?}} | {type:"settings"|"firstRun"|"export"|"import"|"connect"}`.
- `useSettingsStore`: `{settings, loaded, load, save(patch)}` (optimistic), `DEFAULT_SETTINGS`.
- `useHostsStore`: `{hosts, groups, profiles, loaded, filter, query, groupId, selectedId, load, reloadSshConfig, saveHost, deleteHost, saveProfile, deleteProfile, saveGroup, deleteGroup, setFilter, setQuery, setGroupId, select}`.
- `useSessionsStore`: `{tabs, activeId, addTab, closeTab, setActive, updateTab, openLocal}` (`openLocal` no-op until Task 22).
- `useConnectStore`: `ConnectFlow` per contract; `start/answer/cancel/retry` stubs until Task 21.

### UI components (barrel `ui/src/components/index.ts`)
`Button{variant=secondary default, size sm|md|lg=34|40|44, icon: LucideIcon}`, `IconButton{label, icon, size sm|md|lg=28|34|44}`, `TextField{label, value, onChange(value), error, hint, mono, type, placeholder}`, `Select<T>`, `Checkbox{label, checked, onChange(checked), hint}`, `Segmented<T>` (radiogroup), `Tabs{label, tabs, value, onChange, orientation, idBase}` + `tabId()/tabPanelId()`, `Chip{tone, mono}`, `Card{selected, padded}`, `ProgressBar{value, tone, label}`, `StepList{steps:{id,label,state,meta?,detail?}[]}`, `Dialog{title, open, onClose, size sm|md|lg=440|640|860, footer, subtitle, icon, role, initialFocusRef, hideClose}`, `ToastProvider` + `useToast().show({tone, message, details?, durationMs?})`, `Tone = accent|amber|red|green|neutral`.

### UI conventions
- i18n: `t = {app, common, rail, toast, steps, shell, stubs}`; screen tasks add `t.hosts`, `t.editor`, `t.connect`, `t.terminal`, `t.settings`.
- Stubs to replace (same names/props): `HostsScreen`, `TerminalScreen`, `EditorModal({target})`, `ConnectScreen`, `SettingsDialog`, `FirstRunDialog`, `ExportDialog`, `ImportDialog`. `SftpPlaceholder` final for TP1.
- AppShell keeps all views mounted (`hidden`). Shortcuts are capture-phase on window, inactive while a modal is open; inside `[data-terminal]` only Ctrl+Shift+T and Ctrl+, are handled. Host search focuses on `searchFocusToken` change. `FirstRunDialog` saves `firstRunDone: true`.
- CSS Modules next to components, tokens only, radii 6/8/10/12/16, `cx()`, global `.mono`, `.visually-hidden`.
- Tests: `vi.mock("<rel>/ipc/api")` automock + `vi.mocked(api.fn).mockResolvedValue(...)`; reset stores with `setState(getInitialState(), true)`; wrap toast users in `<ToastProvider>`; run `bun run test <filter>` from repo root.
- Terminal: `createOutputStream` → `attach(bytes => term.write(bytes, () => stream.ack(bytes.length)))`; WebGL only on the visible terminal, dispose when hidden, DOM fallback on `onContextLoss` or renderer setting `dom`; opaque background.
