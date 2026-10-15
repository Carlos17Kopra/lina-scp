# LinaSCP TP3a: SFTP-Dateimanager – Verbinden, Navigieren, Dateioperationen

Stand: 2026-10-15. Teil 1 von TP3 („SFTP-Dateimanager“). Reihenfolge:
**3a Verbinden + Navigieren + Dateioperationen** → 3b Übertragungen (Warteschlange, Konflikt-Dialog) →
3c Lesezeichen + Bearbeiten.

Vorlage: `shellport-design.zip` → `screens/SFTP.dc.html` (Commander-Ansicht) und `screens/ConnectSFTP.dc.html`
(Ladebildschirm). TP4 (Synchronisieren, Vergleichen, Aktuell halten, Rechte, Bandbreite, sudo-sftp-server) bleibt
außen vor.

## Ziel

- Ein Host lässt sich per SFTP öffnen: über den Knopf „Dateien“ in der Host-Übersicht oder über das SFTP-View.
- Die Ansicht ist ein Commander mit zwei Panels, links **Lokal**, rechts **Remote**. Beide Panels bieten Navigation,
  Breadcrumbs, Sortierung und versteckte Dateien.
- Dateioperationen auf beiden Seiten: Neuer Ordner, Umbenennen, Löschen. Lokal geht Löschen in den Papierkorb.
- Die Bedienung geht vollständig per Maus und per Tastatur, im Stil des WinSCP-Commanders.
- Bricht die Verbindung ab, erscheint ein Banner mit „Neu verbinden“. Pfad und Auswahl bleiben erhalten.

Nicht enthalten:
- Hochladen, Herunterladen, Drag & Drop, Warteschlange, Konflikt-Dialog, Upload-Rechte (TP3b).
- Automatisches Wiederverbinden der SFTP-Sitzung (TP3b, zusammen mit den Übertragungen).
- Lesezeichen, Bearbeiten im externen Editor (TP3c).
- `sftpServerCmd` (sudo), Rechte ändern, Bandbreite, Synchronisieren, Vergleichen (TP4).
- Remote ↔ Remote, Archive, Suche.
- Ein Doppelklick auf eine Datei tut in TP3a nichts.
- Virtualisierte Liste. Alle Einträge eines Ordners werden gerendert. Virtualisierung erst bei echtem Bedarf.

## 1. Core (`crates/core/src/sftp/`)

### Abhängigkeit

- `russh-sftp = "3.0"` (Client-Teil). Es läuft auf einem russh-Kanal mit `request_subsystem("sftp")`.
- `trash` für den XDG-Papierkorb. Es arbeitet auch auf anderen Mounts nach der freedesktop-Spezifikation.

### Verbinden

- `connect_sftp(req, deps, broker, emit, cancel) -> Result<SftpSession>` neben `connect_ssh`.
- Es nutzt `Pipeline::establish` unverändert, also Jump-Host, TCP, Host-Key, Auth. Danach folgen zwei neue Schritte:
  - `Step::Subsystem`: Kanal öffnen, Subsystem `sftp` anfordern, `russh_sftp::client::SftpSession` starten.
  - `Step::Directory`: Startverzeichnis bestimmen und einmal auflisten. Es ist `host.sftp.remote_start`, sonst
    `realpath(".")` (Home). Ist `remote_start` nicht lesbar, schlägt dieser Schritt fehl. Die Fehlerseite zeigt den
    Grund, „Erneut versuchen“ funktioniert wie gewohnt.
- `plan_steps` liefert je nach Protokoll `[JumpHost?, Tcp, HostKey, Auth, Channel, Shell]` oder
  `[JumpHost?, Tcp, HostKey, Auth, Subsystem, Directory]`.
- Es gibt genau ein abschließendes `ConnectEvent` je Connect-ID (`connected` | `failed`), auch bei Abbruch und bei
  Fehlern vor dem Start der Pipeline. Das gilt wie bei SSH.
- `SessionInfo` wird wie bei SSH befüllt (Host-Key, Kex, Cipher, MAC, Auth-Methoden). Dazu kommt die
  SFTP-Protokollversion für die Statuszeile.
- `connected` liefert zusätzlich den Startpfad und die erste Auflistung, damit das Panel ohne zweiten Roundtrip
  erscheint.

### Datenmodell

```rust
pub enum EntryKind { File, Dir, Symlink { target: Option<LinkTarget> }, Other }
pub enum LinkTarget { File, Dir, Other }   // None = dangling or unreadable

pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub mtime: Option<i64>,      // Unix seconds
    pub mode: Option<u32>,       // permission bits incl. type bits as reported
    pub owner: Option<String>,   // user name, else numeric uid as string
    pub name_lossy: bool,        // local only: name was not valid UTF-8
}

pub struct Listing { pub path: String, pub entries: Vec<Entry> }  // path is canonical/absolute
```

- `.` und `..` kommen nie in `entries` vor. Die UI zeigt „..“ selbst an.
- Symlinks: Die Zielart wird per `stat` bestimmt (remote: ein `stat` je Link). Damit kann die UI in einen
  verlinkten Ordner navigieren.
- Besitzer remote: aus `longname` (dritte Spalte von `ls -l`). Fehlt sie, die uid. Lokal: Name aus `/etc/passwd`
  (einmal gelesen und gecacht), sonst die uid.

### Operationen

Beide Seiten bieten dieselben Operationen. Remote auf `SftpSession`, lokal als freie Funktionen in `sftp/local.rs`.
Pfade sind absolute POSIX-Pfade als `String`.

| Operation | Verhalten |
|---|---|
| `list(path)` | Kanonischen Pfad (`realpath` bzw. `canonicalize`) und Einträge liefern |
| `home()` | Remote `realpath(".")`, lokal `local_start` bzw. `$HOME` |
| `mkdir(dir, name)` | Neuen Ordner anlegen. Existiert der Name, `Fs { kind: AlreadyExists }` |
| `rename(dir, from, to)` | Umbenennen im selben Ordner, **nie überschreibend**. Lokal mit `renameat2(RENAME_NOREPLACE)`. Remote lehnt SFTP-v3-`rename` ein existierendes Ziel ab. Zusätzlich wird vorher per `lstat` geprüft, um eine klare Meldung zu liefern |
| `delete(dir, names)` | Endgültig und rekursiv, von unten nach oben. Typen kommen aus `lstat`, Symlinks werden als Link entfernt und **nie** verfolgt |
| `trash(dir, names)` | Nur lokal: in den XDG-Papierkorb |

- Namen für `mkdir`, `rename` und die Elemente von `names` werden im Core geprüft (`validate.rs`): nicht leer,
  kein `/`, nicht `.` oder `..`, kein NUL. Verstöße ergeben `Config`.
- Schlägt `delete` mittendrin fehl, bricht es ab und liefert den Fehler mit dem betroffenen Pfad. Die UI lädt das
  Panel danach neu, damit sichtbar ist, was übrig blieb.
- Lokale Namen, die kein gültiges UTF-8 sind, kommen mit `name_lossy = true`. Operationen mit einem solchen Namen
  liefern `Config`. Es wird nie ein verlustbehafteter Name an das Dateisystem gegeben.

### Fehler

Neue Variante in `CoreError`:

```rust
Fs { kind: FsKind, path: String, message: String }
pub enum FsKind { PermissionDenied, NotFound, AlreadyExists, NotEmpty, Other }
```

- SFTP-Statuscodes (`PermissionDenied`, `NoSuchFile`, `Failure` …) und `io::ErrorKind` werden darauf abgebildet.
  Ein `Failure` auf `mkdir`/`rename`, dessen Ziel per `lstat` existiert, wird `AlreadyExists`.
- Eine geschlossene Verbindung (Kanal zu, Handle geschlossen, Timeout) wird `Network`.
- Keine Secrets in Meldungen, wie überall.

### Sitzungen und Verbindungsverlust

- `SftpSession` hält den russh-Handle (und den Jump-Handle), die `russh_sftp`-Sitzung und die `SessionInfo`.
- Erkennung eines Abbruchs: ein Wächter-Task wartet, bis der Handle geschlossen ist. Zusätzlich gilt jede Operation
  als Abbruch, die mit `Network` fehlschlägt, solange `handle.is_closed()` stimmt.
- In beiden Fällen meldet die Sitzung **genau einmal** `SftpEvent::Lost { error }`. Danach liefern alle
  Operationen sofort `Network`.
- Requests einer Sitzung haben ein Timeout von `host.timeout_s`. So hängt kein Panel ewig an einem toten Link.

## 2. Backend (`src-tauri`)

- `AppState.sftp_sessions: Map<SessionId, Arc<SftpSession>>`, getrennt von den Terminal-Sitzungen.
- Commands:
  - `connect_sftp(connect_id, host_id, on_event: Channel<ConnectEvent>, on_sftp: Channel<SftpEvent>)`. Die Prompts,
    `connect_answer` und `connect_cancel` funktionieren wie bei SSH.
  - `sftp_list(session_id, path)`, `sftp_mkdir`, `sftp_rename`, `sftp_delete`.
  - `local_list(path)`, `local_home(host_id?)`, `local_mkdir`, `local_rename`, `local_delete`, `local_trash`.
  - `sftp_close(session_id)` trennt die Verbindung und entfernt die Sitzung.
- `SessionInfo` enthält schon alles für die Statuszeile, dazu kommt die SFTP-Version. Ein eigener Info-Command ist
  nicht nötig, die Info kommt mit `connected`.
- Lokale Dateioperationen laufen in `spawn_blocking`.
- Bindings neu erzeugen (`cargo test -p linascp exports_bindings`), `ui/src/ipc/api.ts` wrappt alle neuen Commands.

## 3. UI

### Ablauf

- Der Knopf „Dateien“ in der Host-Übersicht wird aktiv, wenn der Host `protocolSftp` hat. Er ruft
  `useConnectStore.start(hostId, "sftp")` auf. `ConnectProtocol` kennt `"sftp"` bereits.
- Der Ladebildschirm zeigt die SFTP-Schritte mit amberfarbenem Akzent (Design `ConnectSFTP`). Host-Key-Dialoge,
  Fehlerseiten, Auto-Retry und „Erneut versuchen“ verhalten sich wie bei SSH. Abbrechen führt zurück ins vorige View.
- Nach `connected` entsteht ein neuer Tab im SFTP-View, und das View wird aktiv.
- Ohne offene SFTP-Sitzung zeigt das SFTP-View eine Host-Auswahl (nur Hosts mit `protocolSftp`) statt des
  Platzhalters. `SftpPlaceholder` und die Strings `t.stubs.sftp*` entfallen.
- „Terminal“ in der Toolbar startet `useConnectStore.start(hostId, "ssh")` für denselben Host.

### Store `useSftpStore`

```ts
interface SftpTab {
  id: string; hostId: Id; sessionId: string; title: string;
  status: "connected" | "lost"; lostError?: CoreError; info: SessionInfo; sftpVersion: number;
  panes: { local: PaneState; remote: PaneState };
  active: "local" | "remote";
}
interface PaneState {
  path: string; entries: Entry[]; loading: boolean; error?: CoreError;
  selection: string[]; cursor: string | null; anchor: string | null;
  sort: { key: "name" | "size" | "mtime" | "mode" | "owner"; dir: "asc" | "desc" };
  showHidden: boolean; editingPath: boolean;
}
```

- Titel von Tabs desselben Hosts werden nummeriert, wie bei den Terminals.
- Ein Tab schließen ruft `sftp_close` auf.
- `navigate(pane, path)` lädt die Auflistung. Bei einem Fehler bleiben der alte Pfad und die alten Einträge stehen,
  `error` zeigt den Grund.
- Nach `mkdir`, `rename`, `delete` und `trash` wird das Panel neu geladen. Der Cursor steht dann auf dem neuen bzw.
  umbenannten Eintrag oder, nach dem Löschen, auf dem Nachbarn.

### Layout

- `SftpScreen`: Tableiste (Hosts als Tabs, „+“ öffnet die Host-Auswahl), Toolbar, zwei `FilePane` nebeneinander,
  Statuszeile (`user@host`, `SFTP v<N> · <cipher>`).
- Toolbar in TP3a: Neuer Ordner, Umbenennen, Löschen, Neu laden, Versteckte (Umschalter), Terminal. Knöpfe späterer
  Teile werden nicht angezeigt.
- `FilePane`:
  - Kopf mit Breadcrumbs. Jedes Segment ist klickbar. Ein Klick auf die freie Fläche der Leiste oder `Strg+L` macht
    sie zum Pfad-Feld. `Enter` navigiert, `Esc` bricht ab.
  - Tabelle mit den Spalten Name, Größe, Geändert, Rechte, Besitzer. Ein Klick auf den Spaltenkopf sortiert, ein
    erneuter Klick dreht die Richtung. Ordner und Links auf Ordner stehen immer zuerst. Die Namenssortierung ist
    natürlich und ohne Groß-/Kleinschreibung (`Intl.Collator` mit `numeric: true`).
  - Oberste Zeile „..“, außer in `/`.
  - Versteckte Dateien (Name beginnt mit `.`) sind standardmäßig ausgeblendet.
  - Größe menschenlesbar (`18,4 MB`), Datum `08.10.2026 10:41`, Rechte als `rwxr-xr-x`. Ordner zeigen keine Größe.
  - Symlinks bekommen ein Link-Symbol, kaputte Links ein Warnsymbol.
  - Zeilen mit `nameLossy` sind sichtbar, aber nicht auswählbar für Operationen (Hinweis per `title`).
  - Ein `error` erscheint als Zeile oberhalb der Tabelle, mit aufklappbarer Originalmeldung.
- Verbindungsverlust: Ein Banner über dem Remote-Panel zeigt „Verbindung verloren“ und den Knopf „Neu verbinden“.
  Das Remote-Panel ist dann ausgegraut, das lokale bleibt bedienbar. „Neu verbinden“ startet einen normalen Connect
  mit Prompts. Bei Erfolg bekommt derselbe Tab die neue Sitzung und lädt den letzten Remote-Pfad. Ist der Pfad
  nicht mehr lesbar, wird das Startverzeichnis geladen.

### Dialoge

- Neuer Ordner und Umbenennen: Dialog mit Textfeld, vorbefüllt mit dem alten Namen beim Umbenennen (Name ohne
  Endung markiert). Die Validierung entspricht dem Core und zeigt Fehler am Feld. `AlreadyExists` aus dem Core
  erscheint ebenfalls am Feld.
- Löschen:
  - Remote und `Shift+Entf` lokal: roter Dialog „Endgültig löschen?“. Der Text nennt den Namen bzw. die Anzahl. Bei
    Ordnern steht der Hinweis „Der Ordner wird mitsamt Inhalt entfernt.“
  - Lokal mit `Entf`/`F8`: Dialog „In den Papierkorb verschieben?“ (nicht rot).
  - Fokus startet auf „Abbrechen“.

### Tastatur

Die Tasten gelten im aktiven Panel, solange kein Dialog und kein Textfeld den Fokus hat.

| Taste | Aktion |
|---|---|
| `↑` / `↓`, `Pos1` / `Ende`, `Bild↑` / `Bild↓` | Cursor bewegen |
| `Shift` + Bewegung | Auswahl erweitern |
| `Strg+Klick` / `Leertaste` | Eintrag zur Auswahl hinzufügen oder entfernen |
| `Shift+Klick` | Bereich auswählen |
| `Strg+A` | Alles auswählen (ohne „..“) |
| `Tab` | Panel wechseln |
| `Enter` / Doppelklick | Ordner oder Link auf Ordner öffnen, „..“ geht hoch |
| `Backspace` | Eine Ebene hoch |
| `F2` | Umbenennen (nur bei genau einem Eintrag) |
| `F7` | Neuer Ordner |
| `Entf` / `F8` | Löschen (lokal: Papierkorb) |
| `Shift+Entf` | Lokal endgültig löschen |
| `Strg+H` | Versteckte Dateien ein/aus |
| `Strg+R` | Neu laden |
| `Strg+L` | Pfad-Feld |
| Buchstaben | Springen zum ersten Namen mit diesem Anfang. Tippen innerhalb von 1 s setzt das Präfix fort |

- Rechtsklick öffnet ein Kontextmenü mit Öffnen, Umbenennen, Löschen, Neuer Ordner, Neu laden. Ein Rechtsklick
  auf einen nicht ausgewählten Eintrag wählt ihn vorher aus.
- Ist keine Auswahl gesetzt, wirken Operationen auf den Cursor-Eintrag.
- Globale Shortcuts der App (`Strg+K`, Tab-Wechsel) bleiben unverändert. Terminal-Shortcuts gelten nur im
  Terminal-View.

### Strings

Alle neuen Texte stehen in `ui/src/i18n/de.ts` unter `t.sftp`. Der Hinweis `t.editor.sftpHint` wird angepasst:
Startverzeichnisse wirken ab jetzt, die übrigen Einstellungen ab TP3b bzw. TP4.

## 4. Invarianten

- Secrets und Host-Keys: unverändert. SFTP nutzt dieselbe Pipeline, also Keyring und `known_hosts`, und jede
  Entscheidung ist fail-closed.
- Genau ein abschließendes `ConnectEvent` je Connect-ID, auch für SFTP. `useConnectStore.start`/`retry` lehnen nie ab.
- Umbenennen überschreibt nie. Löschen folgt nie Symlinks.
- Kein verlustbehafteter Dateiname erreicht je das Dateisystem.

## 5. Tests

- Core, lokal (Tempdir):
  - `list` liefert Art, Größe, Modus, Besitzer und lässt `.`/`..` weg. Ein Symlink auf einen Ordner hat
    `target = Dir`, ein kaputter Link `None`.
  - `mkdir` auf einen existierenden Namen ergibt `AlreadyExists`.
  - `rename` auf einen existierenden Namen ergibt `AlreadyExists`, und das Ziel bleibt unverändert.
  - Rekursives `delete` eines Ordners, der einen Symlink auf einen Ordner **außerhalb** enthält: Der Link ist weg,
    der Inhalt des Zielordners besteht weiter.
  - `trash` verschiebt in einen Test-Papierkorb (`XDG_DATA_HOME` auf ein Tempdir).
  - Ungültige Namen (`""`, `a/b`, `.`, `..`, NUL) ergeben `Config`.
  - Nicht-UTF-8-Name: `name_lossy = true`, und `rename`/`delete` damit ergeben `Config`.
- Core, Docker-sshd (`--ignored`):
  - `connect_sftp` meldet die Schritte inklusive `Subsystem` und `Directory` und liefert das Home bzw. `remote_start`.
  - Ein nicht lesbares `remote_start` lässt `Directory` fehlschlagen, mit genau einem `failed`.
  - `list`, `mkdir`, `rename` (auch das Nicht-Überschreiben), `delete` rekursiv mit Symlink.
  - Keine Berechtigung ergibt `Fs { kind: PermissionDenied }`.
  - Wird der sshd-Prozess der Sitzung beendet, kommt genau ein `Lost`. Danach liefern Operationen sofort `Network`.
- `src-tauri`: Commands mit Fake bzw. Tempdir für die lokale Seite. Ein unbekannter `session_id` ergibt `NotFound`.
- UI (Vitest):
  - Store: Navigieren, Fehler behalten den alten Pfad, Neu laden nach Operationen, Cursor nach dem Löschen, `lost`
    und „Neu verbinden“ im selben Tab.
  - `FilePane`: Sortierung (Ordner zuerst, natürlich), versteckte Dateien, Breadcrumbs und Pfad-Feld, Tastatur laut
    Tabelle, Tippen-zum-Springen, Kontextmenü.
  - Dialoge: Validierung, `AlreadyExists` am Feld, Papierkorb vs. endgültig, Fokus auf „Abbrechen“.
  - „Dateien“ in der Host-Übersicht ist aktiv bzw. deaktiviert je nach `protocolSftp`.
- Manuelle Abnahme: `docs/acceptance-tp3a.md`.
