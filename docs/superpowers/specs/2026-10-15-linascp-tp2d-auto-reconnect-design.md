# LinaSCP TP2d: Auto-Reconnect

Stand: 2026-10-15. Teil 4 von TP2 („SSH-Terminal komplett“). Reihenfolge:
2a Seitenleiste (fertig) → 2b Snippets (fertig) → 2c Split-Panes + Broadcast (fertig) → **2d Auto-Reconnect**.

Die Design-Vorlage (`shellport-design.zip`) enthält nichts zu Auto-Reconnect. Diese Spec legt das Verhalten fest.

## Ziel

- Bricht eine SSH-Verbindung unerwartet ab (Netz weg, Keepalive-Timeout, Server-Neustart), verbindet sich die Sitzung
  automatisch neu. Das geschieht ohne Klick, und der Verlauf bleibt stehen.
- Die neue Shell läuft im **selben xterm** weiter, unter einer Trennzeile.
- Ein reguläres Ende der Shell (`exit`, `logout`, Server schließt den Kanal) verbindet **nie** neu.
- Auto-Reconnect ist standardmäßig an und global abschaltbar.

Nicht enthalten:
- Einstellung pro Host.
- Lokale Terminals: Sie verbinden nicht neu.
- Eigene Netzwerkerkennung (NetworkManager, Resume-Events). Ein Abbruch wird über Keepalive, Kanalende oder
  Schreibfehler erkannt.
- Sitzungen wiederherstellen (`tmux`/`screen`-Integration). Die neue Shell ist ein neuer Login.
- Ein einmal eingetipptes, nicht gespeichertes Passwort für Reconnects im Speicher halten. Das wäre ein Bruch der
  Secret-Invariante.

## 1. Core

### Abbruch erkennen (`connect/ssh_session.rs`)

- `forward_output` merkt sich, ob `ChannelMsg::Close` kam.
  - Mit `Close` ist das Ende regulär: `SessionEvent::Exit { code }` wie bisher.
  - Endet der Kanal (`wait()` liefert `None`) **ohne** `Close`, ist das ein Abbruch: neues internes Ereignis
    `SessionEvent::Lost(CoreError)`, mit einem Fehler der Art `network`.
- Schlägt `write` oder `resize` mit einem Verbindungsfehler fehl, meldet die Sitzung ebenfalls `Lost`. Das gilt
  höchstens einmal je zugrunde liegender Verbindung. So bemerken auch Hosts mit `keepalive_s = 0` einen toten Link,
  sobald getippt wird.

### `ReconnectingSession` (neu, `connect/reconnect.rs`)

- Sie umhüllt die jeweils aktuelle `SshSession` und implementiert dasselbe `Session`-Trait. **Session-ID, Output-Sink,
  Output-Kanal und Ack-Flow bleiben über alle Neuverbindungen gleich.**
- Sie bekommt einen **Connector**. Das ist ein Closure, der eine neue `SshSession` mit gegebener Größe und gegebenem
  Sink aufbaut. In der App baut er bei jedem Versuch die Anfrage frisch aus Store und Keyring (`build_request`). Danach
  ruft er die normale Pipeline auf (Jump-Host, Auth, Host-Key-Prüfung gegen `known_hosts`). In Tests ist er ein Fake.
- Ablauf bei `Lost` der inneren Sitzung:
  1. Ist die Einstellung `terminal_auto_reconnect` aus, meldet sie sofort `lost { reason }`.
  2. Sonst startet eine Schleife mit höchstens **8 Versuchen**. Vor Versuch *n* wartet sie `BACKOFF[n-1]` mit
     `BACKOFF = [1, 2, 4, 8, 16, 30, 30, 30]` Sekunden. Vor jeder Wartezeit meldet sie
     `reconnecting { attempt, maxAttempts, retryInMs, reason }`.
  3. Bei Erfolg wird die neue innere Sitzung eingesetzt. Die zuletzt gemeldete Größe (`resize`) wird verwendet. Dann
     folgt `reconnected { info }`.
  4. Liefert ein Versuch `inputRequired` oder `notFound` (Host gelöscht), gibt sie sofort auf. Sonst geht es mit dem
     nächsten Versuch weiter.
  5. Nach dem 8. Fehlschlag kommt `lost { reason }` mit dem Fehler des letzten Versuchs.
- Ein erneuter Abbruch nach erfolgreichem Reconnect startet wieder mit vollen 8 Versuchen.
- `write` während des Neuaufbaus verwirft die Daten und meldet `Ok`. `resize` merkt sich die Größe.
- `reconnect_now()`:
  - In der Wartezeit startet es den nächsten Versuch sofort.
  - Nach `lost` (Ursache nicht `inputRequired` oder `notFound`) startet es eine neue Runde mit 8 Versuchen, ohne
    Wartezeit vor dem ersten.
  - Läuft gerade ein Versuch oder ist die Sitzung verbunden, tut es nichts.
- `stop_reconnect()`: In Wartezeit oder laufendem Versuch bricht es ab, ein halb aufgebauter Versuch wird geschlossen.
  Danach kommt `lost { reason: cancelled }`. Sonst tut es nichts.
- `close()`: beendet Schleife, Versuch und innere Sitzung. Danach kommt kein weiteres Event mehr.
- **Genau ein abschließendes Event je Runde:** `exit`, `error` oder `lost`. `reconnect_now` nach `lost` startet eine
  neue Runde.
- Ein `start_command` des Hosts läuft bei jedem erfolgreichen Reconnect erneut, wie bei jedem Login.

### Nicht interaktive Versuche

- Die Reconnect-Versuche nutzen einen Prompt-Modus **ohne Rückfrage**. Jede Prompt-Art (`HostKeyUnknown`,
  `HostKeyChanged`, `Password`, `Passphrase`, `KeyboardInteractive`) beendet den Versuch sofort mit dem neuen Fehler
  `CoreError::InputRequired`. An die UI geht kein Prompt-Event (fail closed).
- Secrets kommen nur aus dem Keyring. Hat der Host kein gespeichertes Secret und verlangt der Server eines, endet der
  Versuch mit `inputRequired`.
- Connect-Events der Versuche (Schritte, Log) gehen nicht an die UI.

### Einstellung

- Neuer Settings-Schlüssel `terminal_auto_reconnect` (bool, Standard `true`) im bestehenden Settings-Store, wie
  `terminal_sidebar_open`. Eine Migration ist nicht nötig.
- Die Sitzung liest den Wert beim Abbruch, also nicht beim Verbinden.

## 2. Tauri

- `connect_ssh` registriert eine `ReconnectingSession` statt der nackten `SshSession`. Der Connector kapselt
  `build_request` und die Pipeline mit dem nicht interaktiven Modus.
- `SessionEventDto` bekommt drei Varianten:
  - `reconnecting { attempt, maxAttempts, retryInMs, reason: CoreError }`
  - `reconnected { info: SessionInfo }`
  - `lost { reason: CoreError }`
- `CoreError` bekommt die Variante `inputRequired`.
- Neue Commands:
  - `session_reconnect_now(sessionId)`
  - `session_reconnect_stop(sessionId)`
  - Beide sind für lokale oder unbekannte Sitzungen kein Fehler, sie tun dann nichts.
- `Settings` bekommt `terminalAutoReconnect` (Standard `true`).
- Bindings werden mit `cargo test -p linascp exports_bindings` neu erzeugt.
- Das Aufräumen einer Sitzung (`cleanup_fn`) läuft erst nach dem abschließenden Event, nicht bei `Lost` der inneren
  Verbindung.

## 3. Oberfläche

### Zustand (`stores/sessions.ts`, `stores/sessionEvents.ts`)

- `TabStatus` bekommt `"reconnecting"` und `"lost"`.
- `Tab.reconnect?: { attempt: number; maxAttempts: number; retryAt: number; reason: CoreError }`. `retryAt` ist
  Client-Zeit in ms (`Date.now() + retryInMs`).
- `sessionEventPatch`:
  - `reconnecting` → `{ status: "reconnecting", reconnect }`
  - `reconnected` → `{ status: "connected", info, connectedAt: Date.now(), reconnect: undefined, error: undefined }`
  - `lost` → `{ status: "lost", error: reason, reconnect: undefined }`
- Broadcast (`broadcastPeers`) berücksichtigt weiter nur `"connected"`. Panes in `reconnecting` oder `lost` werden
  übersprungen.

### Leiste während des Neuverbindens (`ReconnectBar`)

- In der Pane oben, über dem Terminal. Sie verdeckt nicht die ganze Ausgabe, und das Terminal bleibt scrollbar.
- Text: „Verbindung unterbrochen · Versuch {n} von {max} in {s} s“. Der Countdown läuft sekündlich. Bei 0 steht dort
  „Verbinde …“.
- Buttons:
  - „Jetzt versuchen“ ruft `session_reconnect_now` auf.
  - „Abbrechen“ ruft `session_reconnect_stop` auf.
- Die Leiste ist eine Region mit `role="status"`. Die Ansage beschränkt sich auf den Versuch, nicht jede Sekunde.

### Nach Erfolg

`TerminalView` schreibt bei einem Statuswechsel `reconnecting` → `connected` eine gedimmte Trennzeile ins xterm:
`\r\n— Verbindung wiederhergestellt —\r\n`. Der Text kommt aus `de.ts`. Danach folgt die Ausgabe der neuen Shell.

### Aufgegeben (`SessionEnded`, Status `lost`)

- Titel: „Verbindung verloren“, dazu der Grund aus `errorMessage(error)`. Für `inputRequired` lautet er „Für die
  Verbindung ist eine Eingabe nötig (Passwort, Host-Key …)“.
- Buttons:
  - **Netzwerk, Timeout, Abbruch und Ähnliches:**
    - „Erneut versuchen“ ruft `session_reconnect_now` auf. Der Verlauf bleibt, die Leiste erscheint wieder.
    - „Neu verbinden“ ist der interaktive Weg über `replacePane` mit frischem Terminal.
    - „Schließen“.
  - **`inputRequired`:** nur „Neu verbinden“ und „Schließen“.
  - **`notFound` (Host gelöscht):** nur „Schließen“.
- „Neu verbinden“ schließt die alte Sitzung (`session_close`) wie bisher.

### Tab-Leiste, Seitenleiste, Snippets

- Statuspunkt: `reconnecting` pulsiert bernsteinfarben, `lost` ist grau wie „beendet“. Screenreader hören den Zusatz
  „– Verbindung unterbrochen“ bzw. „– Verbindung verloren“.
- Seitenleiste „Sitzung“: Bei `reconnecting` zeigt sie „Neuverbindung · Versuch n von 8“ statt der Verbindungsdauer.
  Nach `reconnected` zeigt sie das neue `info`.
- Snippets: Einfügen und ▶ sind gesperrt, weil der Status ≠ `connected` ist. Das ist bestehendes Verhalten.

### Einstellungen

Im Settings-Dialog, Bereich Terminal, gibt es eine Checkbox „Abgebrochene SSH-Verbindungen automatisch
wiederherstellen“. Sie bindet `terminalAutoReconnect`.

Alle Strings stehen in `ui/src/i18n/de.ts`.

## 4. Fehlerfälle

| Fall | Verhalten |
|---|---|
| `exit` in der Shell, Server schließt den Kanal | `exit` wie bisher, kein Reconnect |
| Netz weg, Keepalive-Timeout, Server-Neustart | `reconnecting`, bis zu 8 Versuche |
| Schreibfehler auf toter Verbindung (`keepalive_s = 0`) | `reconnecting` |
| Auto-Reconnect aus | sofort `lost` mit Grund |
| Versuch braucht Passwort, Passphrase, 2FA oder Host-Key-Entscheidung | `inputRequired`, kein weiterer Versuch, `lost` |
| Host-Key geändert | wie oben, nie automatisch akzeptiert |
| Host inzwischen gelöscht | `lost` mit `notFound` |
| „Abbrechen“ in der Leiste | Die Schleife stoppt, `lost` (`cancelled`), der Verlauf bleibt |
| Pane, Tab oder App wird während eines Versuchs geschlossen | Die Schleife stoppt, die halb aufgebaute Verbindung wird geschlossen, kein spätes Event |
| `stop` und Erfolg gleichzeitig | Wer zuerst die Sitzung sperrt, gewinnt; genau ein Ergebnis (`reconnected` oder `lost`) |
| Eingabe während des Neuaufbaus | verworfen |
| Resize während des Neuaufbaus | gemerkt, die neue PTY bekommt die letzte Größe |
| Erneuter Abbruch nach erfolgreichem Reconnect | wieder 8 Versuche |
| Broadcast an, ein Peer verbindet neu | Der Peer wird übersprungen |

Bekannte Grenze: Die Erkennung hängt vom Keepalive des Hosts ab (Standard 30 s × 3, also bis zu 90 s) oder vom
nächsten Schreibversuch.

## 5. Tests

- **Rust, Core:**
  - Erkennung: `Close` ergibt `Exit`, Kanalende ohne `Close` ergibt `Lost`, ein Schreibfehler ergibt einmalig `Lost`.
  - `ReconnectingSession` mit `tokio::time::pause` und Fake-Connector:
    - Backoff-Folge und Event-Reihenfolge
    - Erfolg beim 3. Versuch, dann `reconnected`, mit gleichem Sink
    - Aufgeben nach 8 Versuchen
    - `inputRequired` und `notFound` beenden sofort
    - Einstellung aus ergibt sofort `lost`
    - `reconnect_now` in der Wartezeit und nach `lost`
    - `stop` und `close` während eines Versuchs, auch beim Wettlauf mit einem Erfolg
    - Eingabe wird verworfen, die letzte Größe wird angewendet
    - genau ein abschließendes Event
    - Neustart der Zählung nach Erfolg
  - Nicht interaktiver Prompt-Modus: Jede Prompt-Art ergibt `inputRequired`, und es wird kein Prompt-Event gesendet.
  - Integration mit Docker-sshd (`--ignored`): Server-Verbindung kappen, dann Reconnect, dann Ausgabe der neuen Shell
    unter derselben Session-ID.
- **Rust, Tauri:**
  - DTO-Mapping der neuen Events
  - `session_reconnect_now` und `session_reconnect_stop` für unbekannte und lokale Sitzungen
  - `terminalAutoReconnect` mit Standardwert und Roundtrip
  - Bindings
- **UI:**
  - `sessionEventPatch` für die drei Events
  - `ReconnectBar`: Countdown, „Jetzt versuchen“, „Abbrechen“
  - `SessionEnded` in der Variante „Verbindung verloren“ mit den Buttons je nach Grund
  - Trennzeile nach `reconnected`
  - Broadcast überspringt `reconnecting`
  - Tab-Statuspunkt und Screenreader-Zusatz
  - Seitenleiste
  - Checkbox in den Einstellungen
- **Manuell:** `docs/acceptance-tp2d.md`, unter anderem:
  - WLAN aus und wieder an
  - Standby
  - Den sshd-Prozess der Sitzung auf dem Server beenden (`systemctl restart ssh` allein trennt bestehende
    Sitzungen nicht)
  - Host-Key ändern
  - Passwort-Host ohne gespeichertes Secret
  - Einstellung aus
