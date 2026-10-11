# Tech Stack Research — Linux SSH/SFTP Client (Tauri 2 + React + TS + Rust)
Research date: 2026-10-11. All versions verified against crates.io / npm registry APIs and official docs where possible (see Source URLs per section). Where a WebFetch summarizer could not extract content from a JS-rendered page, the raw JSON API (crates.io `/api/v1/crates/<name>`, npm registry) was used instead for version/date ground truth.

---

## 1. Tauri 2

### Latest version
- **`tauri` (Rust crate):** 2.12.3 (crates.io, updated 2026-10-10). Workspace is on track toward a 3.0 line — `3.0.0-alpha.4` exists on crates.io but is **not** the stable channel.
- **`@tauri-apps/cli` (npm):** 2.12.2 (published 2026-10-10)
- **`@tauri-apps/api` (npm):** 2.12.3 (published 2026-10-10)
- **Maintenance status:** Actively maintained, frequent releases (weekly-ish cadence), official org `tauri-apps`.

Sources: https://crates.io/crates/tauri , https://registry.npmjs.org/@tauri-apps/cli , https://registry.npmjs.org/@tauri-apps/api

### Streaming high-throughput binary data backend → frontend (terminal output)
**Use Tauri's `Channel` IPC primitive, not the event system, for PTY/terminal-style streaming.**

- Tauri's own docs state the event system ("emit"/"listen") is explicitly **not designed for low latency or high throughput**: event payloads are always JSON strings (not binary-friendly), events emitted in rapid succession from an async context **can be delivered out of order** to listeners, and there's no fine-grained capability/permission scoping per event payload.
- `Channel<T>` is purpose-built for exactly this: it's what Tauri itself uses internally for download progress, **child-process output**, and WebSocket message relay. It guarantees ordered delivery and is much lower overhead than evaluating injected JS per event.
- Pattern: create a `tauri::ipc::Channel<ByteSlice-like payload>` on the frontend (via `@tauri-apps/api/core`'s `Channel` class, which exposes an `id` and an `onmessage` callback), pass its `id` into a Rust command, and from a background task/thread reading the PTY master fd, call `channel.send(data)` per chunk. This avoids the JSON-string tax; channel payloads can be raw bytes (tagged variants) and are delivered in strict order.
- A third-party crate, **`tauri-conduit`**, exists for even more specialized binary streaming (configurable capacity lossy/ordered channels, push-from-any-thread), but it's not official/widely adopted — stock `Channel` is sufficient and is the documented, supported approach for this use case.
- Avoid the Tauri custom-protocol (`tauri://` asset protocol) approach for live terminal output — that's meant for serving static-ish resources/streamed files (e.g. video), not for a continuously-updating bidirectional command channel; it adds HTTP-semantics overhead that channels don't need for this use case.

Sources: https://v2.tauri.app/develop/calling-frontend/ (official Channels vs Events guidance) , https://github.com/userFRM/tauri-conduit

### Linux build dependencies (Debian 13 / trixie)
Official Tauri prerequisites for Debian/Ubuntu (apt packages):
```
libwebkit2gtk-4.1-dev \
build-essential \
curl \
wget \
file \
libxdo-dev \
libssl-dev \
libayatana-appindicator3-dev \
librsvg2-dev
```
- Tauri 2 moved to **webkit2gtk-4.1** (soup3-based, needed for Flatpak/GNOME runtime compatibility) starting at the 2.0.0-alpha.3 milestone; webkit2gtk-4.0 (soup2) is the old Tauri 1 requirement and should not be used.
- Debian 13 (trixie, released 2025-08-09) ships current `libwebkit2gtk-4.1-*` packages in its repos, so no special trixie-only workaround is needed — the standard package list above applies. Docs do not call out Debian 13 specifically; this is the generic Debian/Ubuntu list.
- `libgtk-3-dev` and `patchelf` are not required to be installed manually — they come in transitively via build-essential/webkit2gtk-dev or are bundled by the Tauri CLI's AppImage tooling.
- For Linux rendering problems (common on NVIDIA/Wayland setups — blank windows, WebGL falling back to a slow software path), see Tauri's dedicated "Linux Graphics Issues" doc.

Sources: https://v2.tauri.app/start/prerequisites/ , https://v2.tauri.app/blog/tauri-2-0-0-alpha-3/ , https://v2.tauri.app/develop/debug/linux-graphics/

### Bundling targets
Tauri 2's bundler (`tauri-bundler`, now closely related to `cargo-packager`) supports, on Linux: **`.deb`**, **`.rpm`**, **`AppImage`**, plus Flatpak/Snap/AUR packaging paths via community tooling/guides.
- Active development in 2026: `tauri-bundler` v2.10.0 (2026-09-26) added bundle-size reporting; recent fixes include updated `linuxdeploy`/GTK plugin for AppImages, fixes for `EGL_BAD_PARAMETER` crashes on Fedora/Arch, AppImages no longer forcing `GDK_BACKEND=x11` (now defaults to native Wayland backend), and `.deb`/`.rpm` recognized as self-contained updater artifacts.
- Practical note carried over from Tauri 1 and still true: build on the **oldest glibc** base you want to support (e.g., an older Ubuntu/Debian in CI) rather than the newest, since Tauri binaries dynamically link glibc.

Sources: https://v2.tauri.app/release/tauri-bundler/v2.10.0/ (and related release-log pages), general search aggregation of 2026 bundler changelog entries

### Plugin for persistent store / SQL
| Plugin | Latest version | Status | Notes |
|---|---|---|---|
| `tauri-plugin-store` | **2.5.0** (crates.io, updated 2026-09-30) | Active, official (`tauri-apps/plugins-workspace`) | Simple persisted key-value JSON store; good for small settings/preferences, not a real database. |
| `tauri-plugin-sql` | **2.5.0** (crates.io, updated 2026-09-30) | Active, official (`tauri-apps/plugins-workspace`) | Wraps SQLite/MySQL/Postgres via `sqlx`; frontend talks to it over the JS bindings, migrations supported. |

Both plugins also have `3.0.0-alpha.2` pre-releases tracking the Tauri 3 line, but 2.5.0 is current stable for the Tauri 2 target.

Sources: crates.io API responses for `tauri-plugin-sql` and `tauri-plugin-store`

---

## 2. russh

### Latest version & status
- **`russh`: 0.64.1** (crates.io, updated 2026-10-05). Actively maintained under **warp-tech/russh** (fork/continuation of the original Pijul-era `russh`), used in production by Warp terminal. ~88 releases total, 8.2M+ downloads.
- **Debian packaging:** `rust-russh` 0.57.1-1 was recently reviewed/queued for Debian's package tracker (2026), i.e. it's making it into distro repos, a maintenance-health signal.
- **Security track record:** CVE-2023-48795 (Terrapin prefix-truncation attack) was fixed by adding **strict-kex** support starting at russh 0.40.2+ — relevant because current OpenSSH (Debian 13 ships 10.0p1) also mandates/strongly prefers strict-kex; make sure to pin ≥0.64.1 (current) regardless. A second recent issue, **CVE-2026-68930** (server dispatches channel-scoped `Handler` callbacks for channel IDs that were never opened/confirmed, in `server/encrypted.rs`), was fixed in **0.62.5+** — not relevant if you're building a client-only app, but worth knowing if any server/jump-host-relay code path is added later.

Sources: https://crates.io/crates/russh (API) , https://github.com/warp-tech/russh/security/advisories/GHSA-45x7-px36-x8w8 , https://security-tracker.debian.org/tracker/CVE-2026-68930

### Is russh-keys merged into russh?
**Effectively yes, going forward.** `russh-keys` as a standalone crate is stale: crates.io shows its `max_stable_version` as **0.49.2**, last updated **2025-01-08** — no 2026 releases, while `russh` itself ships continuously and includes a `russh::keys` module (re-exporting/building on the RustCrypto `ssh-key` crate) that provides key loading/decoding, agent client, and known_hosts helpers directly. **Recommendation: depend on `russh::keys` (the module inside the `russh` crate), not the separate `russh-keys` crate**, to avoid pulling in an unmaintained duplicate dependency tree.

Sources: crates.io API for `russh-keys` (0.49.2, updated 2025-01-08) vs `russh` (0.64.1, updated 2026-10-05); https://docs.rs/russh/latest/russh/keys/index.html

### Feature coverage
| Feature | Supported? | API |
|---|---|---|
| Password auth | Yes | `client::Handler` + `authenticate_password` on the client session/handle |
| Publickey auth: ed25519 | Yes | `russh::keys` (`PrivateKey`/`PublicKey`, `Algorithm` enum covers Ed25519) |
| Publickey auth: rsa-sha2-256/512 | Yes (RFC 8332 signature algorithms, needed since OpenSSH deprecated `ssh-rsa`/SHA-1 by default) | via `Algorithm`/`Preferred` key-algorithm negotiation |
| Publickey auth: ecdsa (nistp256/384/521) | Yes | `russh::keys::Algorithm` |
| Encrypted OpenSSH private keys w/ passphrase | Yes | `russh::keys::decode_openssh()` / `decode_secret_key()` / `load_secret_key()` — all accept a password argument for decryption |
| ssh-agent auth | Yes | `russh::keys::agent::client::AgentClient` (agent protocol client; there's also an `agent::server` side) |
| keyboard-interactive (2FA) | Yes | `client::Handler` supports keyboard-interactive as one of the four auth methods (publickey, password, keyboard-interactive, none), plus OpenSSH certificate auth |
| Host key verification callback | Yes | `client::Handler::check_server_key(&mut self, server_public_key: &PublicKeyOrCertificate) -> Result<bool, Error>` — **must be overridden**, default implementation rejects all keys (fail-closed by design) |
| SHA256 fingerprints | Yes, derivable | The returned `PublicKeyOrCertificate`/`PublicKey` types (from `russh::keys`, built on RustCrypto `ssh-key`) expose a `fingerprint()`-style API consistent with `ssh-key`'s fingerprint support (SHA256 base64, OpenSSH-style `SHA256:...` format) |
| Jump host / ProxyJump | Yes, via manual tunneling | No built-in "ProxyJump" string parsing, but fully supportable: open a session to the jump host, call `Handle::channel_open_direct_tcpip(host_to_connect, port, originator_addr, originator_port)` to get a `Channel`, call `.into_stream()` to get an `AsyncRead + AsyncWrite`, then feed that stream into `russh::client::connect_stream()` to run a second SSH handshake over the tunnel. This is the idiomatic multi-hop pattern (chainable for multiple jumps). |
| Keepalive | Yes | `Handle::send_keepalive(want_reply: bool)` and `Handle::send_ping()` (wait for pong); `client::Config` also documents keepalive-interval style options |
| PTY request + resize | Yes | `Channel::request_pty(...)` (term type, col/row, pixel dims, terminal modes) and `Channel::window_change(col_width, row_height, pix_width, pix_height)` |
| Local port forwarding | Yes | `direct-tcpip` channel type via `channel_open_direct_tcpip` |
| Remote port forwarding | Yes | `Handle::tcpip_forward(address, port)` / `cancel_tcpip_forward`, server-side `forward-tcpip`/`channel_open_forwarded_tcpip` |
| Dynamic (SOCKS) forwarding | Partial — no built-in SOCKS proxy server | russh gives you the `direct-tcpip` channel primitive per-connection; a dynamic/SOCKS5 proxy needs an app-level SOCKS listener (e.g. `fast-socks5`) that, per incoming connection, opens a fresh `direct-tcpip` channel to the requested destination. This is the same approach OpenSSH's `-D` effectively does under the hood. |
| Unix domain socket forwarding | Yes (bonus) | `direct-streamlocal` (client-only) and `forward-streamlocal` channel types |

### Known Debian/OpenSSH interop notes
- Debian 13 (trixie) ships **OpenSSH 10.0p1**. Current OpenSSH enforces **strict-kex** and has deprecated/disabled `ssh-rsa` (SHA-1 signatures) by default in favor of `rsa-sha2-256`/`rsa-sha2-512` — russh 0.64.1 supports both strict-kex (fixed since 0.40.2, for Terrapin) and RSA-SHA2 signature algorithms, so no special workaround is needed as long as you're on a current russh version.
- No open high-severity interop bugs specific to Debian/OpenSSH were found for the current 0.64.x line at the time of writing; the two CVEs above are the only notable security history and both are long-fixed in 0.64.1.

Sources: https://docs.rs/russh/latest/russh/client/trait.Handler.html , https://docs.rs/russh/latest/russh/client/struct.Handle.html (or `Handle<H>`) , https://docs.rs/russh/latest/russh/struct.Channel.html , https://docs.rs/russh/latest/russh/keys/index.html , https://raw.githubusercontent.com/warp-tech/russh/main/README.md , https://www.debian.org/News/2025/20250809.en.html (OpenSSH 10.0p1 in trixie)

---

## 3. russh-sftp

### Latest version & maturity
- **`russh-sftp`: 3.0.1** (crates.io, updated 2026-09-28). Maintained separately by **AspectUnk/russh-sftp** (not under the warp-tech org, but it's the de-facto standard SFTP subsystem implementation for russh). ~3.6M downloads, actively updated (last bump within the last two weeks of this research date).
- Implements the **SFTP protocol version 3** (the version essentially every OpenSSH server speaks), with OpenSSH protocol extensions: `limits@openssh.com`, `fsync@openssh.com`, `statvfs@openssh.com`.
- Maturity assessment: solid, widely used, but a single-maintainer project — worth pinning exact versions and watching the repo for responsiveness if you hit edge cases.

Sources: crates.io API for `russh-sftp` ; https://docs.rs/russh-sftp/latest/russh_sftp/client/struct.SftpSession.html

### Client API
Via `SftpSession` (client module):
- Directory listing: `read_dir()` (iterator over entries)
- Open/create: `open()` (read-only), `create()` (write-only, truncates), `open_with_flags()` / `open_with_flags_and_attributes()` for custom `OpenFlags`
- Read/write: convenience `read()` / `write()` whole-file helpers; file handles returned by `open`/`create` implement `AsyncRead`/`AsyncWrite` for streaming
- Metadata: `metadata()`, `symlink_metadata()` (lstat-equivalent, doesn't follow symlinks), `set_metadata()` (covers chmod/perm/size/time attrs — SFTP v3 attribute struct)
- Filesystem ops: `rename()`, `remove_file()`, `remove_dir()`, `create_dir()`, `symlink()`, `read_link()`, `hardlink()`, `canonicalize()`, `expand_path()` (`~` expansion via OpenSSH extension), `try_exists()`
- Extras: `fs_info()` (statvfs), `set_timeout()` (default 10s response timeout)

### Performance notes
- The official docs surface does **not** document built-in request pipelining/batching at the `SftpSession` convenience-method level (i.e., `read()`/`write()` whole-file helpers are not automatically parallelized).
- For high-throughput transfers you should pipeline manually: issue multiple concurrent `read`/`write` requests at different offsets against the same open file handle (the SFTP protocol itself is designed to allow many outstanding requests per handle, which is what gives SFTP its speed advantage over naive serial reads) — spawn N tokio tasks sharing a cloned handle/session, each responsible for a byte-range chunk, similar to how OpenSSH's own SFTP client/`-R` option and tools like `sftp-benchmarks` pipeline requests. This is something to design explicitly in the app rather than get for free from the crate.

Sources: https://docs.rs/russh-sftp/latest/russh_sftp/client/struct.SftpSession.html , crates.io API for `russh-sftp`

---

## 4. Parsing `~/.ssh/config` and `~/.ssh/known_hosts` in Rust

| Need | Crate | Version | Notes |
|---|---|---|---|
| `~/.ssh/config` parsing | **`ssh2-config`** | **0.8.1** (crates.io, updated 2026-09-28) | Despite the name, it's **not tied to libssh2/ssh2-rs** — README states it's usable "with any other OpenSSH compatible SSH client implementation," i.e. fine to pair with russh. Supports `Host`, `HostName`, `User`, `Port`, `IdentityFile`, `ProxyJump`, `Ciphers`/`KexAlgorithms`/`MACs`/`HostKeyAlgorithms`, with a `query()` API to resolve effective per-host config (pattern matching, `Include`, etc.). Actively maintained (veeso/ssh2-config). |
| `~/.ssh/config` parsing (alt.) | `ssh_config` | 0.1.0 | Effectively abandoned — single 0.1.0 release from 2020-01-10, no updates since. **Not recommended**; use `ssh2-config` instead. |
| `~/.ssh/known_hosts` read | **`russh::keys`** (built on RustCrypto `ssh-key`) | bundled in russh 0.64.1 | `check_known_hosts()` / `check_known_hosts_path()` re-exported from the `known_hosts` submodule — directly usable inside a `check_server_key` callback. |
| `~/.ssh/known_hosts` read/write incl. hashed entries | **`ssh-key`** (RustCrypto) | **0.6.7** (crates.io, updated 2026-06-29) | Dedicated `known_hosts` module: `KnownHosts` parser, `Entry` struct (host patterns + key), `HostPatterns` enum, `Marker` enum (`@cert-authority`/`@revoked`). The crate broadly documents encode/decode support for key, `authorized_keys`, and `known_hosts` formats including the OpenSSH hashed-hostname format (`\|1\|salt\|hash`), so writing new (optionally hashed) entries back to the file is supported at the crate-capability level — you compose it yourself (append an `Entry`, using the hashing helper, then serialize). This is the lower-level crate that `russh::keys` wraps. |

**Recommendation:** `ssh2-config` for `~/.ssh/config`, and `russh::keys`/`ssh-key`'s `known_hosts` module for reading/writing `~/.ssh/known_hosts` (including hashed entries) — no need for a 3rd-party known_hosts-specific crate; this keeps you on the same `ssh-key` dependency russh already pulls in, avoiding duplicate crypto/key-parsing code.

Sources: https://docs.rs/ssh2-config/latest/ssh2_config/ , crates.io API for `ssh2-config`, `ssh_config`, `ssh-key` ; https://docs.rs/russh/latest/russh/keys/index.html ; https://docs.rs/ssh-key/latest/ssh_key/known_hosts/index.html

---

## 5. `keyring` crate (Linux: Secret Service / GNOME Keyring / KWallet)

### Latest version
- **`keyring`: 4.2.0** (crates.io, updated 2026-08-29). Maintained now under the **open-source-cooperative/keyring-rs** org (moved from the original single-maintainer repo — healthy governance signal). ~30M downloads (one of the most-used crates in this space).

### Linux backend architecture (important recent change)
As of the v4 line, `keyring` was refactored around a **`keyring-core`** trait plus pluggable **backend crates** rather than one monolithic implementation. For Linux, the relevant backend crates (seen as optional deps of `keyring` 4.2.0) are:
- `dbus-secret-service-keyring-store` — **synchronous** Secret Service (GNOME Keyring / KDE KWallet's Secret Service compat layer) access via raw D-Bus, no async runtime required.
- `zbus-secret-service-keyring-store` — Secret Service access via the `zbus`/`secret-service` crate stack; has runtime+crypto feature variants (`rt-async-io-crypto-rust`, `rt-async-io-crypto-openssl`, `rt-tokio-crypto-rust`, `rt-tokio-crypto-openssl`) letting you pick async-std vs tokio and RustCrypto vs OpenSSL for the Secret Service session encryption.
- `linux-keyutils-keyring-store` — uses the kernel keyring (`keyctl`) directly; **not persistent across reboots and session-scoped**, so it's a poor fit for "remember my saved SSH credentials" — use it only as an ephemeral/in-session cache if at all.

### Pitfalls
- **v4 is a breaking change from v2/v3**: v2→v3 added a synchronous Secret Service path (via `dbus-secret-service`) so you were no longer forced onto an async runtime just to touch the keyring; v4 continues that direction but now requires choosing a backend crate explicitly (or relying on keyring's default feature selection) and requires either system `libdbus` to be installed, or building it vendored in.
- On a Tauri app (which already runs under tokio via the Tauri runtime), either `dbus-secret-service-keyring-store` (sync, in a `spawn_blocking`) or the tokio-flavored `zbus-secret-service-keyring-store` variant work; **recommend the sync dbus-based store** to avoid nested-runtime foot-guns, wrapping calls in `tokio::task::spawn_blocking`.
- **Headless/CI/minimal environments**: Secret Service requires a running D-Bus session bus and an actual keyring daemon (`gnome-keyring-daemon` or KWallet) — this is normally present on a real GNOME/KDE desktop session but **absent in most CI containers and bare SSH sessions**, so integration tests touching the real keyring will fail there; mock/stub the `keyring` calls in CI.
- Secret Service has practical **secret size limits** and the "collection" may be locked (user must unlock their keyring, e.g., after login with a different password than the OS login) — handle the "locked" error path in UI rather than treating every failure as "not found."

Sources: crates.io API for `keyring` ; https://docs.rs/keyring/latest/keyring/ ; https://github.com/open-source-cooperative/keyring-rs/wiki/Keyring ; search aggregation re: v4 `dbus-secret-service` / `secret-service` crate feature flags (rt-async-io-crypto-rust etc.)

---

## 6. xterm.js

### Latest version
- **`@xterm/xterm`: 6.0.0** (npm, published 2025-12-22). Note the scope change: the project moved from the unscoped `xterm` package to **`@xterm/xterm`** a while back; `xterm`/`xterm-addon-*` (unscoped) packages are deprecated — always use the `@xterm/*` scoped packages.
- 6.1.0 betas exist on npm (pre-release channel) but 6.0.0 is current stable.

### Addon status (all `@xterm/*`, versions as of npm registry, published 2025-12-22 batch)
| Addon | Latest version | Status |
|---|---|---|
| `@xterm/addon-fit` | 0.11.0 | Maintained, standard "fit terminal to container" addon |
| `@xterm/addon-webgl` | 0.19.0 | Maintained — GPU-accelerated WebGL2 renderer, now the **recommended default renderer** for performance |
| `@xterm/addon-canvas` | 0.7.0 (last published 2024-04-05 — stale relative to the others) | **Still published, not removed**, but it is **not listed among the actively maintained addons** in the xterm.js repo's own addons directory listing (only `addon-attach`, `addon-clipboard`, `addon-fit`, `addon-image`, `addon-ligatures`, `addon-progress`, `addon-search`, `addon-serialize`, `addon-unicode-graphemes`, `addon-unicode11`, `addon-web-fonts`, `addon-web-links`, `addon-webgl` currently live under `addons/` in the repo). Its documented role is purely as a **fallback for WebGL** when WebGL2 isn't available/performant — it was pulled out of core into an addon back in v5.0, and effectively treated as legacy/fallback-only since. **Do not rely on it as a primary renderer going forward**; treat the DOM renderer (xterm.js's built-in default, no addon needed) as the "safe" fallback instead of canvas if WebGL is unavailable. |
| `@xterm/addon-web-links` | 0.12.0 | Maintained, clickable URL detection |
| `@xterm/addon-search` | 0.16.0 | Maintained, in-terminal search |
| `@xterm/addon-unicode11` | 0.9.0 | Maintained, correct Unicode 11 character width handling (important for wide chars/emoji in terminal output) |

### React integration
No first-party React component from the xterm.js team. Community options:
- `xterm-for-react` — older wrapper, works via refs for anything not explicitly exposed.
- **`react-xtermjs`** — more modern option with a hooks-based API: `const { instance, ref } = useXTerm()`. This is the better-fit pattern for a modern React+TS codebase (functional components, hooks) vs. the older class-ref wrappers.
- Given xterm.js's imperative nature, many production apps just wrap it directly in a `useEffect`/`useRef` custom hook rather than pulling in a wrapper dependency at all — reasonable given how small the direct xterm.js API surface you need (constructor, `.open()`, `.write()`, `.onData()`, `.dispose()`, addon `.loadAddon()`) actually is.

### Known issues with WebKitGTK (relevant since Tauri on Linux = WebKitGTK)
- There is a **documented WebKit-specific rendering bug**: using transparency on the `<canvas>` element xterm.js's canvas/WebGL renderers draw onto causes **green artifacts covering text** in WebKit — reported upstream at `bugs.webkit.org` (bug #264268) and reproduced specifically by a Tauri app (Tauri uses WebKitGTK on Linux/macOS). This affects both the canvas renderer and WebGL1/2 contexts when transparency is enabled on the terminal's background. **Mitigation: don't use a transparent terminal background on Linux/WebKitGTK** (set an opaque background color on the xterm.js theme) to sidestep this entirely.
- More broadly, Tauri's own "Linux Graphics Issues" doc flags that on some Linux setups (notably NVIDIA + certain driver combos) WebKitGTK and the GPU driver disagree, producing anything from blank windows to WebGL2 silently falling onto a **slow software rasterizer path** — this would show up as high input latency / low FPS specifically in a WebGL-rendered terminal, without any visible error. **Recommendation:** ship with WebGL addon as default, but implement a runtime fallback to the DOM renderer (not canvas) if WebGL2 context creation fails or if you detect pathologically slow frame times, and expose a user-facing "renderer" setting (auto/webgl/dom) as an escape hatch for problem systems — this is a known enough class of WebKitGTK issue that Linux desktop apps built on Tauri should treat it as expected, not exceptional.

Sources: https://registry.npmjs.org/@xterm/xterm (and addon packages) ; https://github.com/xtermjs/xterm.js (addons directory listing + addon-webgl description) ; https://bugs.webkit.org/show_bug.cgi?id=264268 ; https://v2.tauri.app/develop/debug/linux-graphics/ ; https://lists.proxmox.com/pipermail/pve-devel/2023-October/059631.html (independent confirmation of WebGL2 software-fallback detection pattern used by other terminal-in-browser projects)

---

## 7. Local data storage for hosts/profiles

### Options considered
| Approach | Crate(s) | Latest version | Fit |
|---|---|---|---|
| Embedded SQL (sync) | `rusqlite` | **0.40.2** (crates.io, updated 2026-08-08) | Lightweight, synchronous, wraps `libsqlite3-sys`; very mature, huge download count (122M+), minimal async ceremony — good fit if you don't already need an async DB layer. |
| Embedded SQL (async) | `sqlx` (sqlite feature) | **0.9.0** (crates.io, updated 2026-05-21) | Async, compile-time checked queries, heavier dependency surface; this is what `tauri-plugin-sql` itself uses under the hood. |
| Flat file | JSON/TOML + `serde` | n/a (no single "latest version" — this is a pattern, not a crate) | Simplest to implement, human-editable/diffable, trivial to back up, but no real querying, no safe concurrent partial writes, and you have to hand-roll migrations. |

### Recommendation for this app
For a **single-user desktop SSH/SFTP client storing host profiles**, with an explicit "may later sync across devices" requirement:

**Use SQLite (via `rusqlite` or `tauri-plugin-sql`), not flat JSON/TOML, as the source of truth — even though the dataset is small.** Reasoning:
1. **Future sync requirement is the deciding factor.** Syncing a single shared JSON/TOML file across devices invites last-write-wins data loss/merge conflicts the moment two devices edit profiles independently (common with Dropbox/Syncthing-style file sync). A SQLite file has the same raw "it's one file" sync problem at the filesystem level, but it gives you a real schema and transactional integrity to build a **proper sync layer on top** later (e.g. row-level `updated_at`/`version` columns, soft-deletes, a changes/outbox table for a future sync protocol, or migrating to `sqlx` + a remote Postgres/Turso/libsql backend without a data-model rewrite). JSON gives you none of that scaffolding.
2. If you go through **`tauri-plugin-sql`** (2.5.0, current) rather than bare `rusqlite`, you get migrations, frontend-callable queries, and it's officially maintained alongside Tauri itself — reduces glue code and keeps the app aligned with the rest of the stack's upgrade cadence.
3. Secrets (passwords/passphrases) should **never** go in the SQLite file or JSON regardless of this choice — those belong in the OS keyring (`keyring` crate, section 5); the DB/file only stores host metadata, a reference/handle to the keyring entry, and non-secret connection settings.
4. If the dataset were truly tiny and static (e.g., a handful of hardcoded defaults) and sync were explicitly out of scope, JSON/TOML would be fine and simpler — but given the user's own stated "may later sync" intent, start with SQLite now rather than doing a storage-engine migration later.

Sources: crates.io API for `rusqlite` and `sqlx` ; crates.io API for `tauri-plugin-sql` (section 1 above)

---

## Summary of key risks / open items to track
1. **Canvas addon staleness** (`@xterm/addon-canvas` 0.7.0, April 2024) — fine as a fallback dependency, but don't expect upstream fixes; prefer DOM-renderer fallback over canvas-renderer fallback given the WebKitGTK green-artifact bug.
2. **`russh-sftp` is a single-maintainer crate** outside the main `warp-tech/russh` org — track its repo for responsiveness; pin exact versions.
3. **`russh-keys` (standalone crate) is stale since Jan 2025** — make sure any code/examples found while implementing don't accidentally pull in the old crate instead of `russh::keys`.
4. **keyring v4's backend-crate split** means you must explicitly pick/enable the right Linux feature (sync `dbus-secret-service-keyring-store` recommended) and test against both GNOME Keyring and KWallet's Secret Service shim; CI will need mocking since there's no D-Bus session there.
5. **WebKitGTK is the single biggest Linux-specific risk surface** in this stack — both the transparency/green-artifact canvas bug and the GPU-driver-dependent WebGL slow-path issue are real, reported, and specifically implicate Tauri-on-Linux + xterm.js. Build in a renderer fallback and an opaque terminal background from day one rather than treating these as edge cases.
6. Tauri 3.0 is in alpha (`3.0.0-alpha.4`) — do not target it yet; 2.x is the stable, supported line for now, but expect a migration path to matter within this app's lifetime.
