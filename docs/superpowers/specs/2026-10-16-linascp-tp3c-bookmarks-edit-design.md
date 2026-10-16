# LinaSCP TP3c: SFTP – Lesezeichen und Bearbeiten

Stand: 2026-10-16. Teil 3 von TP3 („SFTP-Dateimanager“). Reihenfolge: 3a Verbinden + Navigieren +
Dateioperationen (fertig) → 3b Übertragungen (fertig) → **3c Lesezeichen + Bearbeiten**.

Vorlage: `shellport-design.zip` → `screens/SFTP.dc.html`: Lesezeichen-Knopf im Kopf jedes Panels (neben
„Aktualisieren“), Toolbar-Knopf „Bearbeiten“. Baut auf TP3a (`2026-10-15-linascp-tp3a-sftp-browse-design.md`) und
der Transfer-Engine aus TP3b (`2026-10-15-linascp-tp3b-transfers-design.md`) auf.

Der Nutzer hat auf die Fragerunde verzichtet („ich vertraue deinen Defaults“). Alle Entscheidungen unten sind
Defaults des Autors, nicht abgestimmt.

## Ziel

- Lesezeichen: Jedes Panel merkt sich Ordner. Remote-Lesezeichen gehören zum Host, lokale gelten für alle Hosts.
  Ein Klick springt hin.
- Bearbeiten: Eine Remote-Datei öffnet sich im Texteditor des Systems (oder einem eingestellten Editor). Jedes
  Speichern lädt die Datei automatisch zurück, solange der SFTP-Tab offen ist.
- Hat jemand die Remote-Datei inzwischen geändert, wird nicht still überschrieben, sondern gefragt.
- Eine lokale Datei öffnet sich direkt im Editor, ohne Kopie.

Nicht enthalten:
- Lesezeichen im Export/Import (`.linascp`). Sie sind schnell neu gesetzt; kommt bei Bedarf.
- Lesezeichen umbenennen, sortieren, gruppieren. Angezeigt wird der Ordnername, der volle Pfad als Tooltip.
- Ein eingebauter Editor.
- Direktes Überschreiben ohne `.filepart` (für Dateien in Ordnern ohne Schreibrecht). Das Zurückladen läuft über
  die TP3b-Engine und braucht Schreibrecht im Ordner (TP4).
- Bearbeiten über das Schließen des Tabs oder einen App-Neustart hinaus.
- „Öffnen mit …“ für beliebige Programme nach Dateityp.

## 1. Lesezeichen

### Core (`crates/core/src/store/`)

- Migration V4:
  ```sql
  CREATE TABLE bookmarks (
      id          TEXT PRIMARY KEY NOT NULL,
      host_id     TEXT,             -- NULL = local bookmark
      path        TEXT NOT NULL,
      position    INTEGER NOT NULL,
      created_at  INTEGER NOT NULL
  );
  CREATE INDEX bookmarks_host_id ON bookmarks(host_id);
  ```
  `host_id` hat bewusst keinen Fremdschlüssel: Hosts aus `~/.ssh/config` stehen nicht in `hosts`, haben aber eine
  stabile ID (`import_id`, uuid v5). Ihre Lesezeichen sollen trotzdem halten.
- `Bookmark { id, host_id: Option<Id>, path, position, created_at }`.
- `Store::list_bookmarks(host_id: Option<Id>) -> Vec<Bookmark>`: genau die lokalen (`None`) bzw. die des Hosts,
  nach `position`.
- `Store::add_bookmark(host_id, path) -> Bookmark`: hängt hinten an. Gibt es den Pfad für diesen Host schon, kommt
  der vorhandene Eintrag zurück (kein Duplikat).
- `Store::remove_bookmark(id)`: ein unbekannter `id` ist kein Fehler.
- `delete_host` löscht die Lesezeichen des Hosts mit.
- Validierung (`validate.rs`, `validate_bookmark_path`): absolut, höchstens 4096 Bytes, keine Steuerzeichen.
  Ein Pfad wird nicht normalisiert; die UI liefert die kanonischen Pfade aus dem Listing.

### Tauri

- `bookmarks_list(hostId: Option<Id>) -> Vec<Bookmark>`, `bookmark_add(hostId: Option<Id>, path) -> Bookmark`,
  `bookmark_remove(id)`.

### Oberfläche

- `stores/bookmarks.ts` (neu): lokale Lesezeichen und die Lesezeichen je Host, geladen beim ersten Öffnen des
  Menüs.
- Knopf „Lesezeichen“ (Icon `Bookmark` aus lucide; gefüllt, wenn der aktuelle Ordner gemerkt ist) im Panel-Kopf,
  rechts neben den Breadcrumbs. Er öffnet ein Menü (`role="menu"`, wie das Kontextmenü):
  - Oben „Diesen Ordner merken“ bzw. „Lesezeichen entfernen“, wenn der aktuelle Ordner schon gemerkt ist.
  - Darunter die Lesezeichen (Ordnername, Pfad als `title`). Klick oder `Enter` navigiert ins Panel. Daneben je ein
    kleiner Entfernen-Knopf.
  - Ohne Lesezeichen: Hinweis „Noch keine Lesezeichen“.
  - Pfeiltasten bewegen den Fokus, `Esc` schließt und gibt den Fokus an den Knopf zurück.
- `Ctrl+B` im Panel schaltet das Lesezeichen für den aktuellen Ordner um.
- Ein Lesezeichen, das nicht mehr existiert, führt zum normalen Navigationsfehler des Panels. Es wird nicht
  automatisch entfernt.

## 2. Bearbeiten

### Ablauf

1. Auslösen: `F4`, `Enter` oder Doppelklick auf eine Datei, Kontextmenü „Bearbeiten“, Toolbar-Knopf „Bearbeiten“.
   Gilt für den Cursor-Eintrag (nicht die Mehrfachauswahl). Ordner verhalten sich wie bisher (`Enter` öffnet sie).
   Lossy Namen und Symlinks ohne Datei-Ziel werden ignoriert. Ohne Verbindung sind die Auslöser im Remote-Panel aus.
2. **Lokales Panel:** Die Datei öffnet sich direkt im Editor. Kein Watcher, keine Kopie.
3. **Remote-Panel:**
   - Der Core löst den Pfad per `realpath` auf und prüft per `stat`, dass es eine Datei ist. Ein Symlink wird so
     nie durch eine Datei ersetzt: Zurückgeladen wird immer auf das Ziel.
   - Ist dieselbe Remote-Datei in dieser Sitzung schon in Bearbeitung, öffnet sich nur der Editor erneut.
   - Sonst wird sie in ein frisches Verzeichnis `~/.cache/linascp/edit/<uuid>/<name>` heruntergeladen
     (Verzeichnisse `0700`). Der Download läuft als normaler Auftrag durch die TP3b-Warteschlange und erscheint im
     Übertragungs-Panel.
   - Die lokale Kopie bekommt den Modus `remote & 0o666 | 0o600`, also nie Ausführrechte. So startet auch ein
     Standardprogramm nie eine heruntergeladene `.desktop`-Datei oder ein Skript.
   - Danach öffnet sich der Editor. Der Remote-Stempel (`size`, `mtime`) zum Zeitpunkt des Downloads wird gemerkt.
4. **Beobachten:** Einmal pro Sekunde wird `size` und `mtime` (Nanosekunden) der lokalen Kopie gelesen. Polling
   statt inotify, weil Editoren beim Speichern oft umbenennen und so einen Watch verlieren.
   - Hat sich die Kopie geändert und ist sie einen Tick lang stabil geblieben, wird sie zurückgeladen.
   - Ändert sie sich während des Hochladens erneut, folgt danach ein weiterer Upload.
   - Ist die Sitzung gerade nicht verbunden, wartet der Upload, bis sie es wieder ist (der Watcher versucht es bei
     jedem Tick erneut).
5. **Zurückladen:**
   - Vorher `stat` der Remote-Datei. Weicht der Stempel vom gemerkten ab, geht die Datei auf `conflict` und die UI
     fragt (siehe unten). Ist die Remote-Datei verschwunden, wird sie ohne Frage neu angelegt.
   - Hochgeladen wird als Auftrag der TP3b-Engine mit fester Konfliktantwort `overwrite` und festem Dateimodus: dem
     aktuellen Remote-Modus (`& 0o777`), nicht `upload_perms`. Das Bearbeiten ändert also keine Rechte. Die Datei
     erscheint im Übertragungs-Panel und nutzt `.filepart`, Rename und Reconnect wie jeder Upload.
   - Nach Erfolg wird der neue Remote-Stempel gemerkt. Schlägt der Job fehl, geht die Datei auf `failed` mit dem
     Fehler; der nächste lokale Speichervorgang versucht es erneut.
6. **Konflikt:** Dialog „Datei auf dem Server geändert“: „{name} wurde auf dem Server geändert, seit sie geöffnet
   wurde. Mit deiner Version überschreiben?“ Knöpfe „Nicht hochladen“ (Fokus, `Esc`) und „Überschreiben“.
   - „Überschreiben“ lädt hoch.
   - „Nicht hochladen“ lässt beide Seiten, wie sie sind, und merkt den neuen Remote-Stempel nicht. Der nächste
     lokale Speichervorgang fragt also erneut.
   - Wie beim TP3b-Konfliktdialog: nur ein Dialog zur Zeit, nur im aktiven Tab, ein Punkt am Reiter des anderen
     Tabs.
7. **Beenden:** „Bearbeitung beenden“ an der Datei in der Liste (siehe unten) stoppt den Watcher. Die lokale Kopie
   samt Verzeichnis wird gelöscht, wenn ihr letzter Stand hochgeladen ist. Sonst bleibt sie liegen und ein Hinweis
   nennt den Pfad.
8. **Tab schließen:** Wie „Beenden“ für alle Dateien des Tabs, nach einem letzten Poll. Der Schließen-Dialog aus
   TP3b zählt Dateien mit nicht hochgeladenen Änderungen (`dirty`) zu den offenen Übertragungen dazu.

### Core (`crates/core/src/edit/`, neu; Änderungen in `crates/core/src/transfer/`)

- Transfer-Engine:
  - `TransferQueue::enqueue_with(direction, sources, target_dir, EnqueueOptions { decision: Option<Decision>,
    file_mode: Option<u32> }) -> BatchId`. `enqueue` ruft es mit `Default` auf. `decision` wird zur Vorgabe des
    Auftrags (wie „Für alle übernehmen“), `file_mode` ersetzt für Dateien des Auftrags die Moduswahl aus
    `TransferOptions`.
  - `TransferQueue::wait_batch(batch_id) -> Vec<Job>`: wartet, bis alle Jobs des Auftrags in einem Endstatus sind
    (oder die Queue geschlossen ist), und liefert sie.
- `Side` bekommt `realpath(path) -> String`.
- `EditManager` gehört zu einer SFTP-Sitzung, neben der `TransferQueue`:
  - `new(queue, remote: Fn() -> Option<Arc<dyn Side>>, root: PathBuf, launcher: Arc<dyn Launcher>, emit)`.
  - `open(remote_path) -> EditId`, `resolve(id, overwrite: bool)`, `reopen(id)`, `stop(id) -> Option<PathBuf>`
    (der behaltene Pfad, falls Änderungen nicht hochgeladen sind), `close() -> Vec<PathBuf>`.
  - `Launcher` ist ein Trait mit `open(path) -> Result<()>`. Der Core kennt kein GTK; die echte Implementierung lebt
    in `src-tauri`. Tests nutzen einen Fake, der die Aufrufe zählt.
  - Event `EditEvent::File(EditFile)` bei jedem Statuswechsel. `EditFile { id, name, remote_path, state, dirty,
    error? }`, `state` ∈ `downloading`, `synced`, `changed`, `uploading`, `conflict`, `failed`. `dirty` heißt: die
    lokale Kopie hat einen Stand, der noch nicht hochgeladen ist.
- Tests mit zwei `LocalSide`s (wie TP3b) und Fake-Launcher. Das Poll-Intervall ist im Test kurz einstellbar
  (`tokio::time::pause` oder Parameter).

### Editor starten (`src-tauri`)

- Einstellung `editorCommand: Option<String>` (DB-Schlüssel `editor_command`). Leer = Standard-Texteditor des
  Systems.
- Standard: `gio::AppInfo::default_for_type("text/plain", false)` und `launch_uris` mit der `file://`-URI. Der
  Texteditor wird über `text/plain` gewählt, nicht über den Typ der Datei: So öffnet „Bearbeiten“ nie ein
  Bildprogramm oder startet ein Programm. Gibt es keinen, kommt ein Fehler mit dem Hinweis auf die Einstellung.
- Mit `editorCommand`: an Leerzeichen geteilt, erstes Wort ist das Programm, die Datei kommt als letztes Argument
  (`ponytail`: kein Quoting; reicht für `code -w`, `gnome-text-editor`, `kate`). Gestartet ohne Shell, der Prozess
  wird nicht abgewartet.
- `validate_editor_command`: nicht leer nach `trim`, höchstens 512 Bytes, keine Steuerzeichen.
- `gio` 0.18 steht über Tauri schon im `Cargo.lock`. Es wird als direkte Abhängigkeit von `src-tauri` in genau
  dieser Version eingetragen, ohne neue Downloads.

### Tauri-Commands und Events

- `edit_open(sessionId, path)` (remote), `local_edit(path)` (lokal), `edit_resolve(sessionId, editId, overwrite)`,
  `edit_reopen(sessionId, editId)`, `edit_stop(sessionId, editId) -> Option<String>`.
- `sftp_close` ruft zuerst `EditManager::close()` und liefert die behaltenen Pfade an die UI zurück, dann wie in
  TP3b.
- `SftpEvent::edit { file: EditFile }` auf dem bestehenden geordneten Kanal des Tabs.
- Unbekannte IDs ergeben einen Fehler, nie einen Panic.

### Oberfläche

- `stores/edits.ts` (neu) oder Teil von `stores/transfers.ts`: Dateien in Bearbeitung je Tab, aus den
  `edit`-Events.
- Das Übertragungs-Panel bekommt einen vierten Reiter „Bearbeitet“ mit Zähler. Je Zeile: Name, Remote-Ordner,
  Status („Synchron“, „Geändert“, „Wird hochgeladen“, „Auf dem Server geändert“, Fehlertext), Knöpfe „Im Editor
  öffnen“ und „Bearbeitung beenden“. Das Panel ist sichtbar, sobald es Jobs oder bearbeitete Dateien gibt.
- Einstellungen: Feld „Editor“ mit Platzhalter „Standard-Texteditor des Systems“ und Hilfetext „Befehl, die Datei
  wird angehängt, z. B. `code -w`“.
- Hinweise (Notices) für: Editor nicht startbar, behaltene lokale Kopie mit Pfad.
- Alle Texte in `ui/src/i18n/de.ts` unter `t.sftp.bookmarks` und `t.sftp.edit`.

## 2b. Öffnen mit und neue Datei (Nachtrag)

- **„Öffnen mit …“** (Kontextmenü, beide Panels) zeigt den nativen GTK-Dialog `GtkAppChooserDialog` für den
  Dateityp (Typ per `gio` aus dem Namen geraten, „weitere Anwendungen“ eingeschlossen). Die gewählte App öffnet die Datei; Remote-Dateien laufen
  wie beim Bearbeiten über die private Kopie mit Upload. `reopen` nutzt die zuletzt gewählte App.
- Das Häkchen „Immer für diesen Dateityp verwenden“ (in den GTK-Dialog eingefügt) merkt die App pro MIME-Typ in den LinaSCP-Einstellungen
  (`open_with.<mime>` = Desktop-ID), **nie** in der System-`mimeapps.list`. Abwählen bei der gemerkten App vergisst sie.
- Doppelklick und `Enter` nutzen die gemerkte App, sonst den Editor. `F4`/„Bearbeiten“ nutzt immer den Editor.
- Gestartet wird nur eine installierte App, über ihre Desktop-ID (`*.desktop`, geprüft) nachgeschlagen; die ID wird
  nie als Befehl ausgeführt. Kopien bleiben nicht ausführbar.
- **„Neue Datei“** (Kontextmenü, Toolbar, `Shift+F4`) legt eine leere Datei exklusiv an (lokal `create_new`, remote
  `CREATE|EXCLUDE`, Modus 0644): ein vorhandener Name wird nie angefasst. Danach steht der Cursor auf ihr.

## 3. Invarianten

- Secrets und Host-Keys: unverändert.
- Eine Remote-Datei wird nie überschrieben, wenn sie seit dem letzten bekannten Stand auf dem Server geändert wurde,
  außer nach ausdrücklichem „Überschreiben“.
- Eine Remote-Datei wird über das Bearbeiten nie halb geschrieben (TP3b `.filepart`) und behält ihre Rechte.
- Ein Symlink auf dem Server wird beim Bearbeiten nie durch eine Datei ersetzt.
- Lokale Kopien liegen in `0700`-Verzeichnissen und sind nie ausführbar.
- Nicht hochgeladene Änderungen werden nie gelöscht.
- Kein verlustbehafteter Dateiname wird bearbeitet.

## 4. Tests

- Core, Store: Lesezeichen anlegen, Duplikat liefert den vorhandenen Eintrag, Reihenfolge, Trennung lokal/Host,
  entfernen, `delete_host` nimmt sie mit, Migration V4 auf einer V3-Datenbank. Validierung der Pfade.
- Core, Engine: `enqueue_with` mit `overwrite` fragt nicht, `file_mode` setzt genau diesen Modus, `wait_batch`
  liefert die Endstände.
- Core, Edit (zwei `LocalSide`s, Fake-Launcher):
  - Öffnen lädt herunter, Modus der Kopie ohne `x`, Verzeichnis `0700`, Launcher einmal aufgerufen.
  - Zweites Öffnen derselben Datei lädt nicht erneut und ruft den Launcher erneut.
  - Symlink wird auf sein Ziel aufgelöst; nach dem Zurückladen ist der Link noch ein Link.
  - Lokale Änderung wird hochgeladen, Remote-Inhalt gleich, Remote-Modus unverändert.
  - Remote zwischendurch geändert: `conflict`, kein Upload; `resolve(true)` lädt hoch, `resolve(false)` nicht, und
    die nächste Änderung fragt erneut.
  - Remote gelöscht: wird neu angelegt.
  - `stop` nach Upload löscht die Kopie; mit nicht hochgeladener Änderung bleibt sie und der Pfad kommt zurück.
  - `close` wie `stop` für alle.
- Core, Docker-sshd (`--ignored`): Datei öffnen, lokal ändern, Inhalt auf dem Server prüfen, Rechte unverändert.
- `src-tauri`: Validierung `editorCommand`, Settings-DTO, unbekannte Edit-IDs.
- UI (Vitest): Lesezeichen-Menü (merken, entfernen, navigieren, leer, Tastatur, `Ctrl+B`), Bearbeiten-Auslöser je
  Panel (`F4`, `Enter`, Doppelklick, Menü, Toolbar, deaktiviert ohne Verbindung), Reiter „Bearbeitet“, Edit-Konflikt-
  dialog, Schließen-Dialog zählt nicht hochgeladene Dateien, Einstellungsfeld „Editor“.
- Manuelle Abnahme: `docs/acceptance-tp3c.md` (Editor startet unter GNOME, Speichern im Editor lädt hoch, Konflikt
  mit gleichzeitiger Änderung per Terminal, WLAN weg während des Speicherns).

## 5. Was dem Umsetzenden überlassen bleibt

- Ob die Edit-Zustände in einem eigenen Store oder in `stores/transfers.ts` leben.
- Wie genau `wait_batch` intern wartet (`Notify`, `watch`).
- Aufteilung von `edit/` in Dateien.
