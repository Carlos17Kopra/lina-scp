# Backlog after TP1

These findings came out of the TP1 task reviews and the final whole-branch review. They were deliberately
deferred because none of them blocked TP1, and they are kept here so they don't get lost. Items that were
fixed before the merge are not listed.

## Security / robustness (prioritise in TP2)

- **Mirror mode:** a crafted import can still define an undotted alias that is a real host name (`localhost`, a search-domain short name like `gitlab`) with an attacker `HostName`; only dotted aliases must equal their address. Full fix: confirm imported hosts before they reach the mirror, or namespace aliases.
- **Secrets hygiene:** keyboard-interactive responses are not zeroized: russh takes the `Vec<String>` by value, and the answer has already passed through IPC/serde copies. Needs an upstream API taking secret buffers.

## Behaviour / UX

- Hosts imported from ssh-config without `IdentityFile` only try the agent; OpenSSH also tries the default `~/.ssh/id_*` files.
- `has_secret` is not updated when a secret is saved from a connect prompt.
- A startup failure (e.g. the DB schema is newer than the build) only goes to stderr.
- A stale stored password is never cleared after a successful retype (fail2ban risk).
- `rejected_methods` actually holds the server's remaining methods (UI label).
- Jump direct-tcpip errors are always classified as `Other`; agent transport errors are reported as Auth.
- Fingerprint row in host details is a placeholder (`—`).
- A stale `groupId` after deleting a group gives a misleading empty state; the selection survives filtering.
- Editor:
  - Late channel events after `connectTest` resolves may be dropped.
  - `hasSecret` stays set across method changes and keyring failures.
  - Some validation errors show before the user touches a field.
  - A test result stays visible after the draft is edited.
  - `sftp.parallel` accepts 0, and `uploadPerms` is not validated as octal.
- Editor nav-stack edge cases:
  - The group write-back is skipped while a profile frame is on top.
  - An edit typed during `saveGroup` is not persisted if the first save succeeds.
- Connect overlay: focus can drop to `body` after a dialog closes without a view change.
- In `tauri dev`, the StrictMode double mount can swallow the first prompt of a new terminal (production builds are unaffected).
- Local tab titles can repeat after closing a tab.
- WebGL addon not disposed when `loadAddon` throws.
- Middle-click paste is also forwarded to mouse-tracking apps.
- Select-to-copy and middle-click use CLIPBOARD, not PRIMARY (a WebKitGTK limitation, accepted for TP1).
- FirstRunDialog `finish()` is not guarded against a double Escape.
- A long import path has no overflow handling.
- The first-run effect can replace an already open modal.
- Two concurrent settings saves can revert to a stale value.
- Duplicate load-failure toasts.
- The early session-event buffer keeps one entry per closed tab.
- Toast actions are unreachable by keyboard while a dialog is open (Dialog focus guard).
- Component a11y polish:
  - Tabs `aria-controls` points at panels that are not rendered.
  - The Dialog focusable selector includes disabled and hidden elements.
  - The toast "copied" state never resets.
  - `aria-describedby` overwrites the value passed in by the consumer.
- `sync_mirror` and `settings_set` do a small amount of blocking I/O on the async runtime. Wrap them in `spawn_blocking`.

- TP2a terminal sidebar (from the branch review):
  - Fast A+/A− or scheme clicks: optimistic settings saves can finish out of order or revert to a stale value;
    in host scope a quick double click loses a step until `hostSave` + reload resolve. Serialize saves.
  - The "Tunnel folgen …" hint shows below every sidebar panel instead of on the disabled Tunnel tab.
  - The scheme radiogroup has no roving tabindex / arrow keys (10 tab stops).
  - Every xterm resize calls `updateTab`, re-rendering all terminal tabs while the window is dragged.
- TP2b snippets (from the branch review):
  - Import "Duplizieren" appends " (Import)" after validation, so a 64-character name is stored with 73; editing it
    later fails until the name is shortened. Truncate before adding the suffix.
  - The spec asks for field-level validation errors; core returns `Config` without a field and the editor shows a
    toast. Either add field errors or update the spec.
  - The placeholder send after the dialog closes relies on React flushing passive effects for discrete events before
    the `queueMicrotask`; send from an effect after `pending === null` commits instead.
  - Insert/▶ are enabled by `tab.status` only, not by a registered paste target; clicking without one does nothing.
  - Enter during IME composition confirms the placeholder dialog (check `isComposing`).
  - ▶ on a command ending in a newline sends a second Enter (empty prompt line).
- TP2c split panes and broadcast (from the branch review):
  - Early session-event buffer leaks: a session closed after its pane/key is gone still emits Exit, which stays in
    the `early` map forever. Drop events for keys that were closed.
  - Divider drag has no `pointercancel`/`lostpointercapture` handler, and every `pointermove` refits all panes and
    sends `sessionResize`; add cancel handling and rAF throttling.
  - A picking pane hides all sidebar panels behind "Kein Terminal"; only the Sitzung panel needs it (Snippets
    already locks insert/▶ by status).
  - Ctrl+Shift+←/→ is taken from text fields (picker search, snippet search); skip pane focus for inputs outside
    `[data-terminal]`.
  - With broadcast on, every pane gets the same orange frame; the source pane is not visible. Combine focus and
    broadcast frames.
  - A pane connect whose pane disappeared ends silently (session closed, no notice).
  - The terminal-reply filter for broadcast is a pattern match; a modified F3 (`ESC[1;2R`) is not broadcast either.

## Packaging / CI

- deb `Depends` lists `libwebkit2gtk-4.1-0`/`libgtk-3-0` twice, because the bundler appends its own; the package name is `lina-scp`.
- Re-running the release workflow for the same tag can create a second draft release.
- The README apt/backports commands were not verified on a clean system.

## Tests / code quality

- `local_pty_roundtrip` and `a_session_that_exits_on_its_own_is_removed_from_both_maps` (src-tauri) spawn the developer's real `$SHELL`; a slow or interactive rc file (e.g. fastfetch waiting for terminal replies) makes them time out. Make them hermetic.

- Test gaps:
  - Prompter events are never asserted.
  - The trust test doesn't check that the entry is hashed.
  - Cancel is only tested pre-cancelled.
  - The cancel-after-success arm has no test.
  - `api.test` covers 5 of 26 wrappers.
  - Dialog `initialFocusRef`/`alertdialog`/`hideClose` are not tested.
  - Toast durations are not tested.
  - No editor test for the keyring-failure toast or the save-failure toast.
  - The disposal test doesn't assert that subscriptions and the ResizeObserver are released.
- Dead/duplicated code:
  - Unused `t.stubs.*` strings.
  - `TAB_ORDER` is duplicated.
  - ConnectScreen duplicates the `connectView` branching.
  - `pickKeyFailed` is reused across screens; move it to `t.common`.
- Store:
  - `list_hosts` does N+1 queries.
  - Poison recovery is undocumented.
  - `HostKeyStatus` lacks `rename_all_fields`.
