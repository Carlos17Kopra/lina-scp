# Abnahme Teilprojekt 1 – manuelle Checkliste

Grundlage: Spec `docs/superpowers/specs/2026-10-11-linascp-tp1-design.md` (Abschnitt 12) und die
TP1-Screens des Design-Pakets (`*-design.zip` im Repo-Root). Jeden Screen in
**Hell** und **Dunkel** prüfen (Theme-Schalter unten in der Rail). Datum, Gerät und Build notieren.

- Datum: ____________ · Gerät/GPU: ____________ · Build: `LinaSCP_0.1.0` (deb / AppImage / dev)

## 0. Vorbereitung

- [ ] Automatische Tests grün (siehe Abschnitt 4).
- [ ] Testserver bereit: ein Server mit Passwort-Login, einer mit Schlüssel (ohne und mit Passphrase),
      einer mit 2FA/Keyboard-Interactive, ein Jump-Host davor; ein laufender `ssh-agent` mit Schlüssel.
- [ ] Frisches Profil: `~/.local/share/linascp` umbenannt, `~/.ssh/config` mit mindestens einem Host-Eintrag.

## 1. Abnahmekriterien (Spec §12)

1. Verbindungen zu echten Servern
   - [ ] Passwort (Abfrage im Lade-Screen, „Passwort im Schlüsselbund speichern“, zweiter Start ohne Abfrage)
   - [ ] Schlüssel ohne Passphrase
   - [ ] Schlüssel mit Passphrase (Abfrage inline, danach aus dem Schlüsselbund)
   - [ ] SSH-Agent
   - [ ] 2FA / Keyboard-Interactive (Prompts inline beim Schritt „Anmelden“, Echo/No-Echo korrekt)
   - [ ] Über einen Jump-Host (Schritt „Jump-Host … verbinden“ mit Ziel `user@jump:port`)
2. Host-Keys konsistent mit OpenSSH
   - [ ] Von LinaSCP vertrauter Host („Vertrauen und verbinden“) → `ssh host` fragt nicht erneut.
   - [ ] Von `ssh` bekannter Host → LinaSCP verbindet ohne Dialog (auch gehashte Einträge, `[host]:port`).
   - [ ] „Nur dieses Mal“ schreibt nichts nach `~/.ssh/known_hosts`.
   - [ ] Geänderter Key (Eintrag in `known_hosts` manipulieren) → „Host-Schlüssel hat sich geändert“, Verbindung blockiert;
         „Schlüssel ersetzen und verbinden“ erst nach Häkchen aktiv, ersetzt nur den betroffenen Eintrag.
3. `~/.ssh`-Integration
   - [ ] Erststart-Dialog: „Integrieren“ vorausgewählt; Hosts aus `~/.ssh/config` erscheinen mit „aus ssh-config“.
   - [ ] „ssh-config neu einlesen“ übernimmt Änderungen an `~/.ssh/config`.
   - [ ] Bearbeiten eines ssh-config-Hosts erzeugt einen LinaSCP-Host gleichen Namens; `~/.ssh/config` bleibt unverändert.
   - [ ] Spiegeln aktivieren (Bestätigung) → `~/.ssh/config.d/linascp.conf` + `Include` am Anfang von `~/.ssh/config`,
         Backup `~/.ssh/config.linascp.bak`; `ssh <LinaSCP-Host>` funktioniert.
   - [ ] Spiegeln beenden → Datei und `Include`-Zeile entfernt.
4. Fehler-Screens
   - [ ] Zeitüberschreitung (nicht erreichbare IP), Text nennt den Timeout des Hosts.
   - [ ] Verbindung abgelehnt (Port ohne sshd), Text nennt den Port.
   - [ ] Host nicht gefunden (Tippfehler im Namen).
   - [ ] Anmeldung fehlgeschlagen (falsches Passwort): abgelehnte Methoden, leeres Passwort → „Bitte ein Passwort eingeben.“,
         neues Passwort + „im Profil speichern“ → Verbindung klappt.
   - [ ] Host-Key abgelehnt („Abbrechen“ im Dialog) → zurück zur Übersicht mit Hinweis.
   - [ ] „Versuch n von 3“ zählt weiter; Auto-Retry wartet 2 s / 4 s / 8 s und stoppt danach.
5. Export → Import auf einem zweiten Gerät
   - [ ] Export mit „Geheimnisse einschließen“ (+ Schlüsseldateien), Passphrase doppelt, mindestens 8 Zeichen; Datei hat Rechte `0600`.
   - [ ] Import auf Gerät B: Konfliktliste, Überspringen/Ersetzen/Als Duplikat wirken; falsche Passphrase → „Falsche Passphrase“.
   - [ ] Nach dem Import verbinden die Hosts ohne erneute Passworteingabe; Schlüssel liegen in `~/.ssh/linascp/` (0600).
6. Lokale Terminals
   - [ ] `Ctrl+Shift+T` und „+“ öffnen „Lokales Terminal“ mit `$SHELL` im Home-Verzeichnis, `TERM=xterm-256color`.
   - [ ] `exit` → „Sitzung beendet“ mit „Schließen“.
7. Automatische Tests
   - [ ] Alle Suiten aus Abschnitt 4 grün, inklusive Docker-Integrationstests.

## 2. Screens (jeweils Hell und Dunkel)

| # | Screen | Hell | Dunkel | Prüfpunkte |
|---|---|---|---|---|
| 1 | Server-Übersicht | [ ] | [ ] | Gruppen mit Zählern, Login-Profile mit „verwendet von“, Filter Alle/SSH/SFTP, Suche (`Ctrl+K` fokussiert), Quick Connect `user@host:port` (auch `[::1]:2222`), Karten mit SSH/SFTP-Chips, Detail-Panel (Adresse, Port, Profil, Jump-Host, Startpfad, Port-Forwards), „Dateien“ ausgegraut mit Tooltip, Löschen mit Bestätigung, leerer Zustand |
| 2 | Editor – Host | [ ] | [ ] | Reiter Allgemein/Authentifizierung/Verbindung/SFTP, Pflichtfelder, Port 1–65535, Zahlenfelder ≥ 0, Env `KEY=VALUE`, „Speichern“ gesperrt bei Fehlern, neue Gruppe, Farbwahl, Port-Forward-Regeln, „Verbindung testen“ mit Schritten und Ergebnis |
| 3 | Editor – Login-Profil | [ ] | [ ] | Sprung aus dem Auth-Reiter, „Zurück zum Host“ behält Eingaben, gespeichertes Profil ist im Host ausgewählt, Hinweis „Änderungen gelten für alle n Hosts“ |
| 4 | Laden: SSH | [ ] | [ ] | Pulsierender Avatar, Fortschrittsbalken, Schrittliste mit Meta-Texten, „▸ Details anzeigen“ zeigt `ssh -v`-Log, Abbrechen (auch `Esc`) bricht sofort ab, Rail markiert „Terminal“ |
| 5 | Unbekannter Host-Key | [ ] | [ ] | Tab „· wartet auf Bestätigung“, SHA256/MD5-Umschalter, Kopieren → „Kopiert“, Nur dieses Mal / Vertrauen und verbinden / Abbrechen |
| 6 | Host-Key geändert | [ ] | [ ] | Roter Rahmen, Vergleich Gespeichert/Aktuell, „Technische Details“ mit OpenSSH-Warnung, „Verbindung abbrechen“ fokussiert und dominant, Ersetzen erst nach Häkchen |
| 7 | Verbindungsfehler | [ ] | [ ] | Varianten Zeitüberschreitung/Abgelehnt/Nicht gefunden mit Ursachen und Originalmeldung, „Neuer Versuch …“ während des Retrys, Auto-Retry-Häkchen, Host bearbeiten, Zur Übersicht |
| 8 | Anmeldung fehlgeschlagen | [ ] | [ ] | Tab „· nicht angemeldet“, Methodenliste, Passwortvalidierung, „Login-Profil bearbeiten“ öffnet das Profil |
| 9 | Terminal | [ ] | [ ] | Tab-Leiste (aktiver Tab mit Akzentlinie oben), deckender Hintergrund, JetBrains Mono, Größenänderung passt Zeilen an, Markieren kopiert, Mittelklick fügt genau einmal ein, `Ctrl+Shift+C/V`, Links klickbar, „Sitzung beendet“ mit Neu verbinden |
| 10 | Einstellungen / Erststart / Export / Import | [ ] | [ ] | Theme, ssh-Modus mit Bestätigungstext (Include-Zeile + Backup), Renderer Auto/WebGL/DOM, Schriftgröße, Kompatibilitäts-Rendering (wirkt nach Neustart), Export-/Import-Dialoge |

## 3. Querschnitt

- [ ] Tastatur: `Ctrl+K`, `Ctrl+N`, `Ctrl+Shift+T`, `Ctrl+,`; im Terminal bleiben `Ctrl+K`/`Ctrl+N` beim Terminal.
- [ ] Bedienung nur mit Tastatur möglich (Tab-Reihenfolge, Fokus sichtbar, Dialoge fangen den Fokus, `Esc` schließt).
- [ ] Screenreader (Orca): Icon-Buttons haben Namen, Reiter/Radiogruppen werden angesagt.
- [ ] Kontrast ≥ 4,5:1 in beiden Modi (Stichprobe mit einem Kontrastprüfer).
- [ ] Renderer: „WebGL“ flüssig; „DOM“ funktioniert; 20 Tabs öffnen und wechseln → kein schwarzer Terminal-Inhalt (WebGL nur am sichtbaren Tab).
- [ ] Durchsatz: `yes | head -c 500M` bzw. `cat` großer Datei – UI bleibt bedienbar, Speicher stabil (Flow-Control).
- [ ] Kompatibilitäts-Rendering an → nach Neustart `WEBKIT_DISABLE_DMABUF_RENDERER=1` in `/proc/<pid>/environ`.
- [ ] Keine Geheimnisse in Logs: `grep -ri '<testpasswort>' ~/.local/state/linascp/logs` findet nichts.
- [ ] Gesperrter Schlüsselbund: Verbindung trotzdem möglich (Abfrage), Hinweis „Speichern … derzeit nicht möglich“.

## 4. Automatische Tests (Referenz)

- [ ] `cargo test --workspace`
- [ ] `cargo test -p linascp-core --test ssh_integration -- --ignored --test-threads=1`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `bun run typecheck`
- [ ] `bun run test`

## 5. Paketierung

- [ ] `.deb` installiert, App im Startmenü unter „Netzwerk“, startet; Deinstallation sauber.
- [ ] AppImage startet auf einem zweiten Gerät ohne Installation.
- [ ] Hinweis: Enthält der Desktop-Eintrag im AppImage nicht `RemoteAccess` in `Categories`, ist das rein kosmetisch
      (AppImages werden nicht ins Menü installiert).

Ergebnis: ☐ abgenommen ☐ Nacharbeit nötig – Notizen: ______________________________________
