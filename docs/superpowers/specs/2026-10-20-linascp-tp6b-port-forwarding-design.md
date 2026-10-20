# LinaSCP TP6b: Port-Forwarding-Screen (Tunnel ohne Terminal)

Stand: 2026-10-20. Teil 2 von TP6 (Aufteilung siehe TP6a-Spec).

Der Nutzer hat die Fragerunde übersprungen („Überspring die Fragen, leg einfach los“). Alle Entscheidungen unten
sind Defaults des Autors, nicht abgestimmt.

## Ziel

- Der bisher ausgegraute Rail-Eintrag „Port-Forwarding“ öffnet einen eigenen Screen.
- Dort lassen sich die Port-Forward-Regeln eines Hosts über eine eigene SSH-Verbindung **ohne Shell** betreiben:
  verbinden, Regeln starten/stoppen, trennen. Kein Terminal-Tab nötig.
- Regeln, Validierung, Tunnel-Logik und Zustände sind die von TP5a (`crates/core/src/tunnel.rs`). Neu ist nur die
  Verbindung, die sie trägt, und der Screen.

## Nicht enthalten (später, bei Bedarf)

- Regeln ad hoc im Screen anlegen oder bearbeiten (das bleibt im Host-Editor, Reiter „Verbindung“).
- Verbindungen beim App-Start automatisch aufbauen; Weiterlaufen nach dem Schließen des Fensters (Tray).
- Eine Verbindung mit Terminal-Tabs desselben Hosts teilen. Beide sind unabhängig (siehe §5).
- Unix-Sockets.

## 1. Screen „Port-Forwarding“

- Rail: Eintrag aktiv, `View` `"portForwarding"`, Icon wie bisher (`ArrowRightLeft`).
- Inhalt: eine Karte je **gespeichertem Host mit mindestens einer Regel** (Hosts aus `~/.ssh/config` haben keine
  Regeln und erscheinen nie), sortiert wie die Host-Liste (nach Name).
- Kopf der Karte: Hostname, darunter `user@adresse:port` gedämpft, rechts der Verbindungszustand und ein Knopf:
  - „Getrennt“ → Knopf „Verbinden“.
  - „Verbindet …“ (der Verbindungsdialog läuft) → Knopf aus.
  - „Verbunden“ → Knopf „Trennen“.
  - „Verbindet neu (Versuch n von 3) …“ → Knopf „Trennen“.
  - „Verbindung verloren: <Grund>“ (rot) → Knopf „Verbinden“.
- Darunter die Regeln genau wie im Terminal-Reiter „Tunnel“ (Badge, Text, Zustand, „Starten“/„Stoppen“). Die
  Schalter sind aus, solange die Karte nicht „Verbunden“ ist. Die Zeilen werden aus `TunnelPanel` herausgelöst
  und von beiden benutzt, damit sie gleich aussehen und sich gleich verhalten.
- Ein Link „Host bearbeiten“ je Karte öffnet den Host-Editor.
- Keine Karte: Leertext „Noch keine Weiterleitungen. Regeln legst du im Host-Editor unter „Verbindung“ an.“
- Der Hinweis im Host-Editor (`forwardsHint`) nennt künftig beide Wege: Terminal-Reiter „Tunnel“ und den Screen
  „Port-Forwarding“.

## 2. Verbinden

- „Verbinden“ startet den bekannten Verbindungsablauf (`useConnectStore.start(hostId, "tunnel")`) mit Modal,
  Schritten, Host-Key-Dialogen, Passwort-/Passphrase-/2FA-Abfragen, Fehlerbildschirmen und Wiederholen.
- Schritte: (Jump-Host,) TCP, Host-Key, Anmeldung. Kein Kanal, keine Shell.
- Abbrechen oder Ablehnen kehrt zum Screen „Port-Forwarding“ zurück.
- Nach Erfolg: Karte „Verbunden“, Regeln mit „Auto-Start“ starten von selbst (wie TP5a). Die Regeln gelten so,
  wie sie beim Verbinden gespeichert waren; Änderungen im Editor wirken nach „Trennen“/„Verbinden“.
- Höchstens eine Port-Forwarding-Verbindung je Host. „Verbinden“ auf einer verbundenen Karte gibt es nicht.

## 3. Verbindung halten

- Neuverbindung wie bei SFTP (TP3b, `Reconnecting<T>`): gleiche Versuche, Pausen und Einstellung
  „Automatisch neu verbinden“; Neuverbindungen fragen nie (eine Abfrage = `inputRequired`, fail closed).
- Verlust wird wie bei SFTP erkannt (Handle geschlossen, geprüft jede Sekunde; Keepalive des Hosts schließt ein
  totes Handle).
- Während der Neuverbindung: L/D-Listener bleiben, neue eingehende Verbindungen werden geschlossen; R wird auf der
  neuen Verbindung neu angefordert (genau TP5a `detach`/`attach`).
- Aufgegeben („Verbindung verloren“): alle Tunnel stoppen (`end`). Erneutes „Verbinden“ ist ein neuer
  interaktiver Verbindungsablauf auf derselben Karte; Auto-Start läuft dann wieder.
- „Trennen“: alle Tunnel stoppen, Verbindung schließen, Karte „Getrennt“.

## 4. Backend

- **Core** `connect/tunnel_session.rs`: `TunnelSession` hält Handle, optional Jump-Handle, `SessionInfo` und den
  Verlust-Wächter (wie `SftpSession`), implementiert `Connection` und liefert `link()` (dieselbe `SshLink` wie
  `SshSession`).
- **Core** `connect_tunnel(req, deps, broker, emit, on_lost, cancel) -> Result<TunnelSession>` und
  `plan_tunnel_steps`. Emittiert `Steps` und bei Fehler `Failed`, wie `connect_sftp`.
- **Tauri** `commands/port_forwarding.rs`:
  - `connect_tunnels(connect_id, host_id, events, tunnel_events) -> TunnelsConnected { session_id, forwards }`.
  - `tunnels_close(session_id)`; unbekannte Ids sind in Ordnung.
  - Die `Tunnels` des Eintrags liegen zusätzlich in `state.tunnels`, damit `tunnel_start`/`tunnel_stop`
    unverändert funktionieren.
  - Ereignisse über einen eigenen Kanal: `PortForwardEvent` = `reconnecting { attempt, maxAttempts, retryInMs,
    reason }` | `reconnected` | `lost { error }` | `tunnel { event }`.
- Neuverbindungen setzen `deps.forwarded` auf die Routen der `Tunnels` und rufen nach Erfolg `attach` (im
  Hintergrund, wie beim Terminal); ein Verlust ruft `detach`, „aufgegeben“ ruft `end`.

## 5. Zusammenspiel mit Terminal-Tabs

- Terminal-Tabs und Port-Forwarding-Karten haben je eigene Verbindungen und eigene `Tunnels`. Startet man dieselbe
  L/D-Regel in beiden, bekommt der zweite Start „Fehler: Adresse belegt“; bei R verweigert meist der Server. Das
  ist gewollt und wird nicht abgefangen.

## 6. Invarianten

- Alle Tunnel-Invarianten aus TP5a gelten unverändert (forwarded-tcpip nur für angeforderte Weiterleitungen,
  Loopback ohne Adresse, SOCKS nur `CONNECT` ohne Anmeldung, Tunnel fragen nie).
- Tunnel laufen nie länger als ihre Verbindung: „Trennen“, „aufgegeben“ und das Ende der App stoppen sie.
- Genau ein terminales `ConnectEvent` je Connect-Id, auch bei Fehlern vor der Pipeline und bei Abbruch.

## 7. Tests

- Core-Unit: `plan_tunnel_steps` (mit und ohne Jump-Host).
- Docker (`ssh_integration`): `connect_tunnel` + L-Weiterleitung überträgt Daten; Schließen des Handles feuert
  `on_lost`.
- Tauri: `connect_tunnels` mit unbekanntem Host sendet genau ein `Failed`; `tunnels_close` mit unbekannter Id ist
  `Ok`; ein Verlust ruft `detach`, „aufgegeben“ ruft `end` (mit Test-Link wie in `connect.rs`).
- UI: Screen listet nur Hosts mit Regeln, Leertext, Zustände und Knöpfe je Verbindungszustand, Schalter nur wenn
  verbunden, „Trennen“ ruft `tunnelsClose`; Connect-Store mit Protokoll `"tunnel"` kehrt zum Screen zurück;
  Rail-Eintrag ist aktiv.
- Abnahme: `docs/acceptance-tp6b.md`.
