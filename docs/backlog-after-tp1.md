# Backlog after TP1

These findings came out of the TP1 task reviews and the final whole-branch review. They were deliberately
deferred because none of them blocked TP1, and they are kept here so they don't get lost. Items that were
fixed before the merge are not listed.

## Security / robustness (prioritise in TP2)

- **Mirror mode:**
  - A crafted import can define a `Host` alias equal to a real hostname (e.g. `github.com`) with an attacker `HostName`. This is inherent to import plus mirror. Consider namespacing aliases or confirming imported hosts.
  - A `ProxyJump` to a jump host that was skipped because its alias collides with another host's resolves to that other host. Omit `ProxyJump` when the jump host itself was not rendered.
  - The alias of a jump host imported from ssh-config is also ASCII-sanitized in `ProxyJump`, so a non-ASCII imported alias produces a wrong `ProxyJump`. Use the raw alias for `source != Linascp`.
  - `merge_hosts` also hides a hand-written ssh-config alias that equals a stored host's sanitized name.
- **known_hosts:**
  - `@revoked` lines are ignored.
  - `replace()` renames over a symlinked `known_hosts`, and its read-modify-write can lose a concurrent `ssh` append.
- **Sessions:**
  - Spec §9: panics in session tasks are not caught (no `catch_unwind`; `SessionEvent::Error` is never produced for them).
  - The local PTY writer thread stays parked after close if a write is stuck on a full PTY. Use a non-blocking fd plus poll.
- **Import:**
  - Embedded key files of rejected import entries are still written to `~/.ssh/linascp`.
  - Jump-chain rejection runs in a single pass.
  - Import writes are not one DB transaction.
- **Secrets hygiene:** the plaintext export JSON, keyboard-interactive responses and key text are not zeroized. The passphrase is saved after local decrypt, before the server has accepted it.
- **Connect:**
  - A cancel invoke processed before `connect_ssh` registers its token is still lost; the window is tiny now.
  - Terminal input order across separate `session_write` invokes is not guaranteed.

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

## Packaging / CI

- deb `Depends` lists `libwebkit2gtk-4.1-0`/`libgtk-3-0` twice, because the bundler appends its own; the package name is `lina-scp`.
- Re-running the release workflow for the same tag can create a second draft release.
- The README apt/backports commands were not verified on a clean system.

## Tests / code quality

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
