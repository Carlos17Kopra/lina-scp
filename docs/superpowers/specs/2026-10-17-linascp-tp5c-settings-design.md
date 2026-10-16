# LinaSCP TP5c: Einstellungen komplett

Stand: 2026-10-17. Teil 3 von TP5 (Aufteilung in `2026-10-16-linascp-tp5a-tunnels-design.md`).

Der Nutzer hat auf die Fragerunde verzichtet („leg mit 5c los“). Alle Entscheidungen unten sind Defaults des Autors,
nicht abgestimmt.

## Ziel

- Aus dem minimalen Einstellungsdialog (TP1) wird ein Rail-Screen „Einstellungen“.
- Werte, die bisher fest im Code stehen, werden einstellbar: Scrollback, Cursor, Shell lokaler Terminals und die
  Standardwerte neuer Hosts.

Nicht enthalten:
- Schriftart des Terminals (Fonts sind nicht zuverlässig vorhanden).
- known_hosts verwalten, Protokoll-Ordner öffnen, Tastenkürzel ändern.
- Einstellungen je Host über das hinaus, was der Host-Editor schon kann.

## 1. Screen

- Das Zahnrad in der Rail öffnet die Ansicht `settings` (kein Dialog mehr). Abschnitte untereinander, jede Änderung
  wird sofort gespeichert (wie bisher), Textfelder beim Verlassen.
- **Darstellung:** Design (Hell/Dunkel).
- **Terminal:** Renderer, Schriftgröße, Farbschema, Kompatibilitätsmodus, Auto-Reconnect (wie bisher), neu:
  - Scrollback: 1 000 / 5 000 / 10 000 (Standard) / 50 000 / 100 000 Zeilen.
  - Cursor: Block (Standard) / Balken / Unterstrich; „Cursor blinkt“ (an).
  - Änderungen an Scrollback und Cursor gelten sofort auch in offenen Terminals.
- **Lokales Terminal:** Shell (leer = `$SHELL`, sonst `/bin/bash`). Absoluter Pfad, eine Zeile; eine nicht
  ausführbare Datei wird beim Speichern abgelehnt. Gilt für neu geöffnete lokale Terminals.
- **Neue Hosts:** Benutzer (leer = keiner), Port (22), Keepalive (30 s, 0 = aus), Timeout (15 s, 0 = keins). Der
  Host-Editor setzt sie bei „Neuer Host“ ein; Quick Connect und ssh-config-Import bleiben, wie sie sind.
- **Dateien:** Editor-Befehl (wie bisher).
- **~/.ssh:** Integrieren/Spiegeln mit Bestätigung (wie bisher).
- **Daten:** Export, Import (wie bisher, als Dialoge).

## 2. Speichern und Prüfen (Tauri `settings.rs`)

- Neue Felder in `Settings`: `terminal_scrollback: u32`, `terminal_cursor: block|bar|underline`,
  `terminal_cursor_blink: bool`, `local_shell: Option<String>`, `new_host_user: Option<String>`,
  `new_host_port: u16`, `new_host_keepalive_s: u32`, `new_host_timeout_s: u32`.
- Lesen: ungültige oder fehlende Werte fallen auf den Standard zurück (wie bisher).
- Schreiben: ungültige Werte werden abgelehnt (Fehler mit Grund), nichts wird teilweise gespeichert.
  - Scrollback nur aus der Liste oben. Port 1–65535, Keepalive 0–3600, Timeout 0–600.
  - Benutzer: wie im Host-Editor (`validate.rs`, darf in die ssh-config gespiegelt werden).
  - Shell: absolut, ohne Steuerzeichen, höchstens 1024 Zeichen, existiert und ist ausführbar.
- Export: Scrollback, Cursor, Blinken und die Standardwerte neuer Hosts reisen mit; die Shell nicht
  (gerätespezifisch).

## 3. Tests

- Tauri: Standardwerte, Speichern und Lesen der neuen Felder, Ablehnen ungültiger Werte (ohne teilweises Speichern),
  ungültige gespeicherte Werte fallen zurück, die Shell-Einstellung wird für lokale Terminals benutzt.
- Core: Export/Import nimmt die neuen Schlüssel mit, die Shell nicht.
- UI: Rail öffnet den Screen; jede neue Einstellung speichert; Terminal übernimmt Scrollback/Cursor auch offen;
  „Neuer Host“ übernimmt die Standardwerte.
- Abnahme: `docs/acceptance-tp5c.md`.
