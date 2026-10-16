# LinaSCP TP4a: SFTP – Rechte, Bandbreite, SFTP-Server-Befehl

Stand: 2026-10-16. Teil 1 von TP4 („SFTP für Fortgeschrittene“). Baut auf TP3a–3c auf
(`2026-10-15-linascp-tp3a-sftp-browse-design.md`, `2026-10-15-linascp-tp3b-transfers-design.md`,
`2026-10-16-linascp-tp3c-bookmarks-edit-design.md`).

Der Nutzer hat auf die Fragerunde verzichtet („Leg ohne Fragen los, ich vertraue dir“). Alle Entscheidungen unten
sind Defaults des Autors, nicht abgestimmt.

## Aufteilung von TP4

Die früheren Specs schieben diese Punkte nach TP4: Synchronisieren, Vergleichen, Aktuell halten, Rechte,
Bandbreite, `sftpServerCmd` (sudo), Übertragungsmodus Text/Auto, direktes Überschreiben ohne `.filepart`.
Das sind mehrere unabhängige Vorhaben:

- **4a (dieses Dokument):** Rechte ändern, Bandbreitenlimit, SFTP-Server-Befehl, Übertragungsmodus entfällt.
  Damit wirken alle Felder im Host-Tab „SFTP“.
- **4b:** Ordner vergleichen und synchronisieren.
- **4c:** Remote-Ordner aktuell halten (lokale Änderungen automatisch hochladen).
- **Später, bei Bedarf:** direktes Überschreiben ohne `.filepart`.

## Ziel

- Rechte einer Datei oder eines Ordners lassen sich in beiden Panels ansehen und ändern, auch für mehrere Einträge
  und auf Wunsch rekursiv.
- Das Bandbreitenlimit des Hosts bremst alle Übertragungen eines SFTP-Tabs.
- Mit dem SFTP-Server-Befehl (z. B. `sudo /usr/lib/openssh/sftp-server`) arbeitet der Tab mit anderen Rechten.

Nicht enthalten:
- Besitzer oder Gruppe ändern (chown). Das braucht fast immer root und numerische IDs; mit dem SFTP-Server-Befehl
  als root kommt es bei Bedarf.
- Sonderbits (setuid, setgid, sticky). Sie werden angezeigt (Oktalwert), bleiben beim Ändern aber unverändert.
- Abbrechen einer laufenden Rechteänderung.
- Übertragungsmodus Text/Auto (siehe 4).

## 1. Rechte ändern

### Bedienung

- Kontextmenü „Eigenschaften …“ und Taste F9 (wie WinSCP) öffnen den Dialog für die Auswahl, sonst für den Eintrag
  unter dem Cursor. `..` und Namen, die nicht UTF-8 sind, zählen nicht (wie bei allen Aktionen).
- Der Dialog zeigt:
  - Name (bei mehreren: „3 Einträge“), Art, Größe, Besitzer und Änderungsdatum, soweit bekannt.
  - Ein Raster 3 × 3 mit Kontrollkästchen: Zeilen Besitzer / Gruppe / Andere, Spalten Lesen / Schreiben /
    Ausführen.
  - Ein Oktalfeld (z. B. `0644`), synchron mit dem Raster.
  - Bei mindestens einem Ordner: „Auch für enthaltene Dateien und Ordner“ (aus).
- Mehrere Einträge mit unterschiedlichen Bits: Ein Kästchen ist gemischt (`aria-checked="mixed"`), solange der Nutzer
  es nicht anklickt. Ein Klick schaltet es auf an, dann aus. Gemischte Bits bleiben je Eintrag, wie sie sind.
  Solange ein Bit gemischt ist, bleibt das Oktalfeld leer. Eine Eingabe dort setzt alle Bits fest.
- „Übernehmen“ ist aktiv, sobald sich etwas geändert hat. Danach lädt das Panel neu.
- Ohne bekannten Modus (z. B. kein `permissions` vom Server) starten alle Kästchen gemischt. Ebenso bei einem
  Symlink: die Liste zeigt den Modus des Links (`0777`), geändert wird aber das Ziel.
- Gesendet werden nur die angeklickten Bits; alle anderen bleiben je Eintrag. So setzt „Gruppe: Schreiben“ rekursiv
  nur dieses Bit und macht keine Datei ausführbar. Nach einer Eingabe im Oktalfeld gilt der ganze Modus, ebenso wenn
  nur „rekursiv“ angehakt und nichts angeklickt wurde (den gezeigten Modus nach unten übernehmen).

### Regeln

- Die Änderung ist ein Paar Masken: `set` und `clear` (je nur Bits aus `0o777`). Neuer Modus je Eintrag:
  `(alt & !clear) | set`, die oberen Bits (Typ, Sonderbits) bleiben.
- Rekursiv: Jeder Ordner (auch der ausgewählte) bekommt zusätzlich Ausführen überall dort, wo Lesen gesetzt ist
  (`0644` → Ordner `0755`). Das ist dieselbe Regel wie „Rechte beim Upload“. Dateien bekommen genau den Modus.
  So macht `0644` rekursiv weder Ordner unbetretbar noch Dateien ausführbar.
- Symlinks:
  - Ein ausgewählter Symlink ändert das Ziel (wie `chmod`, SFTP `setstat` folgt Links).
  - Beim rekursiven Abstieg werden Symlinks übersprungen, weder gefolgt noch geändert, damit nichts außerhalb des
    Baums berührt wird.
- Fehler: Ein Eintrag, der nicht geändert werden kann, bricht nicht ab. Am Ende meldet das Panel „N Einträge
  konnten nicht geändert werden“ mit dem ersten Pfad und Grund. Ein Verbindungsverlust bricht ab.
- Reihenfolge rekursiv: Gewährt der neue Modus dem Besitzer `r` oder `x`, die vorher fehlten, wird erst der Ordner
  geändert, dann abgestiegen (ein `0644`-Ordner lässt sich auflisten, aber nicht betreten). Sonst erst die Kinder,
  zuletzt der Ordner (so geht ein Entzug von `r`/`x` nicht vor dem Abstieg verloren). Schlägt das Auflisten fehl,
  wird erst der Ordner geändert und dann erneut aufgelistet.
- Lokal gilt dasselbe (`std::fs::set_permissions`, Abstieg mit `symlink_metadata`).

### Core

- `transfer::Node` bekommt `link: bool` (Pfad selbst ist ein Symlink), gesetzt in `LocalSide` und `SftpSession`.
- Neu `crates/core/src/transfer/chmod.rs`:
  - `pub struct ModeChange { pub set: u32, pub clear: u32 }`
  - `pub struct ChmodFailure { pub path: String, pub error: CoreError }`
  - `pub async fn chmod(side: &dyn Side, paths: &[String], change: ModeChange, recursive: bool)
    -> Result<Vec<ChmodFailure>>`. `Err` nur bei verlorener Verbindung (Netzwerkfehler).
  - Benutzt nur `Side::stat`, `Side::children`, `Side::set_mode`. Kindnamen mit `lossy` werden übersprungen,
    Kindnamen werden mit `validate_file_name` geprüft.
- `ModeChange` wird geprüft: Bits außerhalb `0o777` → `Validation`.

### Tauri

- `sftp_chmod(session_id, paths, change, recursive) -> ChmodReport`, `local_chmod(paths, change, recursive) ->
  ChmodReport`. `ChmodReport { failed: u32, first: Option<ChmodFailureDto> }` mit Pfad und Meldung.

## 2. Bandbreitenlimit

- Host-Feld „Bandbreitenlimit (KB/s)“ (`bandwidth_limit_kbps`), 1 KB = 1024 Bytes. Leer oder 0 = unbegrenzt.
- Das Limit gilt für den ganzen SFTP-Tab: alle parallelen Dateien zusammen, Uploads und Downloads zusammen, auch
  das Zurückladen beim Bearbeiten.
- Umsetzung: `TransferOptions` bekommt `bandwidth: Option<u64>` (Bytes/s). Die Queue hält einen Taktgeber
  (`Pacer`): Jeder Block reserviert vor dem Lesen `len / rate` Sekunden auf einer gemeinsamen Zeitachse und wartet,
  bis sein Platz dran ist. Kein Burst über einen Block hinaus. Ein Abbruch unterbricht das Warten. Mit Limit reserviert
  jede Datei höchstens etwa eine Sekunde im Voraus (`rate / CHUNK` Blöcke, mindestens 1), damit die Reservierungen
  einer abgebrochenen Datei die anderen nicht aufhalten.
- Eine Änderung am Host wirkt für neu geöffnete SFTP-Tabs (wie Parallelität und Rechte beim Upload heute).

## 3. SFTP-Server-Befehl

- Host-Feld „SFTP-Server (z. B. mit sudo)“ (`sftp_server_cmd`). Leer = Subsystem `sftp` (wie bisher).
- Gesetzt: Statt `request_subsystem("sftp")` startet LinaSCP den Befehl per `exec` auf dem Kanal und spricht SFTP
  über dessen stdin/stdout. Typisch: `sudo -n /usr/lib/openssh/sftp-server`.
- Fragt `sudo` nach einem Passwort, endet der Befehl (ohne Terminal kein Prompt) oder es kommt keine SFTP-Antwort.
  Der Schritt „Subsystem“ schlägt dann fehl, die Meldung nennt den Befehl als Ursache. Es wird nie ein Passwort
  in den Kanal geschrieben.
- Validierung in `validate.rs` beim Speichern: höchstens 1024 Zeichen, keine Steuerzeichen (insbesondere kein
  Zeilenumbruch). Ein ungültiger Wert in der DB fällt beim Verbinden nicht auf das Subsystem zurück, sondern
  schlägt fehl (fail closed), damit nie still mit anderen Rechten gearbeitet wird als eingestellt.
- Gilt auch für SFTP-Reconnects, nicht für Terminal-Verbindungen.
- Das Verbindungsprotokoll zeigt `debug1: Starting SFTP server command` statt des Befehls selbst (er kann
  Pfade enthalten, aber nie Geheimnisse; trotzdem knapp halten).

## 4. Übertragungsmodus entfällt

- Text/Auto würden Zeilenenden umwandeln. Lokal ist Linux (LF), die Server sind praktisch immer Unix (LF): Die
  Umwandlung wäre fast immer ein No-op, bräche aber Fortsetzen und Fortschritt (Größe ≠ übertragene Bytes).
- Das Feld verschwindet aus Modell, UI und Bindings. Alte DB-Zeilen und `.linascp`-Exporte mit `transferMode`
  werden weiter gelesen (serde ignoriert das Feld).
- Kommt zurück, falls ein Windows-Server mit CRLF gebraucht wird.

## 5. Host-Tab „SFTP“

- Der Hinweis wird zu: „Diese Einstellungen gelten für neu geöffnete SFTP-Tabs.“
- Das Feld „Übertragungsmodus“ entfällt.

## 6. Invarianten

- Rechteänderungen folgen beim Abstieg nie Symlinks.
- Der SFTP-Server-Befehl wird nie still ersetzt: ungültig oder fehlgeschlagen heißt Verbindungsfehler.
- Bestehende Invarianten (Geheimnisse, Host-Keys, ein terminales `ConnectEvent`, 0600/0700) bleiben.

## 7. Tests

- Core-Unit: `chmod` mit Masken, gemischten Modi, rekursiv mit Ordner-x-Regel, übersprungenen Symlinks (auch
  Symlink auf Ordner außerhalb), Fehler je Eintrag sammelt und läuft weiter, ungültige Masken.
- Core-Unit: `Pacer` hält die Rate (mit `tokio::time::pause`), Abbruch während des Wartens.
- Core-Unit: `TransferOptions::from_settings` für Bandbreite (leer, 0, Wert).
- Core-Unit: `validate` für `sftp_server_cmd`.
- Docker: `sftp_chmod` remote, Verbindung mit `sftp_server_cmd` (Pfad des Servers im Testcontainer), falscher
  Befehl schlägt im Schritt Subsystem fehl.
- UI: Dialog (Raster ↔ Oktal, gemischt, rekursiv nur bei Ordnern, Übernehmen erst nach Änderung, Fehlermeldung),
  F9 und Menüeintrag, Host-Tab ohne Übertragungsmodus.
- Abnahme: `docs/acceptance-tp4a.md`.

## 8. Dem Umsetzenden überlassen

- Layout des Dialogs, solange Raster, Oktalfeld und Kästchen „rekursiv“ da sind.
