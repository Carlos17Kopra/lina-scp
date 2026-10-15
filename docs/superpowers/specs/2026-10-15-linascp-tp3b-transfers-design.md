# LinaSCP TP3b: SFTP-Übertragungen – Warteschlange, Konflikte, Wiederverbinden

Stand: 2026-10-15. Teil 2 von TP3 („SFTP-Dateimanager“). Reihenfolge: 3a Verbinden + Navigieren +
Dateioperationen (fertig) → **3b Übertragungen** → 3c Lesezeichen + Bearbeiten.

Vorlage: `shellport-design.zip` → `screens/SFTP.dc.html`, Abschnitt „Übertragungen“ (Warteschlange mit drei
Reitern) und Dialog „Datei existiert bereits“. Baut auf der TP3a-Spec
(`2026-10-15-linascp-tp3a-sftp-browse-design.md`) und dem Reconnect aus TP2d
(`2026-10-15-linascp-tp2d-auto-reconnect-design.md`) auf.

## Ziel

- Dateien und Ordnerbäume werden zwischen dem lokalen und dem Remote-Panel übertragen: per `F5`, Kontextmenü,
  Toolbar-Knopf oder Drag & Drop.
- Dateien aus Nautilus bzw. vom Desktop lassen sich auf das Remote-Panel ziehen und werden hochgeladen.
- Eine Warteschlange pro SFTP-Tab zeigt Fortschritt, Geschwindigkeit und Status. Einzelne Dateien und ganze Aufträge
  lassen sich abbrechen, fehlgeschlagene Dateien wiederholen.
- Existiert das Ziel schon, fragt ein Konfliktdialog: Überspringen, Nur wenn neuer, Überschreiben, Abbrechen,
  optional für alle weiteren Konflikte des Auftrags.
- Uploads setzen die Rechte aus der Host-Einstellung „Rechte beim Upload“.
- Bricht die Verbindung ab, verbindet sich die SFTP-Sitzung automatisch neu (Regeln aus TP2d). Unterbrochene Dateien
  machen danach dort weiter, wo sie aufgehört haben.
- Halb übertragene Dateien liegen nie unter dem Zielnamen: Übertragen wird in `name.filepart`, umbenannt wird erst
  am Ende.

Nicht enthalten:
- Ziehen aus der App nach Nautilus (Wayland/Tauri können das nicht nativ).
- Verschieben (`F6`), Remote ↔ Remote, Lokal ↔ Lokal.
- Übertragungsmodus Text/Auto (Zeilenenden), Bandbreitenlimit, `sftpServerCmd` (TP4). Übertragen wird immer binär.
- Warteschlange über App-Neustarts hinweg. Sie lebt nur im Speicher.
- Eine gemeinsame Warteschlange über alle Tabs. Jeder SFTP-Tab hat seine eigene.
- Pausieren per Knopf. Pausiert wird nur bei Verbindungsverlust.

## 1. Core (`crates/core/src/transfer/`, `crates/core/src/connect/`)

### Modell

- **Auftrag (`Batch`)**: entsteht aus einem `F5`-, Kontextmenü-, Toolbar- oder Drop-Vorgang. Felder: `id`,
  `direction` (`upload` | `download`), `sources` (absolute Pfade auf der Quellseite), `target_dir` (absoluter Pfad
  auf der Zielseite), Konflikt-Vorgabe (`None` oder die Entscheidung aus „Für alle übernehmen“).
- **Job**: eine Datei oder ein Ordner eines Auftrags. Felder: `id`, `batch_id`, `direction`, `source` (absolut),
  `target` (absolut), `rel` (Pfad relativ zum Auftrag, für die Anzeige), `kind` (`file` | `dir`), `size`, `done`
  (übertragene Bytes), `state`, `error?`.
- **Status (`JobState`)**: `queued`, `running`, `waitingDecision`, `paused`, `done`, `skipped`, `failed`,
  `cancelled`.
- Ordner werden beim Abarbeiten expandiert. Ist ein Ordner-Job dran, wird der Zielordner angelegt (ein vorhandener
  Ordner ist in Ordnung, die Inhalte werden zusammengeführt). Danach kommen seine Einträge als Jobs in die Schlange,
  direkt hinter die Jobs, die schon drinstehen. Ein Ordner-Job ist `done`, sobald er expandiert ist.
- Symlinks bei der Rekursion (beide Seiten): Ein Link auf eine Datei wird als Datei übertragen (Inhalt des Ziels).
  Ein Link auf einen Ordner und ein kaputter Link werden als `skipped` geführt, mit Hinweis. So gibt es keine
  Schleifen. Ein Link, der direkt in `sources` steht, folgt denselben Regeln.
- Sonstige Typen (Socket, FIFO, Gerät) sind `skipped`.
- Namen aus Listings, die nicht verlustfrei UTF-8 sind (`name_lossy`), sind `failed` mit `Config`. Kein
  verlustbehafteter Name erreicht je das Dateisystem.

### Seiten-Abstraktion

- Ein internes Trait `Side` kapselt die Operationen, die die Engine auf Quelle und Ziel braucht: `stat`, `list`,
  `mkdir`, `open_read`, `open_write(offset)`, `read_at`, `write_at`, `set_mode`, `set_mtime`, `rename_over`,
  `remove`.
- Zwei Implementierungen: `LocalSide` (std/tokio-fs) und `RemoteSide` (`SftpSession`, `RawSftpSession`).
- Unit-Tests der Engine laufen mit zwei `LocalSide`s in Tempdirs. Eine Fehler-Injektion (Wrapper um `LocalSide`)
  simuliert Verbindungsverlust und Rechtefehler.

### Engine (`TransferQueue`)

- Gehört zu genau einer SFTP-Sitzung. Höchstens `host.sftp.parallel` Datei-Jobs laufen gleichzeitig (Ordner-Jobs
  zählen nicht mit). Reihenfolge: FIFO über alle Aufträge der Sitzung.
- Datei übertragen:
  - Blöcke à 256 KiB, bis zu 8 Requests gleichzeitig je Datei (Pipelining). Die Antworten werden in Reihenfolge
    geschrieben.
  - Geschrieben wird nach `<target>.filepart` im Zielordner.
  - Am Ende: mtime der Quelle setzen, Rechte setzen (siehe unten), dann `.filepart` auf den Zielnamen umbenennen.
    Remote mit `posix-rename@openssh.com`, falls der Server die Erweiterung anbietet, sonst `remove(target)` und
    `rename`. Lokal mit `rename` (überschreibt atomar).
  - Weitermachen: Hat diese Sitzung einen Job wegen Verbindungsverlust pausiert, geht er nach dem Reconnect ab der
    Größe der `.filepart` weiter. Eine `.filepart`, die nicht aus dieser Sitzung stammt, wird von vorn
    überschrieben.
  - Hat sich die Quelle seit dem Start des Jobs geändert (Größe oder mtime), beginnt er nach dem Reconnect von vorn.
- Rechte:
  - Upload mit gesetztem `upload_perms` (oktal, z. B. `0644`): Dateien bekommen genau diesen Modus. Angelegte
    Ordner bekommen ihn plus `x` überall dort, wo `r` gesetzt ist (`0644` → `0755`).
  - Upload ohne `upload_perms`: Dateien bekommen den Modus der lokalen Datei (`& 0o777`), Ordner den des lokalen
    Ordners.
  - Download: Dateien und Ordner bekommen den Remote-Modus (`& 0o777`), immer mindestens `u+rw` (Ordner `u+rwx`),
    damit die App sie selbst weiter bearbeiten kann. Ein privater Schlüssel mit `0600` bleibt `0600`.
  - Ein vorhandener Zielordner behält seine Rechte.
- Konflikte:
  - Existiert das Datei-Ziel, geht der Job auf `waitingDecision` und ein `conflict`-Event geht raus, mit Größe und
    mtime beider Seiten.
  - Gilt im Auftrag schon eine Vorgabe aus „Für alle übernehmen“, wird sie ohne Event angewendet.
  - Es ist immer nur ein Konflikt je Sitzung offen. Weitere wartende Jobs reihen sich in eine Konflikt-Schlange ein.
    Die übrigen Jobs laufen weiter.
  - Antworten: `overwrite`, `skip` (Job `skipped`), `newer` (überschreiben nur, wenn die Quelle eine neuere mtime
    hat, sonst `skipped`), `cancelBatch` (ganzer Auftrag `cancelled`). Mit `applyToAll` gilt die Antwort
    (außer `cancelBatch`) für alle weiteren Konflikte des Auftrags, auch die schon wartenden.
  - Ist das Ziel ein Ordner und die Quelle eine Datei (oder umgekehrt), wird der Job `failed` mit
    `Fs { kind: AlreadyExists }`.
- Abbrechen pro Job oder pro Auftrag: laufende Übertragungen stoppen, die `.filepart` wird gelöscht, Status
  `cancelled`. Bereits fertige Dateien bleiben.
- Fehler einer Datei (keine Rechte, Platte voll, Quelle verschwunden) machen den Job `failed`, der Rest läuft
  weiter. `retry` setzt einen `failed`-Job zurück auf `queued` (ganz hinten).
- `clear` entfernt alle Jobs in `done`, `skipped` und `cancelled`.
- Fortschritt: je Job `done` in Bytes. Events werden je Job auf höchstens eins pro 250 ms gedrosselt.
  Statuswechsel gehen sofort raus. Die Geschwindigkeit rechnet die UI.
- `close()`: bricht alle Jobs ab, löscht deren `.filepart`-Dateien und beendet die Engine. Danach kommt kein Event
  mehr.

### Wiederverbinden (`connect/sftp_reconnect.rs`, neu)

- `ReconnectingSftp` hält die jeweils aktuelle `SftpSession`, analog zu `ReconnectingSession` aus TP2d.
  Session-ID und Event-Kanal bleiben gleich.
- Ein Connector baut eine neue `SftpSession` ohne Rückfrage auf. Jede Prompt-Art endet mit `inputRequired`.
  Secrets kommen nur aus dem Keyring. Connect-Events der Versuche gehen nicht an die UI.
- Backoff, `MAX_ATTEMPTS` und `is_fatal` werden aus `connect/reconnect.rs` wiederverwendet.
- Ablauf bei `Lost` der inneren Sitzung:
  - Laufende Jobs gehen in jedem Fall auf `paused`. Solange die Sitzung nicht verbunden ist, startet die Engine
    keine weiteren Jobs; `queued`-Jobs bleiben `queued`.
  - Ist Auto-Reconnect aus oder der Fehler fatal, geht sofort `lost` raus.
  - Sonst geht `reconnecting { attempt, maxAttempts, retryAt, reason }` geht raus,
    die Versuche laufen wie in TP2d.
  - Nach einem erfolgreichen Versuch geht `reconnected { info }` raus. Die `paused`-Jobs gehen zurück auf `queued`
    (an ihre alte Position) und machen ab der `.filepart` weiter.
  - Gibt die Sitzung auf, geht `lost { error }` raus. Die Jobs bleiben `paused`.
- `reconnect_now()` startet nach `lost` eine neue Runde bzw. überspringt die Wartezeit. `stop_reconnect()` bricht
  die Runde ab und führt zu `lost`.
- Ein erneuter Abbruch nach erfolgreichem Reconnect startet wieder mit vollen Versuchen.
- Operationen der Panels (`list`, `mkdir`, …) während des Neuaufbaus liefern sofort `Network`.
- Die Auto-Reconnect-Einstellung wird beim Abbruch gelesen, nicht beim Verbinden.

## 2. Tauri (`src-tauri/src/commands/sftp.rs`, `src-tauri/src/commands/transfers.rs`)

### Verbinden und Events

- `connect_sftp` registriert eine `ReconnectingSftp` mit ihrer `TransferQueue`. Der Connector kapselt die Pipeline
  ohne Prompts, wie `connect_ssh` in TP2d.
- `SftpEvent` (bestehender Kanal je Tab) bekommt die Varianten:
  - `reconnecting { attempt, maxAttempts, retryAt, reason }`
  - `reconnected { info }`
  - `lost { error }` (besteht schon, kommt ab jetzt erst, wenn die Sitzung aufgibt)
  - `job { job }`: vollständiger Job bei jedem Statuswechsel und beim Anlegen.
  - `progress { jobId, done }`: gedrosselt.
  - `conflict { jobId, source: FileStamp, target: FileStamp }` mit `FileStamp { size, mtime }`.
  - `batchDone { batchId, targetDir, direction }`: alle Jobs des Auftrags sind in einem Endstatus.
- Ein geordneter Kanal je Tab. Ein Job-Event erreicht die UI nie vor seinem `job`-Event mit dem Anlegen.

### Commands

- `transfer_enqueue(sessionId, direction, sources: Vec<String>, targetDir: String) -> BatchId`
- `transfer_resolve(sessionId, jobId, decision, applyToAll: bool)`
- `transfer_cancel(sessionId, target: { job: JobId } | { batch: BatchId })`
- `transfer_retry(sessionId, jobId)`
- `transfer_clear(sessionId)`
- `sftp_reconnect_now(sessionId)`, `sftp_stop_reconnect(sessionId)`
- `sftp_close` ruft zuerst `close()` der Engine auf (Abbruch, `.filepart` aufräumen), dann schließt es die Sitzung.
- Ein unbekannter `sessionId`, `jobId` oder `batchId` ergibt einen Fehler, nie einen Panic.

### Einstellung

- Das Settings-DTO-Feld `terminalAutoReconnect` heißt jetzt `autoReconnect` und gilt für SSH und SFTP.
  Der DB-Schlüssel `terminal_auto_reconnect` bleibt unverändert, damit gespeicherte Werte ohne Migration gelten.
- Das Label in den Einstellungen heißt „Automatisch neu verbinden (SSH und SFTP)“.
- `t.editor.sftpHint` wird angepasst: Parallelität und Rechte wirken ab jetzt, Modus und Bandbreite ab TP4.

### Validierung

- `sources` und `targetDir` kommen aus der UI. Der Core prüft: absolute Pfade, keine `..`-Komponenten. Alle beim
  Rekursieren gebildeten Namen laufen durch `validate_file_name`.
- `upload_perms` und `parallel` sind schon beim Speichern validiert (`validate.rs`). Die Engine prüft sie trotzdem
  noch einmal und fällt auf „kein Modus“ bzw. `1` zurück, falls ein Wert ungültig ist.

Bindings werden mit `cargo test -p linascp exports_bindings` neu erzeugt.

## 3. Oberfläche

### Zustand (`stores/transfers.ts`, neu)

- Pro SFTP-Tab: Jobs nach ID (in Einfüge-Reihenfolge), die Konflikt-Schlange und Fortschritts-Samples.
- Geschwindigkeit: gleitendes Fenster über 2 s aus den `progress`-Events. Nur für `running`-Jobs, sonst „—“.
- `stores/sftp.ts` leitet die neuen `SftpEvent`-Varianten weiter. `reconnecting`/`reconnected`/`lost` setzen den
  Tab-Status (`"reconnecting"` kommt neu dazu, mit `reconnect`-Infos wie bei Terminal-Tabs).
- Nach `reconnected` laden beide Panels neu. Pfad und Auswahl bleiben erhalten.
- Nach `batchDone` lädt das Ziel-Panel neu, wenn es gerade `targetDir` zeigt.

### Auslösen

- `F5` überträgt die Auswahl (oder den Cursor-Eintrag) des aktiven Panels in den aktuellen Ordner des anderen
  Panels. Die Richtung ergibt sich aus dem aktiven Panel. `..` und Einträge mit `nameLossy` werden ausgelassen.
- Kontextmenü: „Hochladen“ (lokales Panel) bzw. „Herunterladen“ (Remote-Panel). Toolbar: gleicher Knopf.
- Ist die Sitzung nicht `connected`, sind die Auslöser deaktiviert.
- Drag & Drop in der App (HTML5-DnD): Ziehen von Einträgen eines Panels in das andere. Drop auf einen Ordner-Eintrag
  hat diesen Ordner als Ziel, sonst den aktuellen Ordner. Drop ins selbe Panel tut nichts.
- Externer Drop (Tauri `onDragDropEvent`): nur über dem Remote-Panel, mit derselben Ordner-Regel. Über dem lokalen
  Panel oder außerhalb passiert nichts. Während des Ziehens zeigt das Ziel-Panel einen Rahmen.

### Übertragungs-Panel (unten im SFTP-Tab)

- Drei Reiter mit Zählern: „Warteschlange“ (`queued`, `running`, `waitingDecision`, `paused`), „Abgeschlossen“
  (`done`, `skipped`, `cancelled`), „Fehler“ (`failed`).
- Je Zeile: ↑/↓, Name (`rel`), Zielordner, Fortschrittsbalken, Geschwindigkeit, Status. Fehlertexte kommen aus
  `t.sftp.fsErrors` bzw. den Netzwerktexten, nie aus der rohen Core-Meldung.
- Aktionen: „Abbrechen“ an Zeilen in der Warteschlange, „Wiederholen“ an Zeilen unter „Fehler“, „Leeren“ im Reiter
  „Abgeschlossen“.
- Einklappbar. Eingeklappt zeigt eine Zeile die Zusammenfassung („3 laufen · 12 fertig · 1 Fehler“).
- Ohne Jobs ist das Panel ausgeblendet.

### Konfliktdialog

- Nach dem Design: Titel „Datei existiert bereits“, Name, zwei Spalten „Neu · Lokal/Remote“ und
  „Vorhanden · Remote/Lokal“ mit Größe und Datum, Checkbox „Für alle weiteren Konflikte übernehmen“.
- Knöpfe: Abbrechen (ganzer Auftrag), Überspringen, Nur wenn neuer, Überschreiben. `Esc` = Abbrechen.
  Fokus startet auf „Überspringen“ (nichts geht verloren).
- Immer nur ein Dialog. Er erscheint nur im aktiven SFTP-Tab. Hat ein anderer Tab einen offenen Konflikt, bekommt
  dessen Reiter einen Punkt.

### Neu verbinden

- Die `ReconnectBar` aus TP2d wird wiederverwendet: „Verbindung unterbrochen · Versuch n von 8 in s s“ mit
  „Jetzt verbinden“ und „Abbrechen“.
- Das Lost-Banner aus TP3a bleibt für den aufgegebenen Fall. „Neu verbinden“ ruft `sftp_reconnect_now` auf und
  behält die Sitzung und die Warteschlange.

### Tab schließen

- Hat der Tab Jobs in `queued`, `running`, `waitingDecision` oder `paused`, fragt ein Dialog:
  „Noch {n} Übertragungen offen. Tab trotzdem schließen?“ (Abbrechen / Schließen). Schließen ruft `sftp_close`.

### Strings

Alle neuen Texte stehen in `ui/src/i18n/de.ts` unter `t.sftp.transfers` und `t.sftp.conflict`.

## 4. Invarianten

- Secrets und Host-Keys: unverändert. Reconnect-Versuche fragen nie nach (`inputRequired`), Secrets kommen nur aus
  dem Keyring.
- Genau ein abschließendes `ConnectEvent` je Connect-ID beim ersten Verbinden. Reconnect-Versuche senden keine
  Connect-Events an die UI.
- Unter dem Zielnamen liegt nie eine halb übertragene Datei. Eine vorhandene Datei bleibt bis zum erfolgreichen Ende
  unverändert.
- Die Rekursion folgt nie Ordner-Symlinks.
- Kein verlustbehafteter Dateiname erreicht je das Dateisystem.
- Download-Rechte sind nie weiter als die Remote-Rechte (außer dem Mindest-`u+rw`).

## 5. Tests

- Core, Engine mit zwei `LocalSide`s (Tempdirs):
  - Datei und Ordnerbaum übertragen, Inhalt und mtime gleich. Leere Datei, leerer Ordner.
  - Während der Übertragung existiert nur `name.filepart`, danach nur `name`.
  - Höchstens `parallel` Datei-Jobs gleichzeitig `running`.
  - Konflikt: Job wartet, andere laufen weiter. `skip`, `overwrite`, `newer` (beide Fälle), `cancelBatch`,
    `applyToAll` (auch für schon wartende Konflikte). Nur ein Konflikt-Event zur Zeit.
  - Datei gegen Ordner ergibt `failed`/`AlreadyExists`.
  - Abbrechen eines Jobs und eines Auftrags löscht die `.filepart`. Fertige Dateien bleiben.
  - Fehler-Injektion: Rechtefehler macht nur diesen Job `failed`. `retry` überträgt ihn.
  - Fehler-Injektion Verbindungsverlust mitten in der Datei: Job `paused`, nach „Reconnect“ geht es ab der
    `.filepart`-Größe weiter (nur der Rest wird gelesen), Ergebnis identisch.
  - Quelle zwischendurch geändert: Neustart von vorn.
  - Symlink auf Ordner und kaputter Link: `skipped`. Symlink auf Datei: Inhalt übertragen.
  - Rechte: `upload_perms = 0644` → Datei `0644`, Ordner `0755`. Ohne → lokaler Modus. Download `0600` bleibt
    `0600`, Download `0444` wird `0644`.
  - Fortschritt wird gedrosselt, Statuswechsel nicht.
  - `close()` räumt alle `.filepart` auf, danach kein Event.
- Core, Docker-sshd (`--ignored`):
  - Upload und Download von Datei und Baum, mtime und Rechte.
  - Überschreiben per `posix-rename@openssh.com`.
  - `docker pause` mitten in einer großen Datei bei Keepalive: `reconnecting`, dann `reconnected`, die Datei wird
    fertig und ist identisch.
  - Reconnect mit Prompt-Pflicht (Host ohne gespeichertes Secret) endet mit `lost`/`inputRequired`.
- `src-tauri`: Commands mit unbekannten IDs, `sftp_close` mit laufendem Job räumt auf. Settings-DTO
  `autoReconnect` liest den alten Schlüssel.
- UI (Vitest):
  - Store: Events → Jobs, Reiter-Zuordnung, Geschwindigkeit, Konflikt-Schlange, `batchDone` lädt das Ziel-Panel
    neu, `reconnected` lädt beide Panels neu.
  - `F5`, Kontextmenü und Toolbar rufen `transferEnqueue` mit Richtung, absoluten Pfaden und Zielordner. `..` und
    lossy Einträge fehlen. Deaktiviert ohne Verbindung.
  - DnD zwischen den Panels und auf Ordner-Einträge. Externer Drop nur über dem Remote-Panel.
  - Übertragungs-Panel: Reiter, Zähler, Aktionen, Einklappen, ausgeblendet ohne Jobs.
  - Konfliktdialog: Knöpfe, Checkbox, `Esc`, Fokus, nur im aktiven Tab, Punkt am Reiter.
  - Tab schließen mit offenen Jobs fragt nach.
- Manuelle Abnahme: `docs/acceptance-tp3b.md`.

## 6. Risiko

- Ob HTML5-DnD in der Webview und `onDragDropEvent` von Tauri unter GNOME Wayland nebeneinander funktionieren, wird
  als erste Plan-Aufgabe manuell geprüft. Falls nicht, läuft das DnD in der App über Pointer-Events statt HTML5-DnD.
