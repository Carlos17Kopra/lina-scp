# LinaSCP TP5a: Tunnel (Port-Forwarding)

Stand: 2026-10-16. Teil 1 von TP5. Die Port-Forward-Regeln werden seit TP1 mit dem Host gespeichert
(`PortForward { kind: L|R|D, bind, target, autostart }`); ab hier wirken sie.

Der Nutzer hat auf die Fragerunde verzichtet („mach bitte mit tp5 weiter“, vorher „ich vertraue dir“). Alle
Entscheidungen unten sind Defaults des Autors, nicht abgestimmt.

## Aufteilung von TP5

- **5a (dieses Dokument):** Tunnel L/R/D über eine Terminal-Sitzung, Reiter „Tunnel“ in der Seitenleiste.
- **5b:** Schlüsselbund-Screen, Schlüssel erzeugen, öffentlichen Schlüssel auf Hosts verteilen (ssh-copy-id).
- **5c:** Einstellungs-Screen komplett.
- **Später, bei Bedarf:** eigener Rail-Screen „Port-Forwarding“ mit Tunneln ohne Terminal; Regeln ad hoc in der
  Seitenleiste anlegen; Unix-Sockets.

## Ziel

- Die Regeln eines Hosts laufen über die SSH-Verbindung seines Terminal-Tabs, wie `ssh -L/-R/-D`.
- Regeln mit „Auto-Start“ starten nach dem Verbinden von selbst. Alle anderen startet und stoppt der Nutzer im
  Reiter „Tunnel“.
- Nach einer automatischen Neuverbindung laufen die Tunnel weiter, ohne Zutun.

Nicht enthalten: siehe „Später“. SFTP-Tabs öffnen keine Tunnel.

## 1. Regeln

- `bind`: `[adresse:]port`. IPv6 in Klammern (`[::1]:8080`). Ohne Adresse:
  - L und D: `127.0.0.1` (wie OpenSSH ohne `GatewayPorts`).
  - R: `localhost` (der Server bindet dann nach seiner eigenen Regel, meist Loopback).
- `target` (nur L und R): `host:port`, IPv6 in Klammern. Bei D leer.
- Port 1–65535; bei R ist `0` erlaubt (der Server wählt, der gewählte Port wird angezeigt).
- Geprüft wird in Core (`validate.rs`, `parse_forward`). Eine ungültige Regel startet nicht und zeigt den Grund;
  der Host-Editor zeigt denselben Grund beim Speichern.

## 2. Bedienung (Reiter „Tunnel“)

- Der Reiter ist aktiv. Er listet die Regeln des Hosts: Badge L/R/D, Text `127.0.0.1:8080 → localhost:80`
  (bei D `127.0.0.1:1080 · SOCKS`), Zustand und ein Schalter „Starten“/„Stoppen“.
- Zustände: „Aus“, „Startet …“, „Aktiv“ (bei R mit Port 0: „Aktiv · Port 41234“), „Fehler: …“ (rot).
- Keine Regeln: „Für diesen Host sind keine Regeln angelegt.“ und ein Link „Host bearbeiten“.
- Lokale Terminals und Hosts aus `~/.ssh/config` ohne Regeln: derselbe Leertext (ohne Link bei ssh-config).
- Die Regeln gelten so, wie sie beim Verbinden gespeichert waren. Änderungen im Editor wirken im nächsten Tab.
- Schalter sind aus, solange die Sitzung nicht verbunden ist (Neuverbindung, beendet).

## 3. Ablauf (Core, `crates/core/src/tunnel.rs`)

- `Tunnels` gehört zu einer Terminal-Sitzung und überlebt deren Neuverbindungen. Es kennt die aktuelle
  Verbindung über die Schnittstelle `TunnelLink` (direct-tcpip öffnen, `tcpip-forward` anfordern/aufheben), damit
  die Logik ohne SSH testbar ist.
- **L:** `TcpListener` auf `bind`. Je eingehender Verbindung ein `direct-tcpip`-Kanal zum Ziel, dann Daten in
  beide Richtungen (`copy_bidirectional`). Ohne Verbindung (während der Neuverbindung) wird die eingehende
  Verbindung sofort geschlossen; der Listener bleibt.
- **D:** wie L, aber vor dem Kanal ein SOCKS5-Handshake: nur „keine Authentifizierung“, nur `CONNECT`, Adresstypen
  IPv4, IPv6 und Domain. Alles andere bekommt die passende SOCKS-Fehlerantwort und wird geschlossen. Die Antwort
  „Erfolg“ geht erst, wenn der Kanal offen ist.
- **R:** `tcpip-forward` beim Server. Eingehende `forwarded-tcpip`-Kanäle verbindet der Client per TCP mit dem
  Ziel. Ein Kanal für eine Adresse/Port, die diese Verbindung nicht angefordert hat, wird abgelehnt.
- Start schlägt fehl, wenn Binden oder `tcpip-forward` fehlschlägt (z. B. „Adresse belegt“, Server verweigert).
  Ein Fehler beim einzelnen Kanal (Ziel nicht erreichbar) schließt nur diese eine Verbindung.
- Stop: Listener schließen bzw. `cancel-tcpip-forward`; laufende Verbindungen der Regel werden beendet.
- Neuverbindung: L und D laufen weiter. R wird auf der neuen Verbindung neu angefordert; schlägt das fehl,
  wird die Regel „Fehler“.
- Ende der Sitzung (Schließen, aufgegeben, Exit): alle Tunnel stoppen.
- Autostart-Regeln starten nach dem ersten Verbinden.
- Ereignis `TunnelEvent { rule_id, state }` mit `state: off | starting | active { port } | failed { error }`.

## 4. Tauri

- Die Terminal-Verbindung (erste und jede Neuverbindung) meldet ihre Verbindung an `Tunnels`.
- `tunnel_start(session_id, rule_id)`, `tunnel_stop(session_id, rule_id)`.
- Ereignisse als `SessionEventDto::Tunnel { event }` über den Ereigniskanal der Sitzung.
- `ConnectedSession` enthält die Regeln, wie sie beim Verbinden galten (`forwards`).

## 5. Invarianten

- Ein `forwarded-tcpip`-Kanal wird nur für eine Weiterleitung angenommen, die diese Verbindung angefordert hat.
- Ohne Adresse wird lokal nur auf Loopback gebunden. Andere Adressen nur, wenn der Nutzer sie hinschreibt.
- Der SOCKS-Proxy kann nur `CONNECT` und braucht nie Anmeldedaten.
- Tunnel fragen nie nach etwas (sie nutzen die bestehende Verbindung) und laufen nie länger als ihre Sitzung.

## 6. Tests

- Core-Unit `parse_forward`: Standards je Typ, IPv6, Ports, Fehler.
- Core-Unit `Tunnels` mit einem Test-Link, der direkt per TCP verbindet: L und D übertragen Daten in beide
  Richtungen; SOCKS lehnt Anmeldung und `BIND` ab; belegter Port ergibt „Fehler“; Stop schließt; ohne Verbindung
  wird die eingehende Verbindung geschlossen; R leitet angeforderte Kanäle weiter und lehnt fremde ab; R wird nach
  einer neuen Verbindung neu angefordert.
- Docker: L und R gegen den Testserver.
- Tauri: `tunnel_start` mit unbekannter Sitzung.
- UI: Reiter listet Regeln mit Zuständen, Schalter ruft Start/Stop, Leertext.
- Abnahme: `docs/acceptance-tp5a.md`.
