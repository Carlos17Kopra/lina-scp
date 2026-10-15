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
- `docs/acceptance-tp1.md`: manual acceptance checklist (`docs/acceptance-tp2a.md` … `docs/acceptance-tp2c.md` for
  TP2a–TP2c).
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
- TP3: SFTP.
- TP4: advanced SFTP.
- TP5: tunnels, keychain, keygen/ssh-copy-id, full settings.
- No sync between devices. Moving data to another device goes through export/import (`.linascp` files).
