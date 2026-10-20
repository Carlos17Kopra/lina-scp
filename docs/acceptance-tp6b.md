# Abnahme TP6b – Port-Forwarding-Screen

Voraussetzung: derselbe Host wie in `docs/acceptance-tp5a.md` (L `8080` → `localhost:80` mit Auto-Start, D `1080`,
R `0` → `127.0.0.1:3000`), dazu ein zweiter Host mit einer Regel und ein Host ohne Regeln. Für den ersten Host ist
kein Passwort gespeichert (Abfrage beim Verbinden).

## Liste
- [ ] Rail „Port-Forwarding“ ist aktiv (nicht ausgegraut) und öffnet den Screen.
- [ ] Nur die beiden Hosts mit Regeln erscheinen, nach Name sortiert; Hosts aus `~/.ssh/config` nie.
- [ ] Jede Karte: Name, `user@adresse:port`, „Getrennt“, „Verbinden“, die Regeln mit ausgegrauten Schaltern,
      „Host bearbeiten“ öffnet den Editor.
- [ ] Ohne Hosts mit Regeln: „Noch keine Weiterleitungen. Regeln legst du im Host-Editor unter „Verbindung“ an.“
- [ ] Der Hinweis im Host-Editor nennt den Reiter „Tunnel“ und den Screen „Port-Forwarding“.

## Verbinden
- [ ] „Verbinden“: der Verbindungsdialog zeigt TCP, Host-Key, Anmeldung (kein Kanal, keine Shell) und fragt nach
      dem Passwort. Während er läuft, steht die Karte auf „Verbindet …“.
- [ ] Danach: Dialog zu, der Screen bleibt, Karte „Verbunden“, „Trennen“; L steht ohne Zutun auf „Aktiv“,
      `curl http://127.0.0.1:8080` liefert die Seite des Servers. Kein Terminal-Tab ist entstanden.
- [ ] D und R „Starten“ wie in TP5a; `curl --socks5-hostname 127.0.0.1:1080 http://localhost` und auf dem Server
      `curl http://localhost:N` funktionieren.
- [ ] „Abbrechen“ im Dialog: zurück auf dem Screen, Karte „Getrennt“.

## Halten und Trennen
- [ ] Netzwerk kurz trennen: „Verbindet neu (Versuch n von 3) …“, Schalter aus; danach „Verbunden“, L und D laufen
      weiter, R ist neu angefordert.
- [ ] Netzwerk länger trennen: „Verbindung verloren: …“ in Rot, alle Regeln „Aus“, `ss -ltn` zeigt 8080 und 1080
      nicht mehr; „Verbinden“ baut neu auf, Auto-Start läuft wieder.
- [ ] „Trennen“: Karte „Getrennt“, `ss -ltn` zeigt 8080 und 1080 nicht mehr.
- [ ] Alle Regeln eines verbundenen Hosts im Editor löschen: die Karte verschwindet, die Ports sind zu.

## Zusammen mit dem Terminal
- [ ] Host im Screen verbunden (L aktiv), dann ein Terminal-Tab desselben Hosts: dort wird L zu
      „Fehler: … Adresse belegt“; der Screen läuft unverändert weiter.
