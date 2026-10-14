# LinaSCP

SSH- und SFTP-Client für Linux (Tauri 2 + React + Rust). SSH-Funktionen orientieren sich an Termius,
SFTP-Funktionen an WinSCP. Stand: **Teilprojekt 1** – Host-Verwaltung, Login-Profile,
Verbindungsaufbau (Passwort, Schlüssel, Agent, 2FA, Jump-Host), Host-Key-Prüfung über
`~/.ssh/known_hosts`, Terminal-Tabs (SSH und lokal), `~/.ssh`-Integration, Export/Import.
Der SFTP-Dateimanager folgt in Teilprojekt 3.

## Voraussetzungen (Debian 13 „trixie“)

```bash
sudo apt install build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
  libdbus-1-dev unzip docker.io
```

- **atk-Pakete:** Falls `apt` bei `libwebkit2gtk-4.1-dev` wegen nicht erfüllbarer Abhängigkeiten der
  atk-Pakete (`libatk1.0-dev`, `libatk-bridge2.0-dev`) abbricht, diese aus den Backports installieren:
  `echo 'deb http://deb.debian.org/debian trixie-backports main' | sudo tee /etc/apt/sources.list.d/backports.list`,
  `sudo apt update`, `sudo apt install -t trixie-backports libatk1.0-dev libatk-bridge2.0-dev` und danach
  die obige Liste erneut ausführen.
- **Rust** (stable, Edition 2024): `curl https://sh.rustup.rs -sSf | sh`
- **bun** (Paketverwaltung und Skripte): `curl -fsSL https://bun.sh/install | bash`
- **Node.js 20** (für Vitest; Debian-Paket `nodejs` genügt)
- Für Schlüsselbund-Zugriff einen laufenden Secret Service (GNOME Keyring oder KeePassXC).

## Entwickeln

```bash
bun install            # Frontend-Abhängigkeiten
bun run tauri dev      # App mit Hot-Reload starten (Vite auf :5173)
```

Die TypeScript-Bindings (`ui/src/ipc/bindings.ts`) werden im Debug-Start und mit
`cargo test -p linascp exports_bindings` aus den Rust-Commands neu erzeugt.

Hinweis: Im Entwicklungsmodus (`tauri dev`, React StrictMode) kann der erste Prompt eines neuen
Terminals fehlen, bis einmal Enter gedrückt wird. Gebaute Pakete sind davon nicht betroffen.

## Tests

```bash
cargo test --workspace                                                     # Rust-Unit-Tests (ohne Docker)
cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1   # SSH-Integration (Docker)
cargo clippy --workspace --all-targets -- -D warnings                      # Lints
bun run typecheck                                                          # TypeScript
bun run test                                                               # Vitest (UI)
```

Die Integrationstests starten OpenSSH-Container über Docker; der aktuelle Benutzer muss Docker
verwenden dürfen (Gruppe `docker`).

## Bauen und installieren

```bash
bun run tauri build
sudo apt install ./target/release/bundle/deb/LinaSCP_0.1.0_amd64.deb
# oder ohne Installation:
./target/release/bundle/appimage/LinaSCP_0.1.0_amd64.AppImage
```

## Release erstellen

Releases entstehen auf GitHub aus Versions-Tags (`.github/workflows/release.yml`).

1. `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` und `package.json` erhöhen
   (alle drei müssen gleich sein).
2. Committen, dann taggen und pushen:
   ```bash
   git tag vX.Y.Z && git push origin vX.Y.Z
   ```
3. Der Workflow führt alle Prüfungen aus (fmt, clippy, Rust-Tests, SSH-Integrationstests mit Docker,
   Typecheck, Vitest), baut `.deb` und `.AppImage` und legt sie samt `SHA256SUMS` an einen
   **Entwurf** („Draft“) des Releases. Diesen Entwurf auf GitHub prüfen und veröffentlichen.

Der Workflow bricht ab, wenn der Tag (ohne `v`) nicht zur `version` in `src-tauri/tauri.conf.json`
passt oder die drei Versionen voneinander abweichen. Ein manueller Start (Actions, „Run workflow“)
baut nur und stellt die Pakete als Workflow-Artefakte bereit; ein Release entsteht dabei nur, wenn
als Ref ein Tag gewählt wurde.

## Daten und Einstellungen

| Was | Wo |
|---|---|
| Hosts, Profile, Einstellungen | `~/.local/share/linascp/linascp.db` (SQLite) |
| Passwörter, Passphrasen | Schlüsselbund des Systems (Dienst `linascp`) |
| Host-Keys | `~/.ssh/known_hosts` (gemeinsam mit `ssh`) |
| Logs | `~/.local/state/linascp/logs/` (7 Tage) |
| Gespiegelte Hosts (Modus „Spiegeln“) | `~/.ssh/config.d/linascp.conf` + `Include`-Zeile in `~/.ssh/config` (Sicherung `~/.ssh/config.linascp.bak`) |

- **Integrieren** (Standard): `~/.ssh/config` wird nur gelesen; diese Hosts tragen den Hinweis „aus ssh-config“.
- **Spiegeln:** zusätzlich werden LinaSCP-Hosts für `ssh` nutzbar gemacht.
- **Kompatibilitäts-Rendering** (Einstellungen): setzt beim nächsten Start
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` – nur bei Darstellungsfehlern einschalten.
- **Export/Import:** `.linascp`-Datei; Geheimnisse optional mit Passphrase (age) verschlüsselt.

## Tastatur

`Ctrl+K` Suche · `Ctrl+N` neuer Host · `Ctrl+Shift+T` lokales Terminal · `Ctrl+,` Einstellungen ·
im Terminal `Ctrl+Shift+C`/`Ctrl+Shift+V` kopieren/einfügen, Markieren kopiert, Mittelklick fügt ein.
