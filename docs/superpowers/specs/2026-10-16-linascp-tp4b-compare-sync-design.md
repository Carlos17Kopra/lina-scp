# LinaSCP TP4b: SFTP – Vergleichen und Synchronisieren

Stand: 2026-10-16. Teil 2 von TP4 (Aufteilung in `2026-10-16-linascp-tp4a-permissions-bandwidth-design.md`). Baut
auf der Transfer-Engine aus TP3b auf.

Der Nutzer hat auf die Fragerunde verzichtet („leg mit 4b los“, vorher „ich vertraue dir“). Alle Entscheidungen
unten sind Defaults des Autors, nicht abgestimmt.

## Ziel

- Der lokale und der Remote-Ordner der beiden Panels lassen sich rekursiv vergleichen.
- Das Ergebnis ist eine Vorschau: was hochgeladen, heruntergeladen oder gelöscht würde. Der Nutzer kann einzelne
  Zeilen abwählen und startet dann die Synchronisation.
- Übertragen wird über die bestehende Queue (Fortschritt, Fortsetzen, Bandbreite, Reconnect).
- Nichts wird überschrieben oder gelöscht, das sich seit dem Vergleich geändert hat.

Nicht enthalten:
- Markierungen der Unterschiede direkt in den Panels (WinSCP „Verzeichnisse vergleichen“). Die Vorschau zeigt sie.
- Vergleich nach Inhalt (Prüfsumme). Verglichen werden Größe und Änderungszeit.
- Gespeicherte Synchronisations-Profile. Nur das Ausschlussmuster wird je Host gemerkt.
- Automatisches Synchronisieren (das ist 4c „Aktuell halten“).

## 1. Bedienung

- Toolbar-Knopf „Synchronisieren“. Er ist aus, solange die Verbindung fehlt. Verglichen werden die aktuellen Ordner
  beider Panels.
- Der Dialog vergleicht beim Öffnen sofort. Während des Vergleichs: „Vergleiche …“ und „Abbrechen“.
- Oben:
  - Richtung (Segmented): „Hochladen“ (lokal → remote), „Herunterladen“ (remote → lokal), „Beide Richtungen“.
    Standard: „Hochladen“.
  - „Auf der Zielseite löschen, was es nur dort gibt“ (aus). Nur bei einer Richtung.
  - „Ausschließen“: Namensmuster, durch Komma getrennt, `*` und `?` als Platzhalter (z. B. `.git, node_modules,
    *.tmp`). Gilt für Datei- und Ordnernamen in jeder Tiefe. Wird je Host gemerkt. „Neu vergleichen“ wendet es an.
- Liste: eine Zeile je Unterschied mit Kästchen, Aktion („Hochladen“, „Herunterladen“, „Löschen lokal“, „Löschen
  remote“ oder „Übersprungen“ mit Grund), relativem Pfad sowie Größe und Datum je Seite. Übersprungene Zeilen sind
  nicht wählbar.
- Unten: Zusammenfassung („3 hochladen, 1 löschen“) und „Synchronisieren“. Mit Löschungen auf dem Server steht
  darüber rot: „N Einträge werden auf dem Server endgültig gelöscht.“ Lokale Löschungen gehen in den Papierkorb.
- Keine Unterschiede: „Beide Ordner sind gleich.“
- Der Dialog hält die beiden Ordner und die Sitzung vom Öffnen fest. Ändert sich „Ausschließen“, ist
  „Synchronisieren“ aus, bis neu verglichen wurde.
- „Synchronisieren“ schließt den Dialog. Die Übertragungen erscheinen im Übertragungs-Panel, nach Löschungen
  laden die Panels neu.

## 2. Vergleich (Core, `crates/core/src/transfer/compare.rs`)

- `compare(local: &dyn Side, remote: &dyn Side, local_dir, remote_dir, exclude: &[String], cancel) ->
  Result<Vec<Difference>>`.
- Abstieg in Ordner, die auf beiden Seiten Ordner sind. Ein Ordner, den es nur auf einer Seite gibt, ist **eine**
  Zeile (die Queue überträgt ihn samt Inhalt).
- Gleich ist eine Datei, wenn die Größe gleich ist und die Änderungszeiten höchstens 1 s auseinander liegen (oder
  eine fehlt). Sonst:
  - Zeiten verschieden: `localNewer` bzw. `remoteNewer`.
  - Zeiten gleich, Größe verschieden: `sizeDiffers`.
- `onlyLocal`, `onlyRemote`, `typeMismatch` (Datei gegen Ordner).
- Symlinks auf Dateien zählen als Dateien (wie in der Queue). Symlinks auf Ordner, kaputte Links und Sonderdateien
  auf **einer** Seite blenden den Namen auf **beiden** Seiten aus: so wird nie etwas gelöscht oder überschrieben,
  das auf der anderen Seite als Link existiert.
- Namen, die nicht UTF-8 sind, und ungültige Namen vom Server werden ausgeblendet. Ausschlussmuster ebenso.
- Kann ein Ordner auf einer Seite nicht gelesen werden, bricht der Vergleich mit diesem Fehler ab (fail closed: ein
  unlesbarer Ordner darf nie als „leer“ gelten und so Löschungen auslösen).
- `cancel` wird zwischen den Ordnern geprüft, dann `Cancelled`.
- `Difference { rel, status, local: Option<NodeInfo>, remote: Option<NodeInfo>, contents: Option<DirContents> }`,
  `NodeInfo { dir, size, mtime }`. `rel` ist relativ zu den Startordnern, mit `/` getrennt.
- Für einen Ordner, den es nur auf einer Seite gibt, enthält `contents` den ganzen Baum darunter (mit denselben
  Ausschlüssen) und `hidden`, falls darin etwas ausgeblendet wurde (Ausschluss, Link, Sonderdatei, unbrauchbarer
  Name). Die Vorschau zeigt „Ordner · N Einträge“.

## 3. Plan (UI, `ui/src/screens/sftp/syncPlan.ts`)

Aktion je Zeile aus Status, Richtung und „Löschen“:

| Status | Hochladen | Herunterladen | Beide Richtungen |
|---|---|---|---|
| onlyLocal | hochladen | Löschen lokal, sonst übersprungen | hochladen |
| onlyRemote | Löschen remote, sonst übersprungen | herunterladen | herunterladen |
| localNewer | hochladen | übersprungen („lokal neuer“) | hochladen |
| remoteNewer | übersprungen („remote neuer“) | herunterladen | herunterladen |
| sizeDiffers | hochladen | herunterladen | übersprungen („gleiches Datum, andere Größe“) |
| typeMismatch | übersprungen | übersprungen | übersprungen |

Alle wählbaren Zeilen sind anfangs gewählt. Ein zu löschender Ordner mit `hidden` wird übersprungen („enthält
ausgeschlossene oder besondere Einträge“): sonst verschwände, was die Vorschau nie gezeigt hat.

## 4. Ausführen

- Übertragungen: `TransferQueue::enqueue_expecting(direction, sources, target_dir, expected)`. Das ist ein Batch je
  Zielordner. `expected` ordnet Zielpfaden den Stand (`FileStamp`) zu, den der Vergleich gesehen hat.
  - Ist das Ziel noch genau so, wird ohne Frage überschrieben.
  - Ist es anders oder neu aufgetaucht, öffnet sich der normale Konfliktdialog.
  - Sync-Batches sind wie normale Batches wiederholbar.
  - Ausschlussmuster gelten auch beim Übertragen ganzer Ordner (die Queue lässt passende Namen beim Auflisten weg).
  - Auch ein nach Verbindungsabbruch fortgesetzter Auftrag prüft das Ziel erneut.
- Löschen (Tauri `sync_delete`): je Eintrag erst neu `stat`.
  - Datei: nur wenn der Stand noch der aus dem Vergleich ist, sonst „übersprungen (geändert)“.
  - Ordner: nur wenn sein ganzer Baum (mit denselben Ausschlüssen neu gelesen) genau der aus dem Vergleich ist und
    nichts darin ausgeblendet wird. Remote endgültig (`SftpSession::delete`), lokal in den Papierkorb.
  - Ergebnis: Zahl der Fehler, erster Fehler, Zahl der übersprungenen Einträge. Die UI zeigt das als Hinweis.
- Relative Pfade vom Frontend werden im Core geprüft: jedes Segment mit `validate_file_name`, nie leer, nie
  absolut.

## 5. Tauri

- `sync_compare(session_id, local_dir, remote_dir, exclude) -> Vec<Difference>`. Pro Sitzung läuft höchstens ein
  Vergleich; ein neuer bricht den alten ab.
- `sync_cancel(session_id)`.
- `sync_enqueue(session_id, direction, local_dir, remote_dir, items: Vec<SyncItem>) -> Vec<BatchId>`, mit
  `SyncItem { rel, expected: Option<FileStamp> }`.
- `sync_delete(session_id, side, base_dir, items: Vec<SyncItem>) -> SyncDeleteReport`.

## 6. Invarianten

- Ein Ziel, das sich seit dem Vergleich geändert hat, wird nie still überschrieben oder gelöscht.
- Ein unlesbarer Ordner bricht den Vergleich ab und gilt nie als leer.
- Ordner-Links und Sonderdateien werden nie als „fehlt“ behandelt.
- Lokales Löschen geht in den Papierkorb, Remote-Löschen nur nach roter Ansage in der Vorschau.

## 7. Tests

- Core-Unit `compare`:
  - alle Status
  - 1-s-Toleranz
  - einseitiger Ordner als eine Zeile
  - Ausschlussmuster in der Tiefe
  - Ordner-Link auf einer Seite blendet beidseitig aus
  - lossy und feindliche Namen
  - unlesbarer Ordner bricht ab
  - Abbruch
- Core-Unit Queue: `enqueue_expecting` überschreibt bei passendem Stand ohne Konflikt; ein geänderter oder neu
  aufgetauchter Zielstand führt zum Konflikt.
- Core-Unit: Prüfung relativer Pfade, Gruppierung je Zielordner.
- Tauri: `sync_delete` lokal (Papierkorb, geänderte Datei übersprungen).
- Docker: Vergleich gegen den Testserver mit Unterordnern, danach `sync_enqueue` hochladen und erneut vergleichen:
  gleich.
- UI:
  - `planSync` (Tabelle oben)
  - Dialog: Vergleichen und Abbrechen, Richtung wechseln, Zeilen abwählen, rote Ansage, Ausführen ruft
    Enqueue/Delete mit den richtigen Daten, „gleich“-Zustand, Fehler des Vergleichs.
- Abnahme: `docs/acceptance-tp4b.md`.

## 8. Dem Umsetzenden überlassen

- Layout der Vorschau, solange die Spalten aus 1 da sind.
- Nebenläufigkeit beim Vergleich (parallele `stat`s wie in der Queue).
