# LinaSCP TP5b: Schlüsselbund, Schlüssel erzeugen, ssh-copy-id

Stand: 2026-10-17. Teil 2 von TP5 (Aufteilung in `2026-10-16-linascp-tp5a-tunnels-design.md`).

Der Nutzer hat auf die Fragerunde verzichtet („leg mit 5b los“). Alle Entscheidungen unten sind Defaults des
Autors, nicht abgestimmt.

## Ziel

- Ein Rail-Screen „Schlüsselbund“ zeigt die SSH-Schlüssel des Nutzers mit Typ, Fingerprint, Kommentar und wer sie
  verwendet.
- Neue Schlüssel lassen sich erzeugen (Ed25519, RSA 4096), aus dem Screen und im Editor neben „Datei wählen“.
- Der öffentliche Schlüssel lässt sich kopieren und auf einem Host eintragen (wie `ssh-copy-id`).

Nicht enthalten:
- Schlüssel in der App speichern. Private Schlüssel bleiben Dateien (TP1), gespeichert wird nur der Pfad.
- Identitäten des SSH-Agenten anzeigen, Schlüssel in den Agenten laden.
- Passphrase eines bestehenden Schlüssels ändern, Schlüssel löschen oder umbenennen.
- Schlüssel ohne `.pub`-Datei (der öffentliche Teil ließe sich bei verschlüsselten Schlüsseln nicht lesen).
- Eintragen auf mehreren Hosts auf einmal.

## 1. Schlüssel finden (Core, `crates/core/src/keys.rs`)

- Gelesen werden `~/.ssh` und `~/.ssh/linascp` (Importziel aus TP1), je eine Ebene.
- Ein Schlüssel ist eine Datei `X`, neben der `X.pub` liegt. `X.pub` muss ein gültiger OpenSSH-Public-Key sein,
  sonst wird das Paar übergangen.
- `KeyInfo { path, name, algorithm, fingerprint (SHA256), comment, public_key (OpenSSH-Zeile), encrypted }`.
  `encrypted` ist `None`, wenn die private Datei kein OpenSSH-Format hat (PEM).
- Ein fehlendes Verzeichnis ist leer, ein unlesbares ein Fehler.

## 2. Erzeugen

- Dialog „Neuer Schlüssel“: Typ (Segmented „Ed25519“ / „RSA 4096“, Standard Ed25519), Dateiname, Kommentar,
  Passphrase und Wiederholung (beide leer = ohne Passphrase).
- Vorschläge: Name `id_ed25519` bzw. `id_rsa`, wenn frei, sonst mit `_linascp`, `_linascp2`, … dahinter;
  Kommentar `<user>@<hostname>`.
- Geschrieben wird nach `~/.ssh/<name>` (Mode 0600) und `~/.ssh/<name>.pub` (0644), `~/.ssh` bei Bedarf mit 0700.
  Eine vorhandene Datei wird nie überschrieben: dann Fehler „existiert bereits“.
- Der Name ist ein einzelner Dateiname (`validate_file_name`), ohne führenden Punkt und nicht auf `.pub` endend.
- Im Editor füllt der erzeugte Schlüssel den Schlüsselpfad, eine Passphrase das Passphrase-Feld (sie wird dann mit
  dem Host bzw. Profil im OS-Schlüsselbund gespeichert, wie eine getippte).
- Aus dem Screen erzeugt, wird die Passphrase nirgends gespeichert.

## 3. Screen „Schlüsselbund“

- Rail-Eintrag ist aktiv. Kopf: Titel und „Neuer Schlüssel“.
- Je Schlüssel eine Karte: Name, Typ, Fingerprint (mono), Kommentar, „Mit Passphrase“ / „Ohne Passphrase“,
  Pfad, „Verwendet von: web, Profil deploy“ (Hosts und Login-Profile, deren Schlüsselpfad auf die Datei zeigt,
  `~` aufgelöst) oder „Nicht verwendet“.
- Aktionen: „Öffentlichen Schlüssel kopieren“ (Zwischenablage), „Auf Host eintragen …“.
- Leer: „Keine Schlüssel in ~/.ssh gefunden.“ Fehler beim Lesen: rot mit Grund.
- Die Liste lädt beim Öffnen des Screens und nach dem Erzeugen neu.

## 4. Auf Host eintragen (ssh-copy-id)

- Dialog: Auswahl des Hosts (alle Hosts), „Eintragen“. Verbunden wird mit der normalen Anmeldung des Hosts; Fragen
  (Host-Key, Passwort, 2FA) erscheinen im Dialog wie bei „Verbindung testen“.
- Core `copy_public_key`: Verbindung bis zur Anmeldung, dann ein `exec` von `sh` mit festem Skript; der Schlüssel
  kommt über stdin, nie in die Kommandozeile. Das Skript:
  - legt `~/.ssh` (0700) und `~/.ssh/authorized_keys` (0600, `umask 077`) an,
  - fügt die Zeile an, wenn sie nicht genau so schon drin steht (fehlendes Zeilenende davor wird ergänzt),
  - endet mit 0 (eingetragen), 3 (war schon da) oder einem Fehler.
- Ergebnis im Dialog: „Schlüssel eingetragen.“ / „Der Schlüssel war schon eingetragen.“ / Fehler mit Grund.
- Der Schlüssel wird in Core geprüft (eine Zeile, gültiger OpenSSH-Public-Key).

## 5. Tauri

- `keys_list() -> Vec<KeyInfo>`
- `key_generate(kind, name, comment, passphrase: Option<String>) -> KeyInfo`
- `key_defaults(kind) -> KeyDefaults { name, comment }` (Vorschläge aus 2)
- `key_copy_id(connect_id, host_id, public_key, events) -> bool` (`true` = eingetragen), Abbruch über
  `connect_cancel`.

## 6. Invarianten

- Private Schlüssel werden mit 0600 geschrieben, nie überschrieben und nie übertragen. Die Passphrase landet nur
  im Schlüssel selbst und (nur aus dem Editor) im OS-Schlüsselbund.
- `copy_public_key` sendet nur den geprüften öffentlichen Schlüssel, über stdin.

## 7. Tests

- Core-Unit `keys`: Liste mit Paaren, ohne `.pub`, kaputter `.pub`, PEM, fehlendes Verzeichnis; Erzeugen Ed25519
  mit und ohne Passphrase (Modi, Datei lesbar und mit der Passphrase entschlüsselbar), vorhandene Datei, ungültige
  Namen, Namensvorschlag.
- Docker: `copy_public_key` trägt ein, zweiter Aufruf meldet „schon da“, Rechte 0600/0700, kein doppelter Eintrag;
  `authorized_keys` ohne Zeilenende.
- Tauri: `key_copy_id` mit unbekanntem Host meldet genau ein `Failed`.
- UI: Screen listet, „Verwendet von“, kopiert, leerer Zustand; Dialog „Neuer Schlüssel“ prüft Wiederholung und
  ruft `keyGenerate`; Editor übernimmt Pfad und Passphrase; Eintragen-Dialog ruft `keyCopyId` und zeigt das
  Ergebnis.
- Abnahme: `docs/acceptance-tp5b.md`.
