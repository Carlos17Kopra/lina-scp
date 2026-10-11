# LinaSCP – Teilprojekt 1: Fundament + erster Durchstich

**Datum:** 2026-10-11 · **Status:** Entwurf zur Freigabe

## 1. Ziel und Kontext

LinaSCP ist ein Linux-nativer Desktop-Client für SSH und SFTP. SSH-Funktionen orientieren sich an Termius, SFTP-Funktionen an WinSCP. Das Design stammt aus `shellport-design.zip` („Shellport“ war nur Arbeitstitel; überall durch **LinaSCP** ersetzen).

- Nutzer: zunächst nur der Entwickler selbst, verteilt über ein eigenes Git-Repo auf mehrere eigene Linux-Geräte. Später evtl. non-kommerzielles Open-Source-Release.
- Plattform: Linux (Primär: Debian 13, GNOME, Wayland, NVIDIA + AMD iGPU). Andere OS sind kein Ziel.
- Sprache: UI-Texte Deutsch; Code, Kommentare, Bezeichner, Commit-Messages Englisch.

### Teilprojekte (Gesamtprojekt)

| # | Teilprojekt | Status |
|---|---|---|
| **1** | **Fundament + erster Durchstich** (diese Spec) | in Planung |
| 2 | SSH-Terminal komplett: Tabs, Split-Panes, Broadcast, Snippets, Auto-Reconnect, Farbschemata, Sitzungsinfo | später |
| 3 | SFTP-Dateimanager: Commander Lokal/Remote, Breadcrumbs, Lesezeichen, Warteschlange, Überschreiben-/Löschen-Dialoge, Bearbeiten | später |
| 4 | SFTP Advanced: Synchronisieren, Vergleichen, Aktuell halten, Rechte, Bandbreite, sudo-sftp | später |
| 5 | Tunnel (L/R/D) mit UI, Schlüsselbund-Screen inkl. Key-Erzeugung + ssh-copy-id, Einstellungen komplett | später |

### Referenzmaterial

- Design-Paket: `shellport-design.zip` (Screens, `tokens/tokens.css`, `tokens/tokens.json`, `tokens/terminal-schemes.json`)
- Detail-Auszug aller TP1-Screens (Layout, Felder, verbatim-Texte, Komponenten): `reference/tp1-screen-digest.md`
- Tech-Recherche mit Versionen und Quellen: `reference/tp1-tech-research.md`

## 2. Umfang Teilprojekt 1

**Enthalten**
1. Projektgerüst (Rust-Workspace + Tauri 2 + React), Design-Tokens, App-Shell (Rail, Hell/Dunkel persistent).
2. Lokaler Speicher (SQLite), Secrets im OS-Schlüsselbund.
3. Server-Übersicht (Main): Host-Karten, Gruppen, Login-Profile, Filter Alle/SSH/SFTP, Suche `Ctrl+K`, Quick Connect `user@host:port`, Detail-Panel, Aktionen Terminal / Bearbeiten (Dateien deaktiviert bis TP3).
4. Editor (Edit) Host ↔ Login-Profil mit allen Reitern (Allgemein, Authentifizierung, Verbindung, SFTP). SFTP-Reiter wird gespeichert, wirkt ab TP3. Port-Forward-Liste wird gespeichert, wirkt ab TP5. „Verbindung testen“.
5. Verbindungs-Pipeline inkl. Jump-Host, Host-Key-Prüfung, allen Auth-Methoden.
6. Screens: Laden SSH, Unbekannter Host-Key, Host-Key geändert, Verbindungsfehler (timeout/refused/dns), Anmeldung fehlgeschlagen.
7. Terminal-Screen mit Tab-Leiste, aber je Verbindung genau ein Tab; zusätzlich lokale Terminals (`+`).
8. `~/.ssh`-Integration (Integrieren/Spiegeln) mit Erststart-Dialog.
9. Minimaler Einstellungsdialog: Theme, ssh-Modus, Terminal-Renderer, Export, Import.
10. Export/Import als `.linascp`-Datei, optional mit verschlüsselten Geheimnissen.

**Nicht enthalten** (siehe Teilprojekte 2–5): Split-Panes, Broadcast, Snippets, Auto-Reconnect bei Verbindungsverlust in laufender Sitzung, Schema-Auswahl, SFTP-Funktionen, Tunnel-Betrieb, Key-Erzeugung, ssh-copy-id. Rail-Einträge Snippets, Schlüsselbund, Port-Forwarding, Einstellungs-Screen werden ausgegraut angezeigt (Einstellungen öffnet in TP1 den minimalen Dialog).

## 3. Stack

| Bereich | Wahl | Version (Stand 2026-10) |
|---|---|---|
| App-Framework | Tauri 2 (WebKitGTK auf Linux) | 2.12.x |
| Frontend | React 19 + TypeScript (strict) + Vite | aktuell |
| State | Zustand | aktuell |
| Typsichere Bindings | tauri-specta (TS-Typen aus Rust-Commands generiert) | aktuell |
| Styling | CSS Modules + `tokens.css` (1:1 aus Design) | – |
| Icons / Fonts | lucide-react; `@fontsource` IBM Plex Sans + JetBrains Mono (lokal gebündelt) | – |
| Terminal | `@xterm/xterm` + addons fit, webgl, web-links, unicode11 | 6.0.x |
| SSH | `russh` (inkl. `russh::keys`; nicht das veraltete `russh-keys`) | 0.64.x |
| SFTP (ab TP3) | `russh-sftp` | 3.0.x |
| ssh-config | `ssh2-config` | 0.8.x |
| known_hosts | `ssh-key` / `russh::keys` (inkl. gehashter Einträge) | – |
| Lokale PTY | `portable-pty` | aktuell |
| DB | `rusqlite` mit Migrationen | 0.40.x |
| Schlüsselbund | `keyring` 4 + Linux-Backend `dbus-secret-service-keyring-store` (sync, in `spawn_blocking`) | 4.2.x |
| Export-Verschlüsselung | `age` (Passphrase/scrypt) | aktuell |
| Fehler / Logs / Secrets | `thiserror`, `tracing` (+ Datei-Rotation), `zeroize` | – |
| Tests | `cargo test`, Docker-sshd-Container; Vitest + Testing Library | – |

Paketverwaltung/Scripts Frontend: `bun` (auf dem System vorhanden, von Tauri unterstützt).

## 4. Architektur

```
linascp/
├─ Cargo.toml              # workspace
├─ crates/core/            # pure Rust, kein Tauri: ssh, sftp(TP3), store, vault, known_hosts, sshconfig, export, sessions
├─ src-tauri/              # dünne Schicht: Tauri-Commands + Channels, ruft core auf
└─ ui/                     # React + TS + Vite
```

- `crates/core` ist ohne GUI testbar und könnte bei Bedarf als Sidecar (z. B. für ein Electron-Frontend) laufen.
- `src-tauri` enthält keine Fachlogik, nur Übersetzung Command ↔ core-API und Channel-Verdrahtung.

### Datenfluss

- **UI → Kern:** Tauri-Commands, u. a. `hosts_list`, `host_save`, `host_delete`, `profile_save`, `profile_delete`, `session_open_ssh`, `session_open_local`, `session_write`, `session_resize`, `session_close`, `connect_answer` (Host-Key-Entscheidung, 2FA-Antworten, Passwort-Retry), `connect_cancel`, `settings_get/set`, `export_file`, `import_preview`, `import_apply`, `test_connection`.
- **Kern → UI:**
  - je Sitzung ein Tauri-`Channel` mit Binärdaten (Terminal-Output),
  - je Verbindungsaufbau ein Event-`Channel` (Schritt-Fortschritt, Log-Zeilen, Host-Key-Abfrage, 2FA-Prompt, Fehler, Erfolg).
  - Keine Tauri-Events (JSON-only, ungeordnet) für Terminal-Daten.
- **Pausen:** Pipeline wartet bei Host-Key- und 2FA-Abfragen auf `connect_answer`. Ohne Antwort (Abbruch, Fenster zu, Timeout) gilt **Ablehnen** (fail-closed).
- **Sitzungen:** Jede Sitzung (SSH oder lokale PTY) läuft als eigener Tokio-Task hinter einem gemeinsamen `Session`-Trait (`write`, `resize`, `close`, Output-Stream, Exit-Ereignis). Das Terminal-UI unterscheidet nicht zwischen lokal und remote.

## 5. Datenmodell und Speicher

SQLite unter `~/.local/share/linascp/linascp.db`, Schema-Migrationen versioniert. IDs sind UUIDs.

| Tabelle | Felder |
|---|---|
| `hosts` | id, name, address, port (Default 22), user, group_id?, color, notes, protocol_ssh (bool), protocol_sftp (bool), login_profile_id? **oder** eigene Auth (auth_method, key_path, has_secret), jump_host_id?, keepalive_s (Default 30), timeout_s (Default 15), start_command, env (JSON), sftp_settings (JSON), source (`linascp` \| `ssh_config`), created_at, updated_at |
| `groups` | id, name, sort_order |
| `host_tags` | host_id, tag |
| `login_profiles` | id, name, user, method (`password` \| `key` \| `agent` \| `keyboard_interactive`), key_path?, has_secret, created_at, updated_at |
| `port_forwards` | id, host_id, type (`L` \| `R` \| `D`), bind, target, autostart |
| `settings` | key, value (u. a. `theme`, `ssh_mode`, `terminal_renderer`, `terminal_font_size`, `first_run_done`) |

### Geheimnisse

- Passwörter und Key-Passphrasen ausschließlich im OS-Schlüsselbund: `service="linascp"`, `account="<profile|host>:<id>:<password|passphrase>"`. In der DB nur `has_secret`.
- Private Schlüssel bleiben Dateien; gespeichert wird nur der Pfad.
- Schlüsselbund ist hinter einem Trait `SecretStore` gekapselt (Fake-Implementierung für Tests).
- Gesperrter/fehlender Schlüsselbund: Verbindung trotzdem möglich, Secret wird abgefragt, Hinweis „Speichern derzeit nicht möglich“.

### Host-Keys

- Keine eigene Tabelle. LinaSCP liest und schreibt `~/.ssh/known_hosts` (beide Modi).
- Gehashte Einträge werden erkannt; neue Einträge werden gehasht geschrieben (Default, passend zu `HashKnownHosts`).
- Nicht-Standard-Port im OpenSSH-Format `[host]:port`.

### `~/.ssh`-Integration

- **Erststart-Dialog:** Auswahl „Integrieren“ (vorausgewählt) oder „Spiegeln“. Später in Einstellungen änderbar.
- **Integrieren:** `~/.ssh/config` wird beim Start (und per „Neu einlesen“) read-only importiert. Diese Hosts erscheinen mit Badge „aus ssh-config“ (`source=ssh_config`, nicht in DB persistiert außer Overrides). Bearbeiten erzeugt einen LinaSCP-Host (`source=linascp`), der den Config-Eintrag gleichen Namens überlagert. Die Datei wird nie verändert.
- **Spiegeln:** wie Integrieren, plus: alle LinaSCP-Hosts werden nach `~/.ssh/config.d/linascp.conf` geschrieben (vollständig neu generiert, mit Kopfkommentar „generated by LinaSCP – do not edit“). Beim Aktivieren wird nach Bestätigung einmalig `Include ~/.ssh/config.d/linascp.conf` an den **Anfang** von `~/.ssh/config` gesetzt (vorher Backup `~/.ssh/config.linascp.bak`). Beim Deaktivieren: Datei und Include-Zeile entfernen (nach Bestätigung).
- Agent: `SSH_AUTH_SOCK` wird genutzt, wenn gesetzt.

### Export / Import

- Format: `.linascp` = JSON mit `format_version`, enthält Hosts, Gruppen, Tags, Login-Profile, Port-Forwards, Einstellungen.
- Häkchen „Geheimnisse einschließen“: Secrets werden mit `age` (Passphrase, scrypt) verschlüsselt und als ASCII-armored Block eingebettet. Passwort wird beim Export doppelt abgefragt. Private Schlüsseldateien sind optional mit einschließbar (ebenfalls verschlüsselt).
- Import: Vorschau mit Konfliktliste; je Konflikt (gleiche ID oder gleicher Name) Wahl „Überspringen / Ersetzen / Als Duplikat“. Secrets landen im Schlüsselbund des Zielgeräts; Schlüsseldateien nach `~/.ssh/linascp/` mit Rechten `0600`.

## 6. Verbindungs-Pipeline

Zustandsmaschine im Kern. Jeder Schritt meldet `step_started` / `step_done` / `step_failed` + Log-Zeilen (Stil `ssh -v`), daraus speisen sich Schrittliste, Fortschrittsbalken und aufklappbares Log im Lade-Screen.

```
[Jump-Host]* → TCP → Host-Key → Anmelden → Kanal → Shell        (* nur wenn konfiguriert)
```

1. **Jump-Host:** Kompletter Verbindungsaufbau zum Jump-Host (eigene Host-Key-Prüfung, eigene Auth), dann `direct-tcpip`-Kanal zum Ziel, darüber zweiter Handshake (`connect_stream`). Ketten sind architektonisch möglich; TP1 testet einen Sprung.
2. **TCP:** Timeout aus Host-Einstellung. Fehler werden klassifiziert in `timeout`, `refused`, `dns` (+ Originalmeldung).
3. **Host-Key:** Abgleich mit `known_hosts`:
   - bekannt + gleich → weiter
   - unbekannt → Pause, Dialog „Unbekannter Host-Key“ (SHA256 + MD5, Kopieren). Optionen: „Nur dieses Mal“ (nicht speichern), „Vertrauen und verbinden“ (in `known_hosts` schreiben), Abbrechen.
   - geändert → Pause, Warn-Dialog „Host-Key geändert“: Vergleich gespeichert/aktuell, Original-OpenSSH-Warnung. Primäraktion „Verbindung abbrechen“; „Schlüssel ersetzen und verbinden“ erst nach Bestätigungs-Checkbox aktiv; ersetzt nur den betroffenen Eintrag.
4. **Anmelden:** Reihenfolge = konfigurierte Methode:
   - Passwort: aus Schlüsselbund, sonst Abfrage.
   - Schlüssel: Datei laden; bei Verschlüsselung Passphrase aus Schlüsselbund, sonst Abfrage.
   - Agent: alle Identitäten aus `SSH_AUTH_SOCK` nacheinander.
   - Keyboard-Interactive / 2FA: Server-Prompts erscheinen inline im Lade-Screen beim Schritt „Anmelden“ (echo/no-echo beachten).
   - Fehlschlag → Screen „Anmeldung fehlgeschlagen“ mit abgelehnten Methoden (aus Server-Antwort), Passwort neu eingeben (Leer-Validierung), Häkchen „im Profil speichern“, Link „Login-Profil bearbeiten“.
5. **Kanal:** Session-Channel, PTY-Request `xterm-256color` mit aktueller Größe, Env-Variablen aus Host-Einstellungen.
6. **Shell:** Shell starten; ggf. Startbefehl senden. Danach Wechsel in den Terminal-Screen.

Weitere Regeln:
- Keepalive aus Host-Einstellungen (Default 30 s).
- Resize im UI → `window-change`.
- Sitzungsende durch Remote (`exit`) → Tab zeigt „Sitzung beendet“ mit Schließen/Neu verbinden.
- Abbrechen im Lade-Screen bricht die Pipeline sofort ab und schließt offene Verbindungen.
- **Verbindungsfehler-Screen:** Zähler „Versuch n von 3“, Option Auto-Retry (Backoff 2 s / 4 s / 8 s, max. 3 Versuche, danach Stopp). „Erneut versuchen“ bleibt immer klickbar (setzt den Zähler fort).
- **Lokales Terminal:** `portable-pty` mit `$SHELL` (Fallback `/bin/bash`) im Home-Verzeichnis, `TERM=xterm-256color`; keine Pipeline, kein Lade-Screen.

### Sicherheit

- Secrets niemals in Logs, Event-Channel-Payloads oder DB. Log-Zeilen werden vor Ausgabe bereinigt.
- Secrets im Speicher als `Zeroizing`/`SecretString`, nur solange nötig.
- russh-Defaults für Algorithmen (inkl. Strict-KEX / Terrapin-Fix). Kein Downgrade-Schalter in TP1.
- Host-Key-Entscheidung immer fail-closed.
- Exportdateien mit Secrets werden mit Rechten `0600` geschrieben.

## 7. UI

- **Navigation:** View-State `hosts | terminal | sftp` (Rail), kein Router. Modal-Layer für Editor, Host-Key-Dialoge, Einstellungen, Erststart, Export/Import.
- **Theming:** `tokens.css` am App-Root, `.light` überschreibt; Default dunkel, Wahl persistent.
- **Komponenten** (`ui/src/components/`): Button (primary/secondary/ghost/danger), IconButton (mit `aria-label`), Input mit Feldfehler, Select, Checkbox, Segmented/Radio, Chip/Badge, Card, Dialog (Fokus-Falle, `role="dialog"`, Esc), Tabs (`role="tab"`), Toast, ProgressBar, StepList.
- **Screens** gemäß `reference/tp1-screen-digest.md`; Layout-Maße, Texte und Farben aus dem Design-Paket. Die Rail markiert das Protokoll, aus dem ein Ablauf-Screen stammt.
- **Editor-Validierung:** Inline am Feld. Pflicht: Name, Adresse. Port 1–65535. Zahlenfelder (Keepalive, Timeout, Parallelität) ganzzahlig ≥ 0. Env-Zeilen `KEY=VALUE`. Speichern deaktiviert, solange Fehler bestehen.
- **Host → Profil:** Sprung aus dem Auth-Reiter in den Profil-Editor über Navigationsstapel; „Zurück zum Host“ stellt ungespeicherte Host-Eingaben wieder her.
- **Terminal (`<TerminalView>`):** xterm.js mit Fit, WebGL, Web-Links, Unicode11. Deckender Hintergrund (WebKit-Artefakt bei Transparenz). Renderer: WebGL mit automatischem Fallback auf DOM bei Fehler/Context-Loss; manuell umschaltbar in Einstellungen (`auto | webgl | dom`). Copy/Paste: Markieren kopiert in PRIMARY, Mittelklick fügt ein, `Ctrl+Shift+C/V`. Standard-Farbschema aus `terminal-schemes.json` (Schema-Auswahl in TP2), Schriftgröße 14.
- **Tastatur:** `Ctrl+K` Suche, `Ctrl+N` neuer Host, `Ctrl+Shift+T` lokales Terminal, `Ctrl+,` Einstellungen.
- **Texte:** Deutsch, verbatim aus dem Design, zentral in `ui/src/i18n/de.ts`.
- **Barrierefreiheit:** echte `button`/`a`/`input`+`label`, Kontrast ≥ 4,5:1 in beiden Modi.

## 8. Terminal-Rendering unter WebKitGTK (Spike)

Risiko: xterm.js-Leistung in WebKitGTK auf Wayland + NVIDIA. Ein Wegwerf-Spike misst Durchsatz und Framerate (4 Panes, Log-Flood, Vollbild-Redraws) mit WebGL vs. DOM und den Umgebungsvariablen `WEBKIT_DISABLE_DMABUF_RENDERER` / `WEBKIT_DISABLE_COMPOSITING_MODE`, Baseline Chromium.

**Entscheidungsregel:**
- Ist ein Renderer in Tauri flüssig nutzbar (Log-Flood ohne UI-Einfrieren, Redraws ≥ 30 fps), bleibt es bei Tauri. Benötigte Env-Workarounds setzt LinaSCP beim Start selbst (vor WebView-Init) bzw. über den Desktop-Launcher.
- Andernfalls: Wechsel des Frontends auf Electron, `crates/core` als Sidecar. Diese Spec wird dann in Abschnitt 3/4 angepasst und erneut freigegeben.

**Ergebnis (2026-10-11, Details: `reference/spike-xterm-webkitgtk.md`): Tauri bleibt.**

- WebGL in Tauri: ~68–70 MB/s Log-Flood (Electron/Chromium ~70–72), Redraws stabil 60 fps, 23 Starts ohne Absturz/Leerfenster, läuft auf der NVIDIA-GPU.
- `WEBKIT_DISABLE_*`-Variablen werden **nicht** standardmäßig gesetzt (kosten 10–13 % Durchsatz). Stattdessen Einstellung „Kompatibilitäts-Rendering“ (opt-in, wirkt nach Neustart), die `WEBKIT_DISABLE_DMABUF_RENDERER=1` setzt.
- DOM-Renderer nur als Fallback (bei WebGL-Context-Loss oder manuell).
- Transport: `Channel<InvokeResponseBody>` mit `Raw(Vec<u8>)` (300–370 MB/s), niemals `Channel<Vec<u8>>`. Chunks 16–64 KiB, Ausgabe-Bündelung im Backend.
- Flow-Control: Frontend bestätigt geschriebene Bytes (`session_ack`); Backend pausiert das Lesen über 1 MiB unbestätigt, setzt unter 256 KiB fort.
- Max. 16 WebGL-Kontexte: WebGL nur an sichtbaren Terminals, beim Verstecken freigeben.

## 9. Fehlerbehandlung

- Kern: `CoreError` (thiserror) mit Kategorien `Network{Timeout|Refused|Dns}`, `HostKey{Unknown|Changed}`, `Auth{rejected_methods}`, `Keyring`, `Store`, `Config`, `Io`, `Internal`; jeweils mit Originalmeldung.
- Serialisiert an die UI als getaggtes Objekt; die UI wählt Screens ausschließlich anhand der Kategorie.
- Unerwartete Fehler: Toast mit „Details kopieren“; kein App-Absturz (Panics in Session-Tasks werden abgefangen und als Sitzungsfehler gemeldet).
- Logs: `tracing` → `~/.local/state/linascp/logs/` (tägliche Rotation, 7 Tage), bereinigt.

## 10. Tests

- **Kern Unit:** ssh-config-Parser-Anbindung, known_hosts lesen/schreiben (Klartext, gehasht, `[host]:port`), Store + Migrationen, Export/Import inkl. age-Roundtrip und Konfliktauflösung, Fehlerklassifizierung, Pipeline-Zustandsmaschine (Mock-Transport), Spiegel-Modus (Include-Zeile, Backup, Entfernen) in Temp-Home.
- **Kern Integration (Docker):** OpenSSH-Container (generiert in Test-Fixture). Szenarien: Passwort, Key ohne/mit Passphrase, Agent (Test-Agent), Keyboard-Interactive, unbekannter Host-Key, geänderter Host-Key, Jump-Host (2 Container im Netz), abgelehnter Port, DNS-Fehler, Auth-Fehlschlag. Mit `--ignored`/Feature-Flag, damit `cargo test` auch ohne Docker läuft.
- **UI:** Vitest + Testing Library; Commands gemockt. Mindestens: Editor-Validierung, Host-Key-geändert-Dialog (Ersetzen erst nach Checkbox), Filter + Suche, Quick-Connect-Parser, Erststart-Dialog, Export-Dialog (Passwort-Wiederholung).
- **Manuelle Abnahme:** Checkliste gegen die TP1-Screens des Design-Pakets in Hell und Dunkel.

## 11. Paketierung

`tauri build` → `.deb` und AppImage. Desktop-Datei + Icon. Flatpak erst zum Release.

## 12. Abnahmekriterien

1. Verbindung zu echten Servern per Passwort, Schlüssel (mit/ohne Passphrase), Agent, 2FA und über einen Jump-Host.
2. Host-Key-Prüfung konsistent mit OpenSSH: von LinaSCP vertraute Hosts kennt `ssh` und umgekehrt; geänderte Keys werden blockiert.
3. Hosts aus `~/.ssh/config` erscheinen (Integrieren); Spiegeln erzeugt eine von `ssh` nutzbare Config.
4. Alle Fehler-Screens erscheinen für die richtigen Fälle (timeout, refused, dns, auth, host-key).
5. Export → Import auf einem anderen Gerät inkl. Secrets ergibt eine funktionsfähige Konfiguration.
6. Lokale Terminals funktionieren.
7. Alle automatischen Tests grün (Integrationstests mit Docker).
