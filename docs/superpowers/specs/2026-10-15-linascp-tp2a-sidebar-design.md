# LinaSCP TP2a: Terminal-Seitenleiste, Farbschemata, Sitzungsinfo

Stand: 2026-10-15. Teil 1 von TP2 („SSH-Terminal komplett“). Reihenfolge der TP2-Teilprojekte:
2a Seitenleiste + Design + Sitzung → 2b Snippets → 2c Split-Panes + Broadcast → 2d Auto-Reconnect.
Jedes Teilprojekt hat eine eigene Spec, einen eigenen Plan und einen eigenen Branch.

Vorlage: `shellport-design.zip`, `screens/SSH.dc.html` (rechte Seitenleiste mit den Reitern Snippets, Tunnel,
Sitzung, Design) und `tokens/terminal-schemes.json` (liegt bereits als `ui/src/screens/terminal/schemes.json` vor).

## Ziel

- Das Terminal-Farbschema ist wählbar, unabhängig vom App-Design. Schema und Schriftgröße gelten global und lassen
  sich pro Host überschreiben.
- Eine einklappbare Seitenleiste im Terminal zeigt die Daten der aktiven Sitzung und die Design-Einstellungen.

Nicht enthalten:
- Snippets (2b), Split-Panes und Broadcast (2c), Auto-Reconnect (2d), Tunnel (TP5).
- Eine Gruppen-Ebene für Design-Einstellungen.
- Live-Werte (Bytes, Latenz), Zoom-Kürzel und eigene Farbschemata.

## 1. Daten und Einstellungen

### Settings

`Settings` bekommt zwei neue Felder:

| Feld | Typ | Standard | Bedeutung |
|---|---|---|---|
| `terminalScheme` | `String` | `"auto"` | Schema-ID aus `schemes.json`, oder `"auto"` |
| `terminalSidebarOpen` | `bool` | `false` | Seitenleiste ein- oder ausgeklappt |

- `"auto"` behält das TP1-Verhalten bei: dunkles App-Design → `linascp`, helles App-Design → `linascpLight`.
- Fehlende Keys in der DB liefern den Standardwert. Für Settings ist deshalb keine Migration nötig.

### Host

`Host` bekommt `terminal_scheme: Option<String>` und `terminal_font_size: Option<u16>` (wie `Settings.terminal_font_size`).

- Migration V2: `ALTER TABLE hosts ADD COLUMN terminal_scheme TEXT` und `ADD COLUMN terminal_font_size INTEGER`,
  beide nullbar.
- Export und Import übernehmen die Felder automatisch, weil sie Teil von `Host` sind. Ältere `.linascp`-Dateien
  ohne die Felder ergeben `None` (`#[serde(default)]`).
- Der Mirror (`~/.ssh/config.d/linascp.conf`) ignoriert beide Felder.

### Validierung im Core (`validate.rs`)

- Schriftgröße (Host und Settings): 10–20, entsprechend `schemes.json` `fontSize.min/max`.
- Schema (Host und Settings): `"auto"` oder `[A-Za-z]{1,32}`. Der Core kennt die Schema-Liste nicht.
- Eine ID, die die UI nicht kennt (etwa aus einem Import einer neueren Version), behandelt die UI wie `"auto"`.
  Sie ist kein Fehler.

### Effektiver Wert pro Tab

- `scheme = host?.terminalScheme ?? settings.terminalScheme` und
  `fontSize = host?.terminalFontSize ?? settings.terminalFontSize`.
- Lokale Terminals haben keinen Host und verwenden nur die globalen Werte.

## 2. Sitzungsinfo (Backend)

Der Command `connect_ssh` liefert statt der `SessionId` ein `ConnectedSession { sessionId, info: SessionInfo }`.
Die UI verwendet schon heute den Rückgabewert des Commands und nicht das `Connected`-Event, und Channel-Nachrichten
haben keine garantierte Reihenfolge zur Invoke-Antwort. `ConnectEvent::Connected` bleibt deshalb unverändert. Die
Info entsteht einmal pro Verbindung und ändert sich danach nicht. Rekeys werden ignoriert. Im Core hängt sie an
`SshSession` (`SshSession::info()`).

```rust
pub struct SessionInfo {
    pub host_key_algorithm: String,  // z. B. "ssh-ed25519"
    pub host_key_sha256: String,     // "SHA256:…"
    pub kex: String,                 // z. B. "curve25519-sha256"
    pub cipher: String,              // z. B. "chacha20-poly1305@openssh.com"
    pub mac: String,                 // client→server MAC, unverändert so, wie russh ihn liefert (bei AEAD-Ciphern ein Platzhalter von russh)
    pub auth_methods: Vec<AuthMethod>, // die Methoden der erfolgreichen Anmeldung, in Reihenfolge (2FA: mehrere)
}
```

- `kex`, `cipher` und `mac` kommen aus `client::Handler::kex_done` (`negotiation::Names`) des Ziel-Hosts. Der
  Handler legt sie in einem geteilten Slot ab, den die Pipeline nach dem Auth-Schritt ausliest.
- `host_key_*` kommen aus `check_server_key`. Es ist dieselbe `HostKeyInfo`, die auch der Host-Key-Prompt
  verwendet.
- `auth_methods` meldet der Auth-Schritt zurück (`auth::authenticate` liefert die verwendeten Methoden).
- Fehlt ein Wert, etwa weil `kex_done` nicht aufgerufen wurde, bleibt das Feld ein leerer String und die UI zeigt
  „—“. Die Verbindung scheitert daran nicht.
- Die Struktur enthält keine Secrets. Sie darf nicht um Benutzernamen aus Prompts, Passwörter, Passphrasen oder
  Key-Inhalte erweitert werden.
- Adresse, Port, Benutzer, Jump-Host, Keepalive und Login-Profil kennt die UI bereits aus dem `Host`. Sie stehen
  deshalb nicht in `SessionInfo`.

Der Sessions-Store der UI speichert pro Tab `info?: SessionInfo` und `connectedAt: number` (Client-Zeit beim
Connected-Event, bei lokalen Terminals beim Öffnen).

## 3. Seitenleiste (UI)

- Ein Icon-Button rechts in der Tab-Leiste (`aria-pressed`) und `Ctrl+Shift+B` im Terminal schalten die Leiste um.
  Der Zustand wird in `settings.terminalSidebarOpen` gespeichert.
- Breite: 280 px. Ist die Leiste eingeklappt, bekommt das Terminal die volle Breite. Nach dem Umschalten passt das
  Terminal seine Größe an (Fit), und die neue Größe geht per `sessionResize` an das Backend.
- Reiter (Tabs-Komponente aus dem Kit):
  - **Sitzung**: Standard.
  - **Design**.
  - **Tunnel**: deaktiviert, mit dem Hinweis „Folgt in einer späteren Version“.
  - **Snippets** kommt mit 2b dazu.
- Die Leiste zeigt immer die Daten des aktiven Tabs. Gibt es keinen Tab, wird sie nicht gerendert.
- Alle Texte stehen in `ui/src/i18n/de.ts`.

### Reiter „Sitzung“

SSH-Tab, als Definitionsliste:

| Zeile | Quelle |
|---|---|
| Host | `user@address:port` |
| Jump-Host | Name des Jump-Hosts oder „—“ |
| Anmeldung | Methoden (deutsche Labels, mit „ + “ verbunden) plus Profilname, falls ein Profil genutzt wird |
| Host-Key | Algorithmus + SHA256-Fingerprint, mit Kopieren-Button (Terminal-Clipboard-Modul) |
| Schlüsselaustausch | `kex` |
| Verschlüsselung | `cipher`, `mac` |
| Terminal | `xterm-256color · {cols}×{rows}` |
| Keepalive | „alle {n} s“ oder „aus“ bei 0 |
| Verbunden seit | „14:02 (2 Std. 14 Min.)“, minütlich aktualisiert |

- Lokaler Tab: „Lokale Shell“, Terminalgröße und „Geöffnet seit“.
- Ist die Sitzung beendet, bleiben die Daten sichtbar, und „Verbunden seit“ zeigt „getrennt“.

### Reiter „Design“

- Eine Liste mit „Wie App-Design“ und den 9 Schemata. Jeder Eintrag zeigt eine Mini-Vorschau wie im Mockup:
  Hintergrund, Prompt in Grün, Pfad in Blau, warn in Gelb, error in Rot. Auswahl per Radio-Semantik
  (`role="radiogroup"`).
- Schriftgröße: `A−` · „13 px“ · `A+`. Die Buttons sind an den Grenzen deaktiviert.
- Der Schalter „Gilt für“ hat die Optionen „Alle Sitzungen“ und „Nur diesen Host“:
  - Vorausgewählt ist „Nur diesen Host“, sobald der Host ein eigenes Schema oder eine eigene Schriftgröße hat.
  - Wer „Nur diesen Host“ wählt, kopiert die aktuellen effektiven Werte an den Host (`hostSave`). Weitere
    Änderungen gehen an den Host.
  - Wer „Alle Sitzungen“ wählt, setzt beide Host-Felder auf `None`. Weitere Änderungen gehen an die Settings.
  - „Nur diesen Host“ ist deaktiviert bei lokalen Tabs, bei Hosts mit `source = sshConfig` und bei gelöschten
    Hosts. Ein Hinweis unter dem Schalter nennt den Grund.
- Jede Änderung speichert sofort und wirkt sofort auf alle Tabs, deren effektiver Wert sich ändert.
- Scheitert das Speichern, erscheint ein Fehler-Toast, und die Anzeige springt auf den gespeicherten Wert zurück.

### Einstellungsdialog

- Neben der Schriftgröße kommt ein Auswahlfeld „Farbschema“ dazu, mit „Wie App-Design“ und den 9 Schemata.
- Beide Felder bearbeiten nur die globalen Werte.

## 4. Farbschema → xterm

- `terminalTheme(schemeId, appTheme)` ersetzt `terminalTheme(appTheme)`. `"auto"` oder eine unbekannte ID ergibt
  das bisherige Verhalten.
- Die Zuordnung der Farben bleibt wie in TP1:
  - Bright-Varianten haben dieselben Farben wie die normalen.
  - `black` = `bd`, `brightBlack` = `dim`.
  - Für Magenta und Cyan gibt es keine Werte, sie bleiben beim xterm-Standard.
- Der Hintergrund ist weiterhin deckend.
- Ein Schemawechsel setzt `term.options.theme` und `fontSize` am laufenden Terminal. Danach folgt ein Fit mit
  Resize, weil sich die Zellgröße mit der Schriftgröße ändert.

## 5. Fehlerfälle

- Unbekannte Schema-ID aus DB oder Import: die UI verwendet `"auto"`, ohne Fehlermeldung.
- Der Host wird gelöscht, während sein Tab offen ist: der Tab fällt auf die globalen Werte zurück, und „Nur diesen
  Host“ ist deaktiviert.
- `hostSave` oder `settingsSet` scheitert: Toast, Rücksprung auf den letzten gespeicherten Wert.
- Ein Validierungsfehler des Cores, etwa wegen manipulierter DB-Werte, wird als `CoreError::Config` gemeldet.

## 6. Tests

Rust (`crates/core`):
- Migration V2 auf einer V1-DB: die Spalten existieren, und bestehende Hosts haben `None`.
- Host-Roundtrip mit und ohne Override. Ein Import einer alten Export-Datei ohne die Felder funktioniert.
- Validierung: Schriftgröße 9 und 21 werden abgelehnt, 10 und 20 akzeptiert. Schema `"auto"`, `"nord"` und
  `"x".repeat(32)` sind gültig, `""`, `"a b"`, `"../x"` und 33 Zeichen ungültig.
- Settings: neue Keys mit Standardwerten, Roundtrip.
- Mirror-Ausgabe unverändert mit gesetzten Overrides.
- Docker-sshd-Integrationstest: `Connected.info` hat nicht-leere Werte für `kex`, `cipher` und `host_key_sha256`.
  Der Fingerprint entspricht dem Host-Key des Containers, und `auth_methods` entspricht den verwendeten Methoden.

UI (vitest):
- `terminalTheme`: alle 9 Schemata ergeben deckende Hintergründe. `"auto"` folgt dem App-Design, eine unbekannte
  ID verhält sich wie `"auto"`.
- Auflösung der effektiven Werte: Host-Override, globaler Wert, lokaler Tab.
- Seitenleiste: Button und `Ctrl+Shift+B` schalten um und speichern die Einstellung. Nach dem Umschalten folgt ein
  Resize.
- Reiter „Sitzung“: Zeilen aus `Host` und `SessionInfo`. „—“ bei fehlenden Werten, Kopieren des Fingerprints,
  lokale Variante.
- Reiter „Design“: Schemawahl und Schriftgröße gehen je nach „Gilt für“ an die Settings oder an den Host.
  Umschalten auf „Alle“ löscht die Overrides. Die Grenzen deaktivieren A− und A+. Die Deaktivierungsfälle sind
  abgedeckt. Ein Speicherfehler führt zum Rücksprung.
- Einstellungsdialog: Auswahlfeld für das Farbschema.

Die Testausgabe bleibt frei von Warnungen.
