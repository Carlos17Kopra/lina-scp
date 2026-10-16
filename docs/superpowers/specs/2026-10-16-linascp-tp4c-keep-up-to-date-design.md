# LinaSCP TP4c: SFTP – Remote-Ordner aktuell halten

Stand: 2026-10-16. Teil 3 von TP4 (Aufteilung in `2026-10-16-linascp-tp4a-permissions-bandwidth-design.md`). Baut
auf der Queue (TP3b) und dem Vergleich (TP4b) auf.

Der Nutzer hat auf die Fragerunde verzichtet. Alle Entscheidungen unten sind Defaults des Autors, nicht abgestimmt.

## Ziel

- Der lokale Ordner des linken Panels wird beobachtet. Jede neue oder geänderte Datei darin wird automatisch in den
  Remote-Ordner des rechten Panels hochgeladen (WinSCP „Remote-Verzeichnis aktuell halten“).
- Übertragen wird über die bestehende Queue (Fortschritt, Bandbreite, Reconnect, Konfliktdialog).
- Nichts auf dem Server wird still überschrieben, das sich dort seit dem letzten bekannten Stand geändert hat.

Nicht enthalten:
- Löschungen übertragen. Lokal gelöschte Dateien bleiben auf dem Server.
- Ein erster Abgleich beim Start. Hochgeladen wird, was sich **ab dem Start** ändert. Wer vorher angleichen will,
  nutzt „Synchronisieren“ (TP4b).
- Die Gegenrichtung (Remote beobachten).
- inotify. Es wird alle 2 s abgefragt, wie beim Bearbeiten (TP3c).

## 1. Bedienung

- Toolbar-Knopf „Aktuell halten“. Er ist aus, solange die Verbindung fehlt.
- Ein Dialog zeigt beide Ordner, das Feld „Ausschließen“ (dasselbe Muster wie beim Synchronisieren, je Host
  gemerkt) und den Hinweis „Lokale Änderungen werden ab jetzt hochgeladen. Gelöschtes bleibt auf dem Server.“
  „Starten“ beginnt.
- Solange es läuft, steht über dem Übertragungs-Panel eine Leiste: „Hält <remote> aktuell · N hochgeladen“ und
  „Beenden“. Ein Fehler beim Lesen des lokalen Ordners steht rot in der Leiste; es wird weiter versucht.
- Je SFTP-Tab läuft höchstens ein „Aktuell halten“. Es endet mit „Beenden“ und beim Schließen des Tabs.
  Bei einem Verbindungsabbruch läuft es weiter; die Queue überträgt nach dem Reconnect.

## 2. Ablauf (Core, `crates/core/src/transfer/keep.rs`)

- Start: lokalen Baum lesen (`walk` aus TP4b, mit Ausschlüssen) als Ausgangsstand, Remote-Baum lesen als bekannter
  Serverstand. Schlägt eins davon fehl, schlägt der Start fehl.
- Alle 2 s: lokalen Baum neu lesen.
  - Eine Datei ist bereit, wenn ihr Stand (Größe, Änderungszeit) anders ist als der zuletzt hochgeladene bzw.
    der Ausgangsstand und sich seit dem letzten Lesen nicht mehr geändert hat (ein Takt Ruhe, damit halb
    geschriebene Dateien nicht übertragen werden).
  - Ein neuer Ordner wird als **ein** Eintrag hochgeladen (die Queue überträgt ihn samt Inhalt), sobald nichts
    darin sich im letzten Takt geändert hat. Einträge darunter zählen dann als hochgeladen.
  - Die bereiten Einträge gehen über `enqueue_sync` in die Queue, mit dem bekannten Serverstand als `expected`:
    ist das Ziel noch so, wird ohne Frage überschrieben; sonst öffnet sich der normale Konfliktdialog.
  - Danach gilt der lokale Stand als hochgeladen und als bekannter Serverstand (Uploads übernehmen die
    Änderungszeit).
- Ein Lesefehler lokal: Zustand „Fehler“ mit dem Fehler, alter Stand bleibt, nächster Takt versucht es wieder.
  Ein Fehler beim Einreihen ebenso.
- Ereignis `KeepEvent::State { local_dir, remote_dir, uploaded, error }` bei jeder Änderung.

## 3. Tauri

- `keep_start(session_id, local_dir, remote_dir, exclude)`: ein laufendes wird vorher beendet. Ereignisse als
  `SftpEvent::Keep { event }` über den Kanal des Tabs.
- `keep_stop(session_id)`. `sftp_close` beendet es ebenfalls.

## 4. Invarianten

- Eine Serverdatei, die sich seit dem letzten bekannten Stand geändert hat, wird nie still überschrieben.
- Es wird nie etwas gelöscht.
- Halb geschriebene Dateien (Stand ändert sich noch) werden nicht übertragen.

## 5. Tests

- Core-Unit `keep`: neue und geänderte Datei werden nach einem Ruhetakt hochgeladen; Ausgangsstand wird nicht
  hochgeladen; neuer Ordner als ein Eintrag; Ausschlüsse; auf dem Server geänderte Datei führt zum Konflikt;
  Lesefehler meldet Fehler und läuft weiter; `stop` beendet.
- Tauri: `keep_start`/`keep_stop` mit unbekannter Sitzung.
- UI: Dialog startet mit den Ordnern und dem Muster; Leiste zeigt Zahl und Fehler; „Beenden“ ruft `keepStop`.
- Abnahme: `docs/acceptance-tp4c.md`.
