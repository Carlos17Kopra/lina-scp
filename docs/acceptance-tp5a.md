# Abnahme TP5a – Tunnel

Voraussetzung: ein SSH-Host mit drei Regeln im Host-Editor (Verbindung → Port-Forwarding):
- L `8080` → `localhost:80` mit Auto-Start (auf dem Server läuft ein Webserver auf Port 80),
- D `1080` ohne Auto-Start,
- R `0` → `127.0.0.1:3000` ohne Auto-Start (lokal läuft z. B. `python3 -m http.server 3000`).

## Reiter „Tunnel“
- [ ] Terminal öffnen, Seitenleiste → „Tunnel“: drei Regeln mit Badge L/D/R und `127.0.0.1:8080 → localhost:80`,
      `127.0.0.1:1080 · SOCKS`, `localhost:0 → 127.0.0.1:3000`.
- [ ] L steht ohne Zutun auf „Aktiv“; `curl http://127.0.0.1:8080` liefert die Seite des Servers.
- [ ] D „Starten“ → „Aktiv“; `curl --socks5-hostname 127.0.0.1:1080 http://localhost` liefert dieselbe Seite.
- [ ] R „Starten“ → „Aktiv · Port N“; im Terminal `curl http://localhost:N` liefert die lokale Seite.
- [ ] „Stoppen“ je Regel: Zustand „Aus“, `curl` schlägt fehl.
- [ ] Ein Host ohne Regeln und ein lokales Terminal: „Für diesen Host sind keine Regeln angelegt.“; beim Host führt
      „Host bearbeiten“ in den Editor.

## Fehler
- [ ] Port belegt (`python3 -m http.server 1080` vorher starten), D „Starten“: rot „Fehler: …“; nach dem Beenden des
      Servers startet die Regel.
- [ ] Eine Regel `8080` → `nirgendwo` lässt sich im Editor nicht speichern (Ziel braucht `host:port`).
- [ ] `ss -ltn`: ohne Adresse lauschen L und D nur auf `127.0.0.1`.

## Neuverbindung und Ende
- [ ] Netzwerk kurz trennen: während der Neuverbindung sind die Schalter aus. Danach laufen L, D und R weiter
      (R mit neuem Port bei `0`).
- [ ] Tab schließen: `ss -ltn` zeigt die Ports 8080 und 1080 nicht mehr.
