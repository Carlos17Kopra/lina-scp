# LinaSCP TP6c: Identitäten des SSH-Agenten, Standardschlüssel

Stand: 2026-11-02. Teil 3 von TP6 (Aufteilung in `2026-10-17-linascp-tp6a-cleanup-design.md`).

Der Nutzer hat auf die Fragerunde verzichtet („leg einfach los“). Alle Entscheidungen unten sind Defaults des
Autors, nicht abgestimmt.

## Ziel

- Der Schlüsselbund zeigt die Identitäten des laufenden SSH-Agenten (`SSH_AUTH_SOCK`).
- Die Anmeldung „SSH-Agent“ probiert wie OpenSSH danach die Standardschlüssel `~/.ssh/id_*`. Das betrifft vor allem
  Hosts, die ohne `IdentityFile` aus der ssh-config importiert wurden (Backlog-Punkt).

Nicht enthalten:
- Schlüssel in den Agenten laden oder daraus entfernen (`ssh-add`), den Agenten sperren.
- Ein eigener Agent in der App, Agent-Weiterleitung (`ForwardAgent`).
- Verschlüsselte Standardschlüssel, die nicht im Agenten sind (siehe 2).

## 1. Identitäten lesen (Core, `crates/core/src/agent.rs`)

- `list_agent_identities(socket: Option<&Path>) -> Result<AgentStatus>`.
- `AgentStatus { socket: Option<String>, identities: Vec<AgentIdentity> }`. `socket` ist `None`, wenn
  `SSH_AUTH_SOCK` nicht gesetzt ist; dann ist die Liste leer und es gibt keinen Fehler.
- `AgentIdentity { algorithm, fingerprint (SHA256), comment, public_key (OpenSSH-Zeile) }`, in der Reihenfolge des
  Agenten.
- Ist der Socket gesetzt, aber nicht erreichbar oder antwortet der Agent nicht binnen 5 s, ist das ein Fehler mit Grund.
- Command `agent_identities` (in `src-tauri/src/commands/keys.rs`), Socket aus `SSH_AUTH_SOCK` wie beim Verbinden.

## 2. Standardschlüssel nach dem Agenten (Core, `connect/auth.rs`)

- Die Methode „SSH-Agent“ probiert zuerst alle Identitäten des Agenten. Ist keine angenommen worden, ist kein Agent
  gesetzt oder ist er nicht erreichbar, folgen die vorhandenen Dateien in OpenSSH-Reihenfolge: `id_rsa`, `id_ecdsa`,
  `id_ed25519` in `~/.ssh`. `id_*_sk` fehlen, weil russh mit Sicherheitsschlüsseln nicht signieren kann.
- Übersprungen werden (je mit `debug1:`-Zeile im Verbindungslog):
  - Dateien, deren Schlüssel der Agent schon angeboten hat (verglichen wird der öffentliche Teil der privaten Datei,
    eine `.pub` ist nicht nötig);
  - verschlüsselte Dateien. Es gibt keinen Ort, ihre Passphrase zu speichern, und automatische Reconnects dürfen
    nie fragen. Wer sie braucht, lädt sie in den Agenten oder wählt die Methode „Schlüsseldatei“.
  - unlesbare oder ungültige Dateien.
- Wird eine Datei angenommen, ist die Methode erfolgreich (oder teilweise, wie bisher). Sonst bleibt der Fehler
  „no agent identity or default key was accepted“; fehlte der Agent und gibt es keine Datei, bleibt die bisherige
  Meldung über den fehlenden Agenten.
- Trennt der Server während der Standardschlüssel die Verbindung (OpenSSH bei überschrittenem `MaxAuthTries`), ist
  das ein Anmeldefehler, kein Netzwerkfehler. So wiederholt der Auto-Reconnect keinen dauerhaften Fehler.
- `~/.ssh` kommt aus den App-Pfaden (`ConnectDeps.ssh_dir`), nicht direkt aus `HOME`.
- Der Hinweis im Editor lautet: „Schlüssel werden vom laufenden SSH-Agent bezogen (SSH_AUTH_SOCK), danach aus
  ~/.ssh/id_* ohne Passphrase.“

## 3. Screen „Schlüsselbund“

- Unter den Schlüsseldateien ein Abschnitt „SSH-Agent“ mit einer Karte je Identität: Kommentar (oder Fingerprint,
  wenn leer) als Name, Typ, Fingerprint (mono). Aktionen wie bei Dateien: „Öffentlichen Schlüssel kopieren“,
  „Auf Host eintragen …“ (derselbe Dialog).
- Zustände: „Kein SSH-Agent gefunden (SSH_AUTH_SOCK ist nicht gesetzt).“, „Der SSH-Agent hat keine Schlüssel
  geladen.“, Fehler rot mit Grund.
- Eine Schlüsseldatei, deren Fingerprint im Agenten ist, zeigt zusätzlich „Im SSH-Agent geladen“.
- Beide Listen laden beim Öffnen, „Neu laden“ im Abschnitt lädt den Agenten neu.

## 4. Mirror: importierte Hosts bestätigen (Backlog, Sicherheit)

Ein präparierter Import konnte einen Alias ohne Punkt (`gitlab`, `localhost`) mit fremdem `HostName` anlegen. Der
Mirror wird vor `~/.ssh/config` eingebunden und hätte damit `ssh gitlab` umgeleitet.

- Hosts bekommen `mirror_pending` (DB-Spalte, Schema V5, Standard 0). Ein Import setzt es für jeden geschriebenen
  Host, auch beim Ersetzen.
- Der Mirror überspringt solche Hosts. Hosts, die über sie springen, fallen mit weg (bestehende Regel).
- Nach dem Import zeigt der Dialog jeden betroffenen Host als „alias → adresse“ mit „In ~/.ssh/config übernehmen“
  und „Später“. Bestätigen ruft `hosts_confirm_mirror`. Gezeigt wird das in beiden ssh-Modi, damit ein späteres
  Einschalten von „Spiegeln“ keine unbestätigten Hosts schreibt.
- Speichern im Host-Editor bestätigt den Host, weil dort Name und Adresse sichtbar sind. Andere Speicherwege (z. B.
  das Terminal-Design) behalten das Merkmal.

## Tests

- Core: `list_agent_identities` gegen einen echten `ssh-agent` (in der Docker-Suite, weil das Fixture dort entsteht),
  ohne Socket und mit totem Socket; Fallback, Dedup und Überspringen mit dem Fake-`Authenticator`.
- UI: Abschnitt mit Identitäten, die drei Zustände, Markierung „Im SSH-Agent geladen“.
