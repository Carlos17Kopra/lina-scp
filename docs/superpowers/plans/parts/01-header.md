# LinaSCP Teilprojekt 1 (Fundament + erster Durchstich) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first usable version of LinaSCP — a Linux-native SSH client with host management, `~/.ssh` integration, a secure connection pipeline (host keys, all auth methods, jump host), error screens, single-tab SSH terminals, local terminals, settings and encrypted export/import.

**Architecture:** Rust workspace with a GUI-free core crate (`crates/core`: store, secrets, known_hosts, ssh-config, export, sessions, connect pipeline on russh) and a thin Tauri 2 layer (`src-tauri`: commands + raw-byte Channels with ack-based flow control). React 19 + TypeScript UI (`ui/`) with zustand stores, CSS Modules on the design tokens, xterm.js (WebGL, DOM fallback).

**Tech Stack:** Rust (edition 2024), Tauri 2.12, tauri-specta/specta `=2.0.0-rc.25`, russh 0.64, rusqlite 0.40, keyring-core + dbus-secret-service-keyring-store, ssh2-config 0.8, age 0.11, portable-pty 0.9, tokio; React 19, Vite, zustand, @xterm/xterm 6, Vitest 4.1 + jsdom 29, bun.

**Spec:** `docs/superpowers/specs/2026-10-11-linascp-tp1-design.md` (+ `docs/superpowers/specs/reference/`: screen digest, tech research, spike report)

## Global Constraints

- App name "LinaSCP" everywhere; never "Shellport" (design-package working title). Identifier `de.mc-kg.linascp`.
- Code, comments, identifiers, commit messages: English. UI strings: German, verbatim from the mockups, centralized in `ui/src/i18n/de.ts`.
- Linux only (primary target Debian 13, GNOME Wayland, NVIDIA). Node 20.19 → Vitest 4.1.11 / jsdom 29.1.1.
- Data: SQLite at `~/.local/share/linascp/linascp.db`; logs `~/.local/state/linascp/logs/` (daily, 7 files); secrets only in the OS keyring (`service="linascp"`), never in DB, logs, events or export plaintext.
- Host keys only in `~/.ssh/known_hosts` (shared with OpenSSH), new entries hashed; decisions fail-closed.
- `~/.ssh` modes: "integrate" (default; read-only import, live-linked) or "mirror" (`~/.ssh/config.d/linascp.conf` + one `Include` line, backup `config.linascp.bak`). User's config file is never rewritten otherwise.
- Terminal: WebGL renderer by default, DOM fallback on context loss; opaque background; no `WEBKIT_DISABLE_*` unless "Kompatibilitäts-Rendering" is enabled. Output over `Channel<RawChunk>` (raw bytes), never `Channel<Vec<u8>>`; ack flow control (pause > 1 MiB, resume < 256 KiB).
- Files with secrets (export, imported keys, known_hosts, mirror) written with mode 0600, dirs 0700.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Git: local repository only (Task 1 runs `git init` if none exists); no remote, no push.

## Review Focus

Inputs/conditions the spec implies that the original task tests did not exercise; each now has a pinning test in the owning task:

1. **IPv6 hosts in known_hosts** (`[::1]:2222`, bare `fe80::1`) — must match OpenSSH bracket form, never split on `:` → Task 4, Step 5b.
2. **Real-world `~/.ssh/config` with `Include` and `Match` blocks** — import must not fail; concrete `Host` entries still appear → Task 5, Step 4b.
3. **Host names with spaces/umlauts in mirror mode** (e.g. „Kunde Müller Shop“) — valid `Host` alias, unicode kept → Task 6, Step 4b.
4. **Locked or unavailable GNOME keyring** — connection still works via prompt, saving degrades silently → Task 10, Step 4b.
5. **Split UTF-8 / binary terminal output** — bytes go to xterm unchanged (no per-chunk decoding) → already pinned in Task 22 (`term.write` receives `Uint8Array`; input bytes `0x80` pass through).

---

