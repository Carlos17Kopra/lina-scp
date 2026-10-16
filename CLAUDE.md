# LinaSCP

Linux desktop SSH/SFTP client: SSH like Termius, SFTP like WinSCP. Built for personal use first, with a possible
non-commercial open-source release later. Target platform is Linux only: Debian 13, GNOME Wayland, NVIDIA.

## Communication & naming

- Chat with the user in **German**. Code, comments, identifiers and commit messages are in **English**. UI strings are
  German and live centrally in `ui/src/i18n/de.ts`.
- The app is called **LinaSCP**. Never use "Shellport", which was only the working title in `shellport-design.zip`.
  App identifier: `de.mc-kg.linascp`.

## Layout

- `crates/core`: Rust core library with no Tauri dependency.
  - Store: SQLite at `~/.local/share/linascp/linascp.db`.
  - Secrets: OS keyring only.
  - `known_hosts`, `sshconfig` (import/mirror), export/import (age), validation.
  - `connect/`: russh pipeline (jump host, auth, prompts, timeouts).
  - `session/`: local PTY and SSH sessions.
  - `keys.rs`: the user's SSH key files (TP5b): list, generate; `connect::copy_public_key` is ssh-copy-id.
  - `tunnel.rs`: port forwards L/R/D (TP5a) over a terminal session's connection, kept across reconnects.
  - `transfer/`: transfer queue (TP3b) over the `Side` trait (local fs, `SftpSession`), bandwidth pacer, `chmod`
    (TP4a), `compare` and sync helpers (TP4b), `keep` (TP4c: polls the local tree, uploads what changed).
  - `edit/`: editing remote files (TP3c): private copy in `~/.cache/linascp/edit/`, polled and uploaded through the
    transfer queue; the editor is started by `src-tauri/src/editor.rs` (gio, `text/plain` default).
- `src-tauri`: Tauri 2 app.
  - Commands in `commands/`, output pump with ack flow control in `output.rs`.
  - specta bindings are exported to `ui/src/ipc/bindings.ts`. That file is generated, never edit it by hand. Regenerate
    with `cargo test -p linascp exports_bindings`.
- `ui/`: React 19 + TypeScript (strict) + zustand + xterm.js.
  - `ipc/api.ts` wraps every command.
  - `stores/`, `screens/`, `components/` (kit with CSS modules, tokens only).
- `docs/superpowers/specs/2026-10-11-linascp-tp1-design.md`: TP1 spec. It is the binding authority for TP1 behaviour.
- `docs/superpowers/plans/`: TP1 implementation plan (done).
- `docs/superpowers/specs/2026-10-15-linascp-tp2a-sidebar-design.md`: TP2a spec (terminal sidebar, color schemes,
  session info).
- `docs/superpowers/specs/2026-10-15-linascp-tp2b-snippets-design.md`: TP2b spec (snippets).
- `docs/superpowers/specs/2026-10-15-linascp-tp2c-split-broadcast-design.md`: TP2c spec (split panes, broadcast).
- `docs/superpowers/specs/2026-10-15-linascp-tp2d-auto-reconnect-design.md`: TP2d spec (auto-reconnect).
- `docs/superpowers/specs/2026-10-15-linascp-tp3a-sftp-browse-design.md`: TP3a spec (SFTP connect, commander browsing,
  file operations).
- `docs/superpowers/specs/2026-10-15-linascp-tp3b-transfers-design.md`: TP3b spec (transfers, queue, conflicts, SFTP
  reconnect).
- `docs/superpowers/specs/2026-10-16-linascp-tp3c-bookmarks-edit-design.md`: TP3c spec (bookmarks, editing remote
  files).
- `docs/superpowers/specs/2026-10-16-linascp-tp4a-permissions-bandwidth-design.md`: TP4a spec (permissions, bandwidth
  limit, SFTP server command) and the TP4 split.
- `docs/superpowers/specs/2026-10-16-linascp-tp4b-compare-sync-design.md`: TP4b spec (compare and synchronize).
- `docs/superpowers/specs/2026-10-16-linascp-tp4c-keep-up-to-date-design.md`: TP4c spec (keep a remote folder up to
  date).
- `docs/superpowers/specs/2026-10-16-linascp-tp5a-tunnels-design.md`: TP5a spec (tunnels) and the TP5 split.
- `docs/superpowers/specs/2026-10-17-linascp-tp5b-keychain-design.md`: TP5b spec (keychain, keygen, ssh-copy-id).
- `docs/superpowers/specs/2026-10-17-linascp-tp5c-settings-design.md`: TP5c spec (settings screen).
- `docs/superpowers/specs/2026-10-17-linascp-tp6a-cleanup-design.md`: TP6a spec (backlog cleanup) and the TP6 split.
- `docs/acceptance-tp1.md`: manual acceptance checklist (`docs/acceptance-tp2a.md` … `docs/acceptance-tp2d.md` for
  TP2a–TP2d, `docs/acceptance-tp3a.md` for TP3a, `docs/acceptance-tp3b.md` for TP3b, `docs/acceptance-tp3c.md` for TP3c, `docs/acceptance-tp4a.md` for TP4a, `docs/acceptance-tp4b.md` for TP4b, `docs/acceptance-tp4c.md` for TP4c, `docs/acceptance-tp5a.md` for TP5a, `docs/acceptance-tp5b.md` for TP5b, `docs/acceptance-tp5c.md` for TP5c, `docs/acceptance-tp6a.md` for TP6a).
- `docs/backlog-after-tp1.md`: deferred review findings. Start TP2 planning there.

## Commands (run from the repo root)

```bash
bun install
bun run tauri dev                 # run the app (StrictMode: first terminal prompt may need Enter in dev)
bun run typecheck && bun run test # UI
bun run build                     # UI build (ui/dist)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1   # Docker sshd tests
bun run tauri build               # .deb + .AppImage in target/release/bundle/
```

## Invariants (do not break)

- **Secrets** live only in the OS keyring (`service="linascp"`). Never put them in the DB, logs, events, error
  messages, UI notices or plaintext exports.
- **Host keys** are stored only in `~/.ssh/known_hosts`, which is shared with OpenSSH. New entries are hashed, and
  every decision fails closed: anything other than an explicit accept is a reject.
- **Connections:** exactly one terminal `ConnectEvent` (`connected` | `failed`) per connect id, including errors
  before the pipeline starts and a cancel. `useConnectStore.start`/`retry` never reject.
  Automatic reconnects never prompt: any prompt fails the attempt with `inputRequired` (fail closed).
- **`~/.ssh` modes:**
  - "integrate" (default) is a read-only import.
  - "mirror" writes `~/.ssh/config.d/linascp.conf` plus one `Include` line at the top of `~/.ssh/config`.
  - The user's config is never rewritten otherwise.
  - Every value written to the mirror is validated in core (`validate.rs`), so one bad entry must never break
    the user's `ssh`.
- **Terminal output:**
  - Raw bytes over `Channel<RawChunk>` with ack flow control: pause above 1 MiB unacked, resume below 256 KiB.
  - The WebGL renderer is the default, with a DOM fallback on context loss.
  - Opaque background, and no `WEBKIT_DISABLE_*` unless compat rendering is enabled.
- **File permissions:** files holding secrets get mode 0600, their directories 0700.
- **Editing (TP3c):** a server file changed since the last known state is never overwritten without an explicit
  „Überschreiben“; uploads keep the remote mode and resolve symlinks; local copies live in 0700 directories, are never
  executable, and copies with changes the server does not have are never deleted.
- **SFTP (TP4a):** a recursive permission change never follows or changes symlinks below the selection; a set SFTP
  server command is never silently replaced by the `sftp` subsystem (invalid or failing means the connect fails).
- **Sync (TP4b):** a target changed since the compare is never silently overwritten or deleted; an unreadable
  directory aborts the compare and never counts as empty; local sync deletes go to the trash.
- **Keep up to date (TP4c):** never deletes; a server file changed since its last known state is never silently
  overwritten; two jobs never write the same target; a file goes up only after one quiet poll.
- **Tunnels (TP5a):** a `forwarded-tcpip` channel is accepted only for a forward this connection requested; without
  an address, local listeners bind to loopback only; the SOCKS proxy does `CONNECT` only and never asks for
  credentials; tunnels never prompt and never outlive their session.
- **Keys (TP5b):** generated private keys are written 0600 and never overwrite a file; ssh-copy-id sends only the
  checked public key, over stdin.

## Workflow

- Use TDD: write a failing test first. Keep test output free of warnings, including act() and jsdom noise.
- The main branch is `master`, remote `origin` = https://github.com/Carlos17Kopra/lina-scp (public). Pushing
  `master` is fine. Do feature work on branches.
- Commits end with the `Co-Authored-By:` trailer from the session's attribution instructions.
- **Release:**
  1. Bump `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`. Keep them equal and
     update `Cargo.lock`.
  2. Commit, then `git tag vX.Y.Z && git push origin vX.Y.Z`.
  3. `.github/workflows/release.yml` runs all checks and builds a **draft** release with the .deb, the AppImage and
     `SHA256SUMS`. A full run takes about 25 minutes.
  4. Publish the draft with `gh release edit vX.Y.Z --draft=false`.

## Roadmap

- TP1, foundation (done, v0.1.0): hosts, editor, connect flow, host keys, error screens, terminal tabs, local
  terminals, settings, export/import, packaging.
- TP2: complete the terminal, and work off the backlog in `docs/backlog-after-tp1.md`.
- TP3: SFTP (3a browse, 3b transfers, 3c bookmarks + edit: done).
- TP4: advanced SFTP (4a permissions, bandwidth, server command; 4b compare + sync; 4c keep up to date: done).
- TP5: tunnels, keychain, keygen/ssh-copy-id, full settings (5a–5c: done).
- TP6: 6a backlog cleanup (done), 6b port-forwarding screen, 6c SSH agent identities.
- No sync between devices. Moving data to another device goes through export/import (`.linascp` files).
