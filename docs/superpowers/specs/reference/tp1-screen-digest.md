# Shellport — Requirements Digest for Sub-project 1 (Foundation + First Vertical Slice)

Source: `shellport-design/` mockup package (`.dc.html` design-tool format — inline-styled HTML with
`{{...}}` template bindings, `<sc-for>`/`<sc-if>` loops/conditionals, and a React-class-like
`Component` script at the end of each file defining `state`/`vals()`). Not runnable code — a
specification to rebuild natively in Tauri 2 + React + TypeScript.

Scope of this digest: app shell (rail), host overview (Main), host/login-profile editor (Edit),
connection pipeline (ConnectSSH + the three error/host-key screens), and the single-tab terminal
shell (skimmed from SSH.dc.html — full terminal feature set like split panes/snippets/tunnels is
explicitly out of scope for sub-project 1, only noted for shell consistency).

---

## 0. Global / Cross-Screen Conventions

### 0.1 Design tokens (from `tokens/tokens.css`, `tokens/tokens.json`)

- Fonts: UI = `IBM Plex Sans` (weights 400/500/600), Mono = `JetBrains Mono` (400/500). Base font
  size 14px on `.app`.
- Theming: CSS custom properties on `.app` root class; `.app.light` overrides. Dark is default.
  Selected theme persisted to `localStorage` key `"shellport-theme"` (`"dark"|"light"`). Toggle
  button at bottom of rail re-renders with `themeClass`.
- Radii: chip 6, input 8, button 10, card 12, dialog 16 (px).
- Sizes: rail 64px, nav item 44px, button heights [34,40,42,44]px, input height 40px (42 in error
  screens), sidebar [220,240]px, detail panel [300,360]px, tab-bar height 48px (44 in some error
  screens).
- Icons: inline SVG, 24 viewBox, stroke-width 1.8 (nav) or 2/2.2/2.4/2.6 (buttons/spinners/checks),
  round linecaps/joins — visually consistent with the Lucide icon set (see §7 for name mapping).
- Color semantics (critical for status mapping):
  - `--accent` (teal, `#4FD1C5` dark / `#0E7C72` light) = SSH / primary action / focus.
  - `--amber` (`#F2A65A` / `#A85A0C`) = SFTP / warning (e.g. unknown host key).
  - `--red` (`#F28B82` / `#B8322A`) = error, destructive action, changed host key.
  - `--green` (`#3DDC97` / `#12804F`) = connected / success / done.
  - `--violet`, `--blue`, `--pink` = free-choice host accent colors (no fixed semantic).
- Surfaces: `--bg` (app bg / terminal / lists), `--bg-deep` (behind modals), `--s1` (rail, sidebars,
  header/footer bars, inputs), `--s1b` (dialog footer), `--dlg` (dialogs), `--card` (cards, search
  fields), `--raised` (active nav items, chips, hover), `--hover`.
- Borders: `--bd`, `--bd-sub`, `--bd-card`, `--bd-strong`, `--bd-input`, `--bd-dash` (dashed "add"
  buttons), `--bd-hover`.
- Text: `--text` (primary), `--text2`, `--text3`, `--muted`, `--muted2` (placeholders/inactive
  steps), `--dim` (offline dot, separators).
- Accessibility requirement (explicit in README): use real `button`/`a`/`input`+`label` elements,
  `aria-label` on icon-only buttons, `role="tab"`/`"radio"`/`"dialog"`/`"alertdialog"`, contrast
  ≥ 4.5:1 in both themes. This is a hard requirement to carry into implementation, not just mockup
  flavor.

### 0.2 App shell / Rail (present on every screen except Edit modal)

Fixed-width vertical rail, `flex: 0 0 64px`, background `--s1`, right border `--bd`, padding
`14px 0`, vertical flex with `gap: 6px`, items centered.

Top-to-bottom:
1. **Logo/brand mark** — 36×36px, radius 10, bg `--accent`, mono `>_` glyph, `font-size:15px`,
   margin-bottom 14px. Not a link/button, purely a logo.
2. **Nav items**, each `<a>` 44×44px, radius 10, centered icon, `aria-label` set:
   - **Hosts** → `Main.dc.html`. Icon: two stacked rounded rects with small dot accents (server
     stack). Lucide-ish: `Server` / `Database` stack shape (custom, not a direct 1:1 Lucide icon —
     closest is two `Server` icons stacked, or custom SVG).
   - **Terminal** → `SSH.dc.html`. Icon: rounded rect frame with `>` and underscore-like line
     inside (terminal prompt). Lucide: `Terminal` / `SquareTerminal`.
   - **SFTP** → `SFTP.dc.html`. Icon: open folder/briefcase shape with up/down arrow swap inside.
     Lucide: `FolderSync` / `ArrowDownUp` combined — custom.
   - **Snippets** → `#` (placeholder, no screen in this sub-project; icon `Code` / chevron-brackets
     `<>` style, Lucide: `Code2`).
   - **Schlüsselbund** (keyring) → `#` (placeholder; icon: circle + key shape. Lucide: `KeyRound`).
   - **Port-Forwarding** → `#` (placeholder; icon: two opposing arrows. Lucide: `ArrowRightLeft` /
     `Shuffle`-like custom).
   - A spacer (`flex:1`) pushes the next items to the bottom.
   - **Einstellungen** (Settings) → `#` (placeholder; icon: gear. Lucide: `Settings`).
   - **Theme toggle button** (not a link) — `aria-label`/`title` = "Dunkles Design" or "Helles
     Design" (dynamic, shows the *opposite* mode's label — i.e. label names the action's result).
     Icon swaps between sun (`isLight` state showing sun-icon-for-switching-to-light is reversed —
     actually: `isDark` shows a **sun** icon (Lucide `Sun`), `isLight` shows a **moon** icon (Lucide
     `Moon`) — i.e. icon shown is the target mode's icon, not current mode's icon. Verify against
     mockup exactly: when `isDark` true → sun SVG (8-ray circle) renders; when `isLight` true →
     crescent-moon SVG renders. So the icon represents "switch to the other mode" semantics
     (sun invites switching to light... but it's shown when dark — i.e. icon = destination mode).

   **Note (highlighted, active route)**: active nav `<a>` gets `background: var(--raised); color:
   var(--accent)` (Hosts active on Main.dc.html; Terminal active on SSH/Connect/Error/HostKey
   screens — i.e. "Terminal" rail icon is highlighted even mid-connection and on all
   error/host-key dialogs, confirming these are rendered inside the Terminal section of the app
   shell). On `ErrorAuth.dc.html` the **SFTP** icon is active instead (since that error originated
   from an SFTP connection attempt) — confirms rail highlight should track originating
   protocol/section, not a global "terminal" state.

   Rail nav items with `href="#"` (Snippets, Schlüsselbund, Port-Forwarding, Einstellungen) have
   **no backing screen** in the design package — placeholders only (explicitly called out in
   README "Nicht im Umfang"). For sub-project 1 these can render as disabled/inert or route to a
   stub, but must not be omitted from the rail for layout fidelity.

### 0.3 Tab bar pattern (connection/terminal screens)

A horizontal strip above `<main>` content, `background: var(--s1)`, bottom border `--bd`,
`min-height: 44–48px`, `overflow-x:auto`. Each open session/tab is a `<span>` or `<button role="tab">`
with:
- a small 7×7px colored status dot (host accent color, or green/amber/red per state)
- host/session name
- (in SSH.dc.html only) a close `×` glyph and a `+` "Neue Sitzung" add-tab link
- selected tab gets `box-shadow: inset 0 2px 0 <color>` top-accent underline (not a bottom
  underline — distinctive detail) and `background: var(--bg)` vs `background: transparent` for
  inactive.

On connection/error screens the tab bar shows only one static tab reflecting current status, e.g.:
- ConnectSSH: `web-prod-01` with spinning icon, dot color `--violet`/`--accent`-ish (actually shown:
  bastion tab dot violet + web-prod-01 tab with spin icon, accent underline).
- HostKeyNew: `coolify-01 · wartet auf Bestätigung` (amber dot/underline).
- HostKeyChanged: `staging-app · blockiert` (red dot/underline).
- ErrorConnection: `nas · getrennt` (red dot/underline).
- ErrorAuth: `kunde-shop · nicht angemeldet` (red dot/underline, 44px bar height vs 48 elsewhere —
  minor inconsistency to normalize in implementation).

This establishes the **data model requirement**: a tab/session needs `name`, `statusDot color`,
`statusLabel suffix text`, `active boolean`, and must persist across navigating through the
connect→terminal pipeline (the same tab identity flows from ConnectSSH into SSH.dc.html).

---

## 1. Main.dc.html — Server-Übersicht (Host Overview)

### Layout structure
Full-bleed flex row, `min-height:900px`, `background:--bg`:
1. **Rail** (64px) — see §0.2.
2. **Sidebar** `<aside>`: `flex:1 1 220px; max-width:240px`, bg `--s1`, right border `--bd`,
   padding `20px 12px`, vertical flex gap 22px. Contains:
   - **Gruppen** (Groups) list — uppercase 11px label "Gruppen", then buttons per group (`Alle
     Hosts`, `Produktion`, `Infrastruktur`, `Staging`, `Kunden`, `Privat`) each showing name + mono
     count badge. Active group: `background:--raised; color:--text; font-weight:500`.
   - **Login-Profile** section — uppercase label + "+" add-link (`href="Edit.dc.html"`,
     `aria-label="Login-Profil hinzufügen"`), then a list of profile rows: 26×26 icon chip
     (key-shaped icon) + profile `name` (13px) + `method` subtitle (12px muted), e.g. "deploy /
     Schlüssel · ed25519". Each row links to `Edit.dc.html` (opens editor in Login-Profil mode —
     though note: the mockup link doesn't pass which profile; real app must pass profile id).
   - Spacer.
   - **Sync status card**: padding 12px, radius 10, bg `--card`, border `--bd`. Label "Sync",
     value "Verschlüsselter Tresor" (Encrypted Vault), status line "● Synchronisiert" in green.
     (README notes this sync/vault concept is only "angedeutet" / sketched — not a built feature;
     flag as **backend capability needed**: encrypted credential vault + sync status, scope TBD.)
3. **Main content** `<main flex:999 1 560px>`:
   - **Header bar**: padding `16px 24px`, bottom border, bg `--s1`, flex-wrap row:
     - **Host search field**: `flex:1 1 260px`, height 40, radius 10, bg `--card`, border
       `--bd-strong`. Search icon + `<input placeholder="Hosts, Tags oder IP durchsuchen">` + a
       kbd-hint chip showing **`⌘K`** (mono, 11px, bordered pill). Implies global keyboard shortcut
       ⌘K (Cmd+K / likely Ctrl+K on Windows/Linux) should focus/open this search. **Ambiguous**:
       mockup doesn't wire the actual keydown handler — behavior (focus vs. open command palette)
       must be decided; simplest conforming behavior is "focus the host search input and select
       its content."
     - **Quick Connect field**: `flex:1 1 240px`, placeholder `"Quick Connect: user@host:port"`,
       mono font. On Enter, should parse `user@host[:port]` and initiate a new ad-hoc connection
       (not wired in mockup — **ambiguous**: needs decision on default protocol (SSH), default auth
       (prompt?), and whether it creates a transient Host record or just a session).
     - **"Neuer Host" button** (primary): `<a href="Edit.dc.html">` with "+" icon, bg `--accent`,
       `on-accent` text, bold. Opens Edit modal in create/Host mode (mockup doesn't distinguish
       "new" vs. "edit existing" state in Edit.dc.html — see §2 ambiguity note).
   - **Host grid section** `flex:999 1 480px`, padding 24:
     - Title row: `<h1>{{groupTitle}}</h1>` (selected group name, e.g. "Alle Hosts") + muted count
       subtitle `{{countLabel}}` (e.g. "8 Hosts" / singular "1 Host").
     - **Protocol filter** segmented control (`role="group" aria-label="Protokoll filtern"`):
       3 buttons **Alle / SSH / SFTP** (`aria-pressed`), pill style, active = `bg:--bd-strong`.
       Filters the host grid by `h.ssh`/`h.sftp` boolean.
     - **Host card grid**: CSS grid `repeat(auto-fill, minmax(250px,1fr))`, gap 14px. Each **Host
       card** (`.card`, radius 12, bg `--card`, border `--bd-card` or `--accent` if selected):
       - Clickable button area (`all:unset`) containing:
         - 38×38 initials icon-chip (mono, host accent color) with 2-letter initials derived from
           name (`split('-').map(x=>x[0]).join('').slice(0,2).toUpperCase()` — e.g. "web-prod-01" →
           "WP").
         - Name (15px bold, truncated) + mono address line `user@host[:port if ≠22]` (12px muted,
           truncated).
         - Status dot (8px circle, color = green/amber/dim for on/warn/off) with `title` tooltip =
           status label.
         - Tag chips row: SSH badge (if `h.ssh`, teal bg/fg), SFTP badge (if `h.sftp`, amber
           bg/fg), then free-form tag chips (grey `--raised` bg, `--text3`).
       - Footer row (border-top `--bd`): "Last connected" text (`"Zuletzt " + relative-time`, e.g.
         "Zuletzt vor 5 Min.") + icon-button row: Terminal icon link (if ssh) → routes per scenario
         table, Files icon link (if sftp) → routes per scenario table, Edit (pencil) icon link →
         `Edit.dc.html` always present.
   - **Host Details panel** `<aside flex:1 1 300px; max-width:360px>`, left border, bg `--s1`,
     padding 24, gap 20 — shows the **selected** host (`sel`, default `web-prod-01`):
     - Big icon chip (52×52) + name (18px bold) + `"{{group}} · {{statusLabel}}"` subtitle.
     - Action buttons: **Terminal** (primary teal, if ssh) and **Dateien**/Files (amber-outline, if
       sftp) — 2-col grid, 44px height.
     - **Definition list** (`dl`, 110px label col): Adresse (`user@host`, mono), Port (mono),
       Login-Profil (name), Jump-Host (name or "—"), Startpfad (mono path), Fingerprint
       (`SHA256:[HOST-KEY]` placeholder — i.e. real known-host fingerprint should render here once
       connected/trusted).
     - **Port-Forwarding** list: uppercase label, then rows per forward rule: kind badge (`L`/`R`/
       `D`) + mono rule text (`"8080 → localhost:80"`); "Keine Regeln" (no rules) text if empty.
     - Spacer, then **"Host bearbeiten"** (Edit host) full-width outlined button → `Edit.dc.html`.

### Interactive elements & behavior
- Group buttons: click → `setState({group})`, re-filters host grid (client-side filter by exact
  group match or "Alle Hosts" = all).
- Protocol filter buttons: click → `setState({proto:'all'|'ssh'|'sftp'})`, filters by boolean flag.
- Host card click (the `all:unset` button) → `setState({sel: host.name})`, updates detail panel.
  Does **not** navigate — just changes selection/highlight (selected card gets `--accent` border).
- Icon-buttons on card footer (Terminal/Files/Edit) — real navigation links, **stop propagation**
  implied (otherwise clicking them would also trigger card selection; mockup doesn't handle this
  explicitly since it's declarative `<a>` inside `<button style="all:unset">` — actually the icons
  are siblings of the button, in the card's footer div, not nested inside the selectable button, so
  no propagation conflict).
- Search input ⌘K: see ambiguity above.
- Quick Connect input: see ambiguity above.
- Theme toggle: see §0.2.

### Data fields shown (maps to README data model)
Host: `name, user, host(address), port, group, tags[], ssh(bool), sftp(bool), status(on|warn|off),
last(relative time string), profile(display string "user · method"), jump(string or "—"),
path(startpath), color(css var), fw[] (forwards: {kind:'L'|'R'|'D', rule:string})`.//
LoginProfile (sidebar + detail reference): `name, method(display string)`.
Known-host fingerprint displayed as placeholder `SHA256:[HOST-KEY]` — real fingerprint must come
from the known_hosts store per host.

### Example/seed data (8 hosts) — for dev fixtures, not literal requirements
web-prod-01, db-prod-01, bastion, coolify-01, staging-app, kunde-shop, kunde-cms, nas — with groups
Produktion/Infrastruktur/Staging/Kunden/Privat, and a routing table used purely for the mockup demo
(`H.name → [sshTargetScreen, sftpTargetScreen]`) simulating different connection-outcome scenarios
(see §4 scenario table). This routing table is **demo wiring only**; in the real app the connect
pipeline's outcome is determined by actual connection results, not a lookup table.

### Ambiguities / backend capabilities needed
- "Verschlüsselter Tresor" (encrypted vault) + sync status — needs a real credential storage/
  sync backend (OS keychain? custom encrypted store + optional cloud sync?). Out of mockup scope,
  flagged as placeholder in README.
- ⌘K behavior unspecified (focus field vs. command palette).
- Quick Connect submit behavior unspecified (ad-hoc vs. persisted host, default protocol/auth).
- Host "last connected" (`last`) needs a persistence mechanism (timestamp per host, formatted as
  relative time — needs i18n/relative-time formatting in German, e.g. "vor 5 Min.", "gestern", "vor
  1 Woche", "vor 1 Monat").
- Status dot `on/warn/off` computation: is this a live reachability ping, last-known state, or just
  "has an open session now"? Not specified — needs a decision (e.g. periodic ICMP/TCP probe vs.
  session-derived).

---

## 2. Edit.dc.html — Host / Login-Profil bearbeiten (Modal)

### Layout structure
Centered modal dialog over a dimmed/blurred backdrop (decorative card grid at 0.18 opacity behind
it — purely cosmetic, representing "the host grid behind the modal"). `role="dialog"
aria-modal="true"`, max-width 860px, bg `--dlg`, border `--bd-strong`, radius 16, box-shadow.

- **Header** (padding 18×22, bottom border):
  - Head icon chip (44×44, radius 12, initials, color = host color or amber if Profile mode).
  - Title `<h2>` ("Host bearbeiten" / "Login-Profil bearbeiten") + subtitle (e.g.
    "deploy@10.0.1.12 · Produktion" or "deploy · verwendet von 5 Hosts" — "used by N hosts").
  - **Mode switch** segmented control (`role="group" aria-label="Was bearbeiten"`): **Host** /
    **Login-Profil** toggle buttons (`aria-pressed`).
  - Close button (`×` icon) → `href="Main.dc.html"` (i.e. acts as Cancel/close).
- **Body** (`min-height:520px`, flex row):
  - **Left vertical tab nav** (`role="tablist" aria-orientation="vertical"`, only shown when
    `isHost` mode — Login-Profil mode has no left tabs, single-pane form), 180–200px wide, bg
    `--s1b`: tabs **Allgemein / Authentifizierung / Verbindung / SFTP**. Active tab:
    `background:--raised; box-shadow: inset 2px 0 0 var(--accent)` (left-edge accent bar).
  - **Right content pane** (`flex:999 1 420px`, padding 22×26) — renders one of 5 panels depending
    on `mode`+`tab`: General, Auth, Connection, SFTP (host mode) or the single Login-Profile panel.
- **Footer** (padding 14×22, top border, bg `--s1b`):
  - **"Verbindung testen"** (Test Connection) button, with plug/connect icon, outlined style.
  - Test result message span, shown only after test runs: green text `"● Verbunden über bastion,
    Host-Key bekannt"` ("Connected via bastion, host key known").
  - Spacer.
  - **"Abbrechen"** (Cancel) link → `Main.dc.html`.
  - **"Speichern"** (Save) primary button → `Main.dc.html` (mockup doesn't actually persist
    anything — real app must validate + persist before navigating away).

### Mode switch behavior
`state.mode: 'host' | 'profile'`. Toggling re-renders header title/subtitle/icon and swaps the
entire body content (tabs+panels vs. single profile panel). Clicking "Profil bearbeiten" button
inside the Auth tab's "use profile" panel also switches to Profile mode (`goProfile` handler) —
i.e. **cross-navigation within the same modal** from Host→Authentifizierung→referenced profile.
**Ambiguity**: in real app this must carry the specific profile id being referenced, and "going
back" to the host should return you to the host's Auth tab, not reset — needs explicit navigation
stack/state design (mockup just flips a flat `mode` field with no "return to host" awareness).

### Host mode — Tab: Allgemein (General)
2-col grid, gap 16:
- **Anzeigename** (Display name) — text input, spans both cols. Default: `"web-prod-01"`.
- **Adresse (Host oder IP)** — mono text input. Default: `"10.0.1.12"`.
- **Port** — mono numeric input (`inputmode="numeric"`). Default: `"22"`.
- **Gruppe** (Group) — `<select>`: options `Produktion | Infrastruktur | Staging | Kunden |
  Privat`. (No "none"/custom-group option shown — **ambiguous**: can users create new groups?
  Main.dc.html's group list is derived from existing hosts, implying groups are freeform strings,
  not a fixed enum — the Edit screen's fixed `<select>` options are mockup-static; real
  implementation likely needs a combobox/creatable-select.)
- **Tags** — text input, default `"nginx, docker"` (comma-separated string — needs parsing to
  tags[] array).
- **Protokolle** (Protocols) fieldset with legend, 3 checkboxes: **SSH-Terminal** (checked),
  **SFTP** (checked), **SCP (Fallback)** (unchecked default).
- **Farbe** (Color) — `role="radiogroup"`, 6 color swatch radio-buttons (28px circles): Türkis
  (accent/default selected), Blau, Violett, Orange(amber), Rosa(pink), Grau(muted).
- **Notizen** (Notes) — textarea, height 72px, resizable vertical. Default sample text: "Deploy nur
  nach Freigabe. Wartungsfenster Di 22–23 Uhr." (placeholder/example copy, not a real default).

### Host mode — Tab: Authentifizierung (Auth)
- **Source radiogroup** (2-col grid of selectable cards): 
  - **"Login-Profil verwenden"** (Use login profile) — hint "Gemeinsame Zugangsdaten für mehrere
    Hosts" (shared credentials for multiple hosts). Default selected (`state.src='profile'`).
  - **"Individuell"** (Custom/individual) — hint "Nur für diesen Host" (only for this host).
  - Selected card gets `border: var(--accent)`.
- **If "Login-Profil verwenden"**:
  - **Login-Profil** `<select>`: options `"deploy · Schlüssel ed25519" | "root · SSH-Agent" |
    "ftp-kunde · Passwort"`.
  - Read-only summary card: `dl` grid — Benutzer (user) = "deploy", Methode = "Öffentlicher
    Schlüssel" (Public key), Schlüssel (key) = "id_ed25519_deploy" (mono), "Verwendet von" (used
    by) = "5 Hosts".
  - **"Profil bearbeiten"** (Edit profile) button → switches modal to Profile mode (`goProfile`).
- **If "Individuell"** (custom per-host credentials):
  - **Benutzer** (User) — mono text input, default "deploy".
  - **Methode** (Method) segmented control (4 options): **Passwort | Schlüssel | SSH-Agent |
    Interaktiv / 2FA** (`state.method`, default `'key'`).
  - **If Passwort**: Passwort password-input (default masked value shown — mockup artifact, real
    default should be empty), checkbox "Im verschlüsselten Tresor speichern" (store in encrypted
    vault, checked default).
  - **If Schlüssel (Key)**:
    - **Privater Schlüssel** (Private key) `<select>`: options `id_ed25519_deploy | id_rsa_legacy |
      "Datei wählen …"` (file picker option — needs native file dialog integration).
    - **"Neu erzeugen"** (Generate new) button next to the select — implies key-generation
      capability (**backend capability needed**: generate ed25519/RSA keypair).
    - **Passphrase** — password input, placeholder "Optional".
    - Checkbox "Agent-Forwarding erlauben" (Allow agent forwarding), checked default.
  - **If SSH-Agent**: static info text "Schlüssel werden vom laufenden SSH-Agent bezogen
    (SSH_AUTH_SOCK). 2 Identitäten gefunden." (keys come from the running SSH agent via
    SSH_AUTH_SOCK; "2 identities found" — **backend capability needed**: enumerate agent
    identities live).
  - **If Interaktiv / 2FA** (keyboard-interactive): static info text "Der Server fragt beim
    Verbinden nach Eingaben, etwa Passwort und Einmalcode (2FA)." (server will prompt for input at
    connect time, e.g. password + OTP). **Implies the connect pipeline (ConnectSSH) must support a
    runtime prompt step** not shown in the mockup's fixed 5-step list — flag as gap between Edit's
    stated method and ConnectSSH's step list (see §4 ambiguity).

### Host mode — Tab: Verbindung (Connection)
2-col grid:
- **Jump-Host (ProxyJump)** `<select>`: options `bastion | Keiner` (None). Default "bastion".
- **Keepalive-Intervall** — mono text input, default `"30 s"` (free text w/ unit, not a numeric
  stepper — needs parsing/validation of duration strings).
- **Verbindungs-Timeout** — mono text input, default `"15 s"`.
- **Terminal-Typ** (TERM) — mono text input, default `"xterm-256color"`.
- **Startbefehl** (Start command) — spans both cols, mono input, placeholder `"z. B. tmux attach ||
  tmux"`.
- **Umgebungsvariablen** (Env vars) — spans both cols, mono input, default `"LANG=de_DE.UTF-8"`
  (single-line string — needs parsing into KEY=VALUE pairs, likely multiple vars separated by
  something not shown; **ambiguous** delimiter for multiple env vars).
- Checkboxes row: **Kompression** (Compression, checked default), **X11-Forwarding** (unchecked),
  **Sitzung protokollieren** (Session logging, unchecked).
- **Port-Forwarding** list: one example rule shown — badge "Lokal" (Local) + mono rule
  `"127.0.0.1:8080 → localhost:80"` + checkbox "Auto-Start" (checked). Then **"+ Regel
  hinzufügen"** (Add rule) dashed-border button (no add-rule dialog/form shown in this package —
  **backend/UI capability needed**: full port-forward rule editor with type L/R/D, bind addr, bind
  port, target host, target port, autostart flag — only the list display and the data shape
  `{type:'L'|'R'|'D', bind, target, autostart}` are given in README's data model).

### Host mode — Tab: SFTP
2-col grid:
- **Remote-Startverzeichnis** (Remote start dir) — mono input, default `"/var/www"`.
- **Lokales Startverzeichnis** (Local start dir) — mono input, default `"~/Projekte/web"`.
- **Übertragungsmodus** (Transfer mode) `<select>`: `Automatisch | Binär | Text`.
- **Parallele Übertragungen** (Parallel transfers) — mono input, default `"3"` (numeric but typed
  as text field, no stepper).
- **Rechte beim Upload** (Upload perms) — mono input, default `"0644 / Ordner 0755"` (combined
  file+folder perms string — needs parsing into two octal values).
- **Bandbreitenlimit** (Bandwidth limit) — mono input, placeholder `"Unbegrenzt"` (Unlimited).
- **SFTP-Server** (spans both cols) — mono input, default `"sudo /usr/lib/openssh/sftp-server"`
  (custom sftp-server command, e.g. to run via sudo).
- Checkboxes: **Zeitstempel erhalten** (Preserve timestamps, checked), **Sync-Browsing** (checked),
  **Versteckte Dateien zeigen** (Show hidden files, unchecked), **Teilübertragungen fortsetzen**
  (Resume partial transfers, checked).

### Login-Profil mode (when mode === 'profile')
- Info banner: "Login-Profile bündeln Benutzer und Anmeldedaten. Änderungen gelten für alle 5
  Hosts, die dieses Profil nutzen." (Login profiles bundle user + credentials; changes apply to all
  N hosts using this profile — **N must be computed live from usage, "5" is mockup sample data**.)
- 2-col grid: **Profilname** (Profile name) text input default "deploy"; **Benutzer** (User) mono
  input default "deploy".
- **Methode** (Method) segmented control — same 4 options as host-custom auth: Passwort | Schlüssel
  | SSH-Agent | Interaktiv / 2FA.
- **If Passwort**: Passwort password input (masked default shown — again should be empty in real
  app unless editing existing non-empty stored secret, in which case a "show/change" affordance is
  likely needed — **ambiguous**: mockup always shows a pre-filled dot-mask, real app must
  distinguish "no password set" vs "password set but hidden").
- **If Schlüssel**:
  - **Privater Schlüssel** `<select>`: same 3 options (`id_ed25519_deploy | id_rsa_legacy | "Datei
    wählen …"`).
  - **Öffentlicher Schlüssel** (Public key) — mono readonly input showing
    `"ssh-ed25519 AAAAC3Nz… deploy@shellport"`.
  - **"Public Key kopieren"** (Copy public key) button.
  - **"Auf Hosts verteilen (ssh-copy-id)"** (Distribute to hosts via ssh-copy-id) button —
    **backend capability needed**: implement ssh-copy-id-equivalent (append pubkey to remote
    authorized_keys over an existing/ad-hoc connection), plus a host-picker UI (not shown — the
    mockup just has the button, no dialog).
- Checkbox: **"Anmeldedaten im verschlüsselten Tresor speichern und synchronisieren"** (Store and
  sync credentials in encrypted vault), checked default — same vault dependency as §1.

### "Verbindung testen" (Test connection) behavior
Click → `setState({tested:true})` synchronously (no real async simulation in mockup) → shows green
inline message `"● Verbunden über bastion, Host-Key bekannt"`. **Real implementation**: must
actually attempt a connection using the current (possibly unsaved) form values, show
loading/success/failure states, and surface host-key/auth errors inline (could reuse the
ConnectSSH step list or error screens in miniature — not specified, **ambiguous** design for
failure case within the modal).

### Validation — NOT specified in mockup
No field has visible required/invalid states, no inline validation shown anywhere in Edit.dc.html
(contrast with ErrorAuth's password empty-validation, which IS specified — see §6). **Flag as gap**:
sub-project 1 must define its own validation rules for: required Anzeigename/Adresse, port range
1–65535, numeric parallel-transfers, duration parsing for keepalive/timeout, tag/env parsing, etc.

### Reusable component inventory introduced here
- `.f` input/select/textarea style class (40px height, radius 8, bg `--s1`, border `--bd-input`,
  focus → `--accent` border).
- `.lbl` label wrapper (column flex, 12px `--text3` caption above field).
- `.chk` checkbox row (16px native checkbox, `accent-color:--accent`, 32px min-height touch target).
- Segmented-control button group pattern (`seg()` helper) reused for: Host/Profil mode switch, tab
  underline-list is separate style, protocol filter (Main), and Methode radio group (Auth) — 3+
  distinct instances of the same visual pattern → should be one `<SegmentedControl>` component.
  **Note**: `role="group" aria-pressed"` is used when it's a simple toggle (Host/Profil, protocol
  filter), while `role="radiogroup" aria-checked` is used for Methode — inconsistent roles for what
  is visually the same control; pick one correct ARIA pattern (likely `radiogroup`/`radio` is more
  correct when only one can be selected and it's not literally a pressed-state toggle).
- Color-swatch radio button (circular swatch, ring when selected).
- Card-style selectable option (Source radiogroup: profile vs custom) — bordered card with label +
  hint text, selected state = accent border.
- Dashed "add new" button pattern (`+ Regel hinzufügen`, also seen as `+ Snippet anlegen` in
  SSH.dc.html) — reusable `<AddButton>` style (dashed border, muted text, no fill).

---

## 3. ConnectSSH.dc.html — Laden: SSH (Connection Loading Screen)

### Layout structure
App shell with rail (Terminal icon active) + `<main>`. Tab bar shows two tabs: a static `bastion`
tab (violet dot, used as jump-host context) and the active `web-prod-01` tab with a spinning
loader icon and accent underline.

Centered content column (max-width 560px, padding 40×16):
1. **Header row**: 60×60 icon with an animated **pulse ring** (`@keyframes pulse`: scale 1→1.7,
   opacity 0.55→0, 1.6s loop) around a 16-radius icon chip showing host initials ("WP"). Next to
   it: `<h1>{{headline}}</h1>` (dynamic: `"Verbinde mit web-prod-01 …"` while connecting, becomes
   `"Verbunden"` when finished) + mono subtitle `"deploy@10.0.1.12:22 · über bastion"` (static
   context line: user@host:port · via jumphost).
2. **Progress bar**: `role="progressbar"` 4px thin bar, `--raised` track, `--accent` fill, width =
   `pct%` with a CSS transition (`width .4s ease`). `pct = step/totalSteps*100`.
3. **Step list** (`<ol>`, 5 fixed steps, each a row with icon/state + label + meta text):
   1. **"Jump-Host bastion verbinden"** — meta once reached: `"admin@bastion:2222"`.
   2. **"Host-Key prüfen"** (Check host key) — meta: `"ED25519 · bekannt"` (known).
   3. **"Anmelden"** (Log in / Authenticate) — meta: `"Profil deploy · ed25519"`.
   4. **"Kanal öffnen"** (Open channel) — meta: `"session"`.
   5. **"Shell starten"** (Start shell) — meta: `"xterm-256color · 140×38"` (terminal type ×
      dimensions).
   Each step row has 3 possible icon states: **done** (green checkmark SVG), **active** (spinning
   accent ring SVG, `bg:--accent-soft` row highlight), **pending** (small grey dot, label dimmed to
   `--muted2`). Meta text only shows once a step is reached (`i <= currentStep`).
4. **Collapsible raw log**: toggle button `"▸ Details anzeigen"` / `"▾ Details ausblenden"` (Show/
   hide details), `aria-expanded`. When expanded, shows a `<pre>` log box (mono, 12px, max-height
   180px, scrollable) with simulated `ssh -v`-style debug lines, one appended per completed step,
   e.g.:
   - `OpenSSH_9.8, LibreSSL 3.3.6` (always first line)
   - `debug1: Connecting to bastion.example.net port 2222.`
   - `debug1: Host '10.0.1.12' is known and matches the ED25519 host key.`
   - `debug1: Authentication succeeded (publickey).`
   - `debug1: channel 0: new [client-session]`
   - `debug1: Requesting pty, starting shell.`
   **Real implementation**: this should stream actual `ssh -v`-equivalent diagnostic output from
   the underlying SSH client library, not canned text.
5. **Action row**:
   - **"Abbrechen"** (Cancel) outlined button → `Main.dc.html`.
   - **"Terminal öffnen"** (Open terminal) primary button — only rendered once `finished` (all 5
     steps done); auto-clicked via a `ref` 700ms after completion if `autoOpen` prop is true
     (default true) — i.e. **auto-navigates to the terminal screen on success after a short delay**,
     but a manual button is also available immediately. → `SSH.dc.html`.
   - **"Erneut abspielen"** (Replay) ghost button — resets `step=0` and restarts the simulated
     timer. In the real app this likely has no equivalent (it's a mockup/demo affordance) unless
     repurposed as "Retry connection".

### State machine / timing
`state.step` increments every `ms` interval (prop `speed`: langsam=1400ms / normal=800ms /
schnell=400ms, default normal) via `setInterval`, from 0 to 5. At step ≥ 5: interval cleared; if
`autoOpen` (default true), a 700ms `setTimeout` auto-clicks the "Terminal öffnen" link.
`componentWillUnmount` clears both timers (interval + timeout) — important pattern: real
implementation needs equivalent cleanup to avoid leaks/races if the user cancels mid-connect.

### Step list — generalization for real implementation
The 5-step list (Jump-Host → Host-Key → Anmelden → Kanal → Shell) is the **SSH connection
pipeline**; the ConnectSFTP screen (not required to read in depth per task scope, but referenced in
README) uses an analogous list: **TCP, Host-Key, Anmelden, SFTP-Subsystem, Verzeichnis laden**
(per README table). This implies the connect-progress UI is a **generic step-list component**
parameterized by protocol, reusable for SSH and SFTP pipelines alike.
**Ambiguous**: the step "Jump-Host bastion verbinden" only applies when a jump host is configured;
mockup doesn't show the no-jump-host variant (steps presumably renumber/skip to 4 steps). Also,
keyboard-interactive/2FA auth (configured in Edit) would require an additional runtime
prompt/modal during the "Anmelden" step not represented here — **flag as gap**.

### Data needed
Session/tab context: `hostName, user, address, port, jumpHostName, authMethod/profile,
terminalType, dimensions`. Must feed into and persist through to the terminal tab opened at the end.

---

## 4. Host-Key Dialogs

### 4.1 HostKeyNew.dc.html — Unbekannter Host-Key (Unknown Host Key)

Layout: app shell, tab bar shows single tab `"coolify-01 · wartet auf Bestätigung"` (waiting for
confirmation), amber dot + amber top-underline. Centered dialog card (not `role="dialog"` here,
just `<section>`, max-width 620px, bg `--dlg`, border `--bd-strong`):

- Icon (48×48, amber-soft bg, amber fg) — shield-with-exclamation icon (Lucide: `ShieldAlert`).
- `<h1>` **"Unbekannter Host"** (Unknown host).
- Body text: *"Du verbindest dich zum ersten Mal mit `coolify.example.net:22`. Vergleiche den
  Fingerprint mit dem Wert, den dir der Server-Betreiber auf einem anderen Weg genannt hat."*
  (You're connecting for the first time to X. Compare the fingerprint with the value the server
  operator gave you through another channel.)
- **Fingerprint card** (bg `--s1`, border `--bd`, radius 12):
  - Caption `"ED25519 · SHA256"` + **"Kopieren"/"Kopiert"** (Copy/Copied) toggle-label copy button.
  - `<code>` mono 14px fingerprint: `SHA256:Xq7Lm2Pd9vKc4RtB0nWj8sYfE1uHa6QzGo3iTlNe5Vk` (example).
  - Format toggle pills: **SHA256** / **MD5** (`aria-pressed`), switching the displayed hash
    format; when MD5 selected, an additional mono MD5 fingerprint line renders:
    `MD5:3f:9a:c1:7e:42:0b:d8:55:6c:e2:19:af:73:04:b6:8d`.
- Checkbox (checked default): **"Für alle Hosts dieser Gruppe mit gleiche Adresse merken"** (Trust
  for all hosts in this group with the same address) — implies known-host trust can be scoped to
  more than one logical Host entry sharing a physical address/port — **ambiguous data model
  question**: does "remember" write to a global known_hosts-equivalent keyed by address:port (SSH
  standard behavior), or per-Host-record? The checkbox phrasing suggests the app's Host records are
  the unit of trust by default, with this checkbox opting into broader (address-based) trust.
- Footer: **"Abbrechen"** (Cancel, text-only) → `Main.dc.html`; spacer; **"Nur dieses Mal"** (Just
  this once, outlined) → `ConnectSSH.dc.html`; **"Vertrauen und verbinden"** (Trust and connect,
  primary accent) → `ConnectSSH.dc.html`.
  - **"Nur dieses Mal"**: connect without persisting the host key to any trust store (one-time
    accept).
  - **"Vertrauen und verbinden"**: persist to known-hosts (scoped per the checkbox above) and
    connect.

### 4.2 HostKeyChanged.dc.html — Host-Key geändert (Changed Host Key — Warning)

Layout: tab bar single tab `"staging-app · blockiert"` (blocked), red dot + red underline. Centered
`<section role="alertdialog">`, max-width 660px, **red-bordered** (`border:--red-bd`, distinct from
the neutral-bordered HostKeyNew card — correctly escalates severity).

- Icon (48×48, red-soft bg, red fg) — warning triangle (Lucide: `TriangleAlert`/`AlertTriangle`).
- `<h1 style="color:--red-text">` **"Host-Schlüssel hat sich geändert"** (Host key has changed).
- Body: *"Der Server `staging.example.net` meldet einen anderen Schlüssel als gespeichert. Das
  passiert nach einer Neuinstallation, kann aber auch ein Angriff (Man-in-the-Middle) sein. Die
  Verbindung wurde deshalb gestoppt."* (Server reports a different key than stored. This happens
  after reinstalls but could also be a MITM attack. Connection was stopped as a result.)
- **Two-column comparison** (`grid repeat(2,1fr)`):
  - Left (neutral `--s1`/`--bd`): caption `"Gespeichert · ED25519 · seit 14.03.2026"` (Stored ·
    since DATE) + mono fingerprint `SHA256:b4Kd9QwPz1TnR7cLx2mYv8EhJ0sUa3FgN6oWiZkV5qM`.
  - Right (red `--red-bg2`/`--red-bd`): caption `"Aktuell · ED25519"` (Current) in red + mono
    fingerprint `SHA256:Hn2Wc7Ry0LqTd5PkB9xVm4ZsJ1fGa8EuK3oNiQ6tYwc` in normal text color.
- **`<details>`** "Technische Details" (Technical details) — collapsed by default, expands to show
  the **verbatim classic OpenSSH warning banner** (must be reproduced exactly as literal text):
  ```
  @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
  @    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @
  @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
  Offending ED25519 key in known_hosts:42
  Host key verification failed.
  ```
  This is meant to look like a real ssh client error dump, suggesting the real implementation
  should actually surface the real libssh2/russh/ssh2-crate (whichever Rust SSH backend is chosen)
  verification error text here, formatted similarly.
- **Confirmation checkbox** (unchecked default, `onChange` toggles `state.ok`): *"Ich habe den
  neuen Fingerprint mit dem Betreiber abgeglichen und möchte den gespeicherten Schlüssel
  ersetzen."* (I have verified the new fingerprint with the operator and want to replace the
  stored key.)
- **Footer** — primary destructive action is **gated** by the checkbox:
  - If `!confirmed`: a **disabled** button *"Schlüssel ersetzen und verbinden"* (Replace key and
    connect), `cursor:not-allowed`, muted colors.
  - If `confirmed`: the same label becomes an **enabled red-outlined link** → `ConnectSSH.dc.html`.
  - Spacer, then **"Host bearbeiten"** (Edit host, text link) → `Edit.dc.html`.
  - **Primary/rightmost action: "Verbindung abbrechen"** (Cancel/abort connection) — styled as the
    visually dominant button (`bg:--text; color:--bg` — i.e. uses the *inverted neutral* color, NOT
    red or accent, making it visually the strongest button on the screen despite being the "safe"
    cancel action) → `Main.dc.html`. **This confirms the task's framing**: the safe default
    ("abort") is the visually primary action, while "replace and connect" is deliberately
    secondary/outlined and gated — a deliberate anti-footgun pattern to preserve for real
    implementation (do not swap button prominence).

### Data model implication
Known-hosts store needs: `{address, port, keyType, finger

print (sha256+md5), firstSeenDate}`
per stored entry, and the changed-key flow needs to diff stored vs. presented key, with an explicit
**replace** operation that overwrites the stored entry only after explicit confirmation.

---

## 5. ErrorConnection.dc.html — Verbindungsfehler (Connection Error)

### Layout structure
Tab bar: `"nas · getrennt"` (disconnected), red dot/underline. Centered column, max-width 600px, no
card chrome (unlike Host-Key dialogs — this is a plain page section, not a boxed dialog).

- Icon (52×52, red-soft/red): either a spinning retry-circle icon (`busy` state) or a "wifi-off"
  style icon (`idle` state — concentric arcs + diagonal strike-through + dot, i.e. "no signal").
  Lucide equivalent: `WifiOff`.
- `<h1>{{title}}</h1>` — dynamic: shows `"Neuer Versuch …"` (Retrying…) while `busy`, else the
  variant title.
- Mono subtitle: `"admin@192.168.1.10:22 · {{attemptLabel}}"` where `attemptLabel = "Versuch N von
  3"` (Attempt N of 3).
- Body paragraph `{{message}}` — variant-specific (see table below).
- **"Mögliche Ursachen"** (Possible causes) — uppercase label + bullet list (`–` dash marker, not a
  real `<li>` bullet), 2–3 items per variant.
- **Raw error `<pre>`** — mono 12px, red-text color, shows `{{detail}}` — the literal
  client-error-style string (see table).
- Action row:
  - **"Erneut versuchen"** (Retry) primary button — disabled while `busy`; label becomes
    `"Verbinde …"` (Connecting…) while busy.
  - **"Host bearbeiten"** (Edit host) outlined button → `Edit.dc.html`.
  - **"Zur Übersicht"** (Back to overview) text link → `Main.dc.html`.
  - Spacer, then checkbox **"Automatisch erneut versuchen"** (Auto-retry) — unchecked default, no
    visible auto-retry countdown/logic wired in this screen (contrast with SSH.dc.html's "lost
    connection" panel which DOES show a live countdown — see §5.1 note below). **Ambiguous**: is
    auto-retry meant to behave like the SSH tab's reconnect countdown (auto-trigs retry after N
    seconds, pausable)? The checkbox exists but its behavior isn't implemented in this specific
    mockup file — must design: likely reuse the same countdown pattern from `SSH.dc.html`'s
    connection-lost panel (`"Automatischer neuer Versuch in {{cd}} s"` / pause-resume toggle).

### Three variants (`props.variant`: `timeout | refused | dns`, default `timeout`)

| Variant | Title | Message | Causes | Raw detail |
|---|---|---|---|---|
| `timeout` | Zeitüberschreitung | "Der Server hat innerhalb von 15 Sekunden nicht geantwortet. Er ist ausgeschaltet, nicht im Netz oder eine Firewall verwirft die Pakete." | Gerät ist aus oder im Ruhezustand · Du bist nicht im selben Netz bzw. VPN ist nicht verbunden · Firewall blockiert Port 22 | `ssh: connect to host 192.168.1.10 port 22: Operation timed out` |
| `refused` | Verbindung abgelehnt | "Der Server ist erreichbar, aber auf Port 22 lauscht kein SSH-Dienst." | sshd ist gestoppt · SSH läuft auf einem anderen Port · fail2ban hat deine IP gesperrt | `ssh: connect to host 192.168.1.10 port 22: Connection refused` |
| `dns` | Host nicht gefunden | "Der Hostname konnte nicht aufgelöst werden." | Tippfehler in der Adresse · DNS-Eintrag fehlt oder ist noch nicht propagiert · Interner Name, aber kein VPN verbunden | `ssh: Could not resolve hostname nas.local: nodename nor servname provided, or not known` |

(German texts above are verbatim and must be reproduced exactly; the "raw detail" lines mimic real
OpenSSH client stderr output — the real app should ideally substitute the actual error text from
its SSH backend here rather than hardcoding these strings, but the **format and tone** — literal
client-style one-liner — should be preserved.)

### Retry behavior / state machine
`state = {busy:false, attempt:1}`. Clicking Retry: `busy=true`; after an **1800ms** `setTimeout`
(mockup's artificial delay standing in for a real reconnect attempt), `busy=false` and
`attempt = min(3, attempt+1)`. **No real cap enforcement beyond display** — i.e. mockup lets you
click Retry indefinitely even past "Versuch 3 von 3" (the label caps visually at 3 but the button
isn't disabled after 3 attempts). **Ambiguous/gap**: should retry become disabled or switch
behavior (e.g. suggest giving up, or lengthen backoff) after reaching max attempts? Not specified —
needs a product decision; suggest implementing exponential backoff and/or disabling further
manual retries beyond a configurable max, while always allowing "Host bearbeiten"/"Zur Übersicht"
as escape hatches.
`componentWillUnmount` clears the timeout — same cleanup discipline as ConnectSSH.

### Relation to SSH.dc.html's "connection lost" panel
Not one of the required-reading files in depth, but directly relevant: `SSH.dc.html` has a built-in
"Verbindung zu staging-app verloren" (Connection to X lost) panel shown when `tab===2` (the
'staging-app' demo tab), with: icon (plug-disconnected), title, meta `"Nach 2 Std. 14 Min. ·
Connection reset by peer"` (duration since loss · underlying reason), explanatory paragraph *"Der
Verlauf dieser Sitzung bleibt erhalten. Laufende Prozesse ohne tmux oder screen wurden auf dem
Server beendet."* (session history is preserved; processes without tmux/screen were killed
server-side), a **live auto-reconnect countdown** (`"Automatischer neuer Versuch in {{cd}} s"`,
10s timer, `setInterval`, auto-click reconnect link at 0, pause/resume toggle button labeled
"Pausieren"/"Fortsetzen"), and two actions: **"Jetzt neu verbinden"** (Reconnect now, primary) →
`ConnectSSH.dc.html`, and **"Tab schließen"** (Close tab, outlined). This is the **mid-session**
disconnect UX (while a terminal tab exists) as opposed to ErrorConnection.dc.html which is the
**initial-connect-failed** UX (before any session/tab exists) — two distinct flows that should
share the reconnect-countdown component but are visually/contextually different entry points. This
confirms sub-project 1 needs **both**: (a) ErrorConnection as its own route/state for failed
initial connection attempts, and (b) an in-terminal-tab lost-connection overlay state, reusing a
shared `<ReconnectCountdown>` component.

---

## 6. ErrorAuth.dc.html — Anmeldung fehlgeschlagen (Authentication Failed)

### Layout structure
Tab bar: `"kunde-shop · nicht angemeldet"` (not logged in), red dot/underline, and notably the
**SFTP rail icon is highlighted** (not Terminal) — confirms this error can originate from an SFTP
connection attempt too, not just SSH. Centered dialog card, max-width 560px, bg `--dlg`, neutral
border (`--bd-strong`, not red — less severe chrome than HostKeyChanged despite also being an
error, consistent with "retry is easy" vs. "security risk" severity grading).

- Icon (48×48, red-soft/red) — padlock-with-open-shackle-ish shape (rect body + arc, suggesting a
  "locked out" / credentials rejected glyph). Lucide: `Lock` / `ShieldX`.
- `<h1>` **"Anmeldung fehlgeschlagen"** (Login failed).
- Body: *"Der Server `sftp.kunde-a.example` hat die Zugangsdaten aus dem Profil »ftp-kunde«
  abgelehnt."* (Server rejected credentials from profile "ftp-kunde".)
- **Rejected-methods list** (bg `--s1`, bordered, 13px):
  - Row: `publickey` ................ `kein passender Schlüssel` (no matching key) — muted text.
  - Row: `password` ................ `abgelehnt · Versuch 1 von 3` (rejected · attempt 1 of 3) —
    red text.
  This models SSH's real multi-method auth negotiation (tries publickey first, falls back to
  password) — **data requirement**: the connect pipeline must report, per attempted auth method,
  a status (`not-offered | no-matching-key | rejected | succeeded`) and an attempt counter when
  retries occur (password method specifically retries up to 3 times per the label — consistent
  with typical SSH server `MaxAuthTries` default behavior, though the real max is server-defined
  and should be read from the actual negotiation, not hardcoded to 3).
- **Re-entry form**:
  - **Benutzer** (User) — text input, prefilled `"shop"` (read-only intent implied by context,
    though not marked `readonly` in markup — **ambiguous**: should username be editable here, or
    only the password? Given the error is specifically about credentials/password, likely username
    should stay editable since kbd-interactive/password auth doesn't require matching a fixed
    profile user, but this isn't explicit).
  - **Passwort** — password input, placeholder "Passwort eingeben" (Enter password), `onInput`
    tracked, `aria-invalid`/`aria-describedby="pw-err"` wired to a validation message.
  - **Inline validation**: if the user submits with an **empty** password (via the fallback
    "submit empty" button path), shows red text **"Bitte ein Passwort eingeben."** (Please enter a
    password.) below the field, and sets `aria-invalid="true"`. This is the one fully-specified
    client-side validation example in the whole package — **use as the canonical pattern** for
    required-field validation elsewhere (e.g. Edit.dc.html fields that currently have none).
  - Checkbox (checked default): **"Neues Passwort im Profil »ftp-kunde« speichern"** (Save new
    password in profile "ftp-kunde") — on success, should update the stored LoginProfile's
    credential (vault write).
- **Footer**:
  - **"Abbrechen"** (Cancel, text) → `Main.dc.html`.
  - **"Login-Profil bearbeiten"** (Edit login profile, outlined) → `Edit.dc.html` (should deep-link
    to Profile mode for the specific profile, e.g. "ftp-kunde" — same cross-navigation concern as
    §2).
  - Spacer, then primary **"Erneut anmelden"** (Retry login):
    - If password field has content (`hasPw`): renders as an `<a href="ConnectSFTP.dc.html">`
      (i.e. navigates directly to the SFTP connect-loading screen with the new password).
    - If empty (`noPw`): renders as a `<button onClick>` that just triggers the empty-password
      validation message (`submitEmpty`) instead of navigating — i.e. **client-side guard against
      submitting with no password**, consistent with the inline validation above.

### Data model implication
Auth attempt result object needed: `{method: 'publickey'|'password'|'keyboard-interactive',
status, attemptsUsed, attemptsMax}[]` per connection attempt, surfaced to this screen. Retrying
should re-attempt with the edited credentials and route back into the connect pipeline
(`ConnectSFTP`/`ConnectSSH` as appropriate) rather than re-showing this screen directly — it's the
connect pipeline's job to loop back here again on repeated auth failure.

---

## 7. Reusable UI Components (cross-screen inventory)

| Component | Where seen | Notes / Lucide-equivalent icons |
|---|---|---|
| **Rail nav** (icon button, 44×44, active-highlight) | all screens | See §0.2 for icon list |
| **Theme toggle button** | all screens | `Sun` / `Moon` |
| **Tab bar / session tab** (dot + label + optional close ×, accent-colored top underline for active) | SSH, ConnectSSH, HostKey*, Error* | custom |
| **Segmented control** (pill group, `aria-pressed` or `role=radiogroup`) | Main (protocol filter), Edit (mode switch, Methode), HostKeyNew (SHA/MD5 toggle) | custom |
| **Vertical tab list** (left nav, accent left-bar on active) | Edit (Allgemein/Auth/Verbindung/SFTP) | `role="tablist" aria-orientation="vertical"` |
| **Card (host card)** | Main | radius 12, border accent on select |
| **Selectable option card** (bordered, label+hint, accent border when picked) | Edit (profile vs custom source) | |
| **Color swatch radio** | Edit (host color) | circular, ring on select |
| **Status dot** (8/7px circle, green/amber/dim/red) | Main (host status), tab bar | |
| **Icon-chip avatar** (initials, mono font, rounded square, host accent color) | Main, Edit header, ConnectSSH | sizes: 26/38/44/48/52/60 |
| **Primary button** (accent bg, on-accent text, bold, 40–44h) | everywhere ("Neuer Host", "Speichern", "Vertrauen und verbinden", "Terminal öffnen", "Erneut versuchen", "Erneut anmelden") | |
| **Outlined/secondary button** (`border:--bd-strong` or `--bd-input`, transparent bg) | everywhere ("Abbrechen" variants, "Host bearbeiten", "Nur dieses Mal") | |
| **Text/ghost link button** (no border, muted/accent text) | "Zur Übersicht", "Abbrechen" (modal), copy buttons | |
| **Destructive-but-gated button** (disabled until checkbox confirm) | HostKeyChanged ("Schlüssel ersetzen und verbinden") | important anti-footgun pattern |
| **Inverted-neutral "safe default" button** (`bg:--text;color:--bg`) | HostKeyChanged ("Verbindung abbrechen") | visually most prominent = safest action |
| **Dashed "add" button** | Edit ("+ Regel hinzufügen"), SSH.dc.html ("+ Snippet anlegen", "+ Weiterleitung") | |
| **Copy-to-clipboard button w/ "Kopiert" feedback label swap** | HostKeyNew, Edit (Login-Profil "Public Key kopieren" — no feedback state wired there though) | |
| **Progress bar** (`role=progressbar`, thin 4px) | ConnectSSH | |
| **Step list item** (done/active/pending tri-state icon) | ConnectSSH | reusable for SFTP connect pipeline too |
| **Collapsible log/details** (`<details>` or custom toggle + `<pre>`) | ConnectSSH ("Details anzeigen"), HostKeyChanged (`<details>` native) | two different implementations of the same concept — standardize on one |
| **Definition list (`dl`) info panel** | Main (host details), SSH.dc.html (Sitzung/session info) | 2-col grid, label+value rows |
| **Fingerprint display card** (mono code block + format toggle) | HostKeyNew, HostKeyChanged | |
| **Two-column comparison card** (neutral vs red) | HostKeyChanged | |
| **Inline field validation message** (red text + `aria-invalid`/`aria-describedby`) | ErrorAuth | canonical pattern to reuse in Edit forms |
| **Rejected-methods list row** | ErrorAuth | |
| **Checkbox row with label** (`.chk`, 16px, accent-color) | Edit, HostKeyNew/Changed, ErrorConnection/Auth | |
| **Auto-reconnect countdown w/ pause toggle** | SSH.dc.html (connection-lost panel); ErrorConnection's unwired checkbox implies the same pattern should extend there | |
| **Pulse-ring animated avatar** (connecting state) | ConnectSSH | `@keyframes pulse` |
| **Spin icon** (loading/retry) | ConnectSSH, ErrorConnection (busy), tab bar spinner | `@keyframes spin` |

### Icon shape → Lucide mapping summary
- Hosts/server-stack → custom (two stacked `rect`s with dot) — closest: `Server` (duplicated) or
  custom.
- Terminal → `Terminal` / `SquareTerminal`.
- SFTP/folder-sync → custom (folder + up/down arrows) — closest: `FolderSync` or `FolderInput`
  combined with `ArrowUpDown`.
- Snippets → `Code2` (chevrons `<>` style, actual path shows outward arrows `8 7l-5 5 5 5` /
  `16 7l5 5-5 5` — matches `ChevronsLeftRight` / `Code`).
- Schlüsselbund (keyring) → `KeyRound`.
- Port-Forwarding → `ArrowRightLeft` (two opposing horizontal arrows).
- Settings → `Settings` (gear, exact path matches Lucide's `Settings` gear-with-notches icon).
- Sun/Moon theme → `Sun` / `Moon`.
- Search → `Search`.
- Quick connect (lightning/bolt path `M13 2L4 14h7l-1 8 9-12h-7z`) → `Zap`.
- Plus/add → `Plus`.
- Edit/pencil → `Pencil` / `PenLine`.
- Close/X → `X`.
- Checkmark (done step) → `Check`.
- Loader/spinner (partial circle arc) → `LoaderCircle` / `Loader2`.
- Shield-alert (unknown host key) → `ShieldAlert`.
- Triangle-alert (changed host key) → `AlertTriangle` / `TriangleAlert`.
- Wifi-off (connection error idle icon) → `WifiOff`.
- Lock/server rejected (ErrorAuth icon, rect+arc) → `Lock` (shape is literally a padlock: body
  rect + shackle arc).
- Plug/connect (Verbindung testen, Quick Connect reuse same bolt path) → `Zap` (reused, not a
  distinct plug icon despite the "test connection" semantic — same SVG path as Quick Connect).
- Split vertical/horizontal (SSH toolbar, out of scope detail) → `PanelLeft`/`PanelTop`-ish
  custom.
- Broadcast (SSH toolbar) → `Radio` / `Wifi`-ish custom (concentric arcs + dot).

---

## 8. Cross-Cutting Ambiguities & Backend Capabilities Needed (Consolidated)

1. **Encrypted credential vault + sync** — referenced on Main (sidebar sync card) and Edit (store-
   in-vault checkboxes) but explicitly called out in README as only "angedeutet" (sketched). Needs
   a real design: likely OS keychain integration (Tauri has `keyring` crates) plus an optional
   sync mechanism (scope undefined — could be a non-goal for sub-project 1, worth confirming with
   stakeholders before building more than local secure storage).
2. **SSH key generation** ("Neu erzeugen" button in Edit > Auth > Schlüssel) — needs ed25519/
   RSA/etc. keypair generation capability in the Rust backend.
3. **SSH agent enumeration** ("2 Identitäten gefunden" in Edit > Auth > SSH-Agent) — needs live
   `SSH_AUTH_SOCK` querying.
4. **ssh-copy-id equivalent** ("Auf Hosts verteilen" in Login-Profil edit) — needs a host-picker UI
   (not designed) and an implementation to append a pubkey to remote `authorized_keys`.
5. **Port-forward rule editor** — only the list display + "+ Regel hinzufügen" button exist; no add/
   edit dialog is in the package. Data shape from README: `{type:'L'|'R'|'D', bind, target,
   autostart}`.
6. **Known-hosts store semantics** — scope of "remember" (per-Host vs per-address:port), and the
   replace-on-change operation, need an explicit data layer (not just a flat known_hosts file
   necessarily, given the per-group "remember" checkbox in HostKeyNew).
7. **Live host reachability/status** (`on/warn/off` dot on Main) — mechanism unspecified (polling?
   derived from last session state?).
8. **⌘K and Quick Connect behaviors** — not wired in mockup; need UX decisions (see §1).
9. **Keyboard-interactive/2FA runtime prompt** — Edit screen mentions this method, but ConnectSSH's
   fixed 5-step list has no room for an interactive prompt sub-step. Needs a design for how/where a
   runtime credential prompt interrupts the connect pipeline.
10. **Jump-host-less connect pipeline** — ConnectSSH always shows a "Jump-Host bastion verbinden"
    step; behavior when no jump host is configured (most hosts in the sample data have `jump:'—'`)
    is not shown — presumably that step should be omitted/skipped, needs explicit handling.
11. **Retry cap behavior** (ErrorConnection) — "Versuch 3 von 3" label caps but button doesn't
    disable; needs a product decision on max-retry enforcement and backoff strategy.
12. **Validation rules across Edit.dc.html** — essentially unspecified (no inline validation shown
    except the ErrorAuth password-required pattern, which should be the template to extend).
1. 3. **Edit modal "new vs existing" state** — mockup always shows a populated form (editing
    "web-prod-01"); there's no visible empty/"new host" state design — assume same layout with
    blank/default field values, but confirm with design before implementing.
14. **Group-as-freeform-string vs fixed enum** — Main's groups are dynamic (derived from host data
    + "Alle Hosts"), but Edit's Gruppe `<select>` has 5 hardcoded options with no "new group" entry
    option — needs a creatable-select or an "Add group" affordance.
15. **Env var field** (single string, e.g. `"LANG=de_DE.UTF-8"`) — delimiter/format for multiple
    vars is unspecified; likely needs to become a repeatable key-value list control rather than one
    text input.

---

## 9. Scenario Routing Table (from Main.dc.html mockup data + README)

For dev/test fixtures and to understand the intended demo narrative (NOT literal production
routing logic — production must derive the destination screen from actual connection outcomes):

| Host | SSH target | SFTP target | Narrative |
|---|---|---|---|
| web-prod-01 | ConnectSSH | ConnectSFTP | Success |
| db-prod-01 | ConnectSSH | ConnectSFTP | Success |
| bastion | ConnectSSH | ConnectSFTP | Success |
| coolify-01 | HostKeyNew | HostKeyNew | Unknown host key |
| staging-app | HostKeyChanged | HostKeyChanged | Changed host key (blocked) |
| kunde-shop | ErrorAuth | ErrorAuth | Auth failed |
| kunde-cms | ConnectSSH | ConnectSFTP | Success |
| nas | ErrorConnection | ErrorConnection | Timeout (variant prop can also be set to refused/dns for design review) |

---

## 10. Files Read (for reference)

- `shellport-design/README.md`
- `shellport-design/tokens/tokens.css`, `tokens/tokens.json`
- `shellport-design/screens/Main.dc.html`
- `shellport-design/screens/Edit.dc.html`
- `shellport-design/screens/ConnectSSH.dc.html`
- `shellport-design/screens/HostKeyNew.dc.html`
- `shellport-design/screens/HostKeyChanged.dc.html`
- `shellport-design/screens/ErrorConnection.dc.html`
- `shellport-design/screens/ErrorAuth.dc.html`
- `shellport-design/screens/SSH.dc.html` (skimmed for app shell / tab bar / single-terminal-tab
  reference only — full feature set (split panes, snippets, tunnels, color schemes, broadcast) is
  out of scope for sub-project 1 and only noted here for continuity/consistency of the shared shell)
- `shellport-design/screens/canvas.json` (screen index/notes only)
