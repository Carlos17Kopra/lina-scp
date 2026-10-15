# TP2d acceptance: Auto-reconnect

No DB migration in TP2d (new settings key `terminal_auto_reconnect`).

## Reconnect
- [ ] SSH tab running `top`; disable Wi-Fi / pull the cable until the keepalive trips (≤ 90 s with defaults): the pane shows "Verbindung unterbrochen · Versuch 1 von 8 in 1 s", the terminal stays visible and scrollable, typing does nothing.
- [ ] Network back: "— Verbindung wiederhergestellt —" appears under the old output, the new prompt follows; tab dot back to green; sidebar "Sitzung" shows the new connection time.
- [ ] Laptop standby for several minutes, wake up: the session reconnects (or shows the bar, then reconnects).
- [ ] On the server, kill the session's sshd process (`sudo pkill -f "sshd-session: <user>"`, older OpenSSH `"sshd: <user>"`; `systemctl restart ssh` alone keeps existing sessions): reconnect within seconds.
- [ ] Host with `keepalive_s = 0`: kill the network, type a key: the bar appears.
- [ ] "Jetzt versuchen" skips the countdown; "Abbrechen" shows "Verbindung verloren" with "Erneut versuchen" (history kept), "Neu verbinden" (fresh pane) and "Schließen".
- [ ] Server stays down: after 8 attempts (~2 min) "Verbindung verloren" with the network error.

## No reconnect
- [ ] `exit` in the shell: "Sitzung beendet", no reconnect.
- [ ] Settings → Terminal → "Abgebrochene SSH-Verbindungen automatisch wiederherstellen" off: a dropped connection shows "Verbindung verloren" at once.
- [ ] Password host without a saved password: drop the connection: "Verbindung verloren" with "Für die Verbindung ist eine Eingabe nötig …"; only "Neu verbinden" / "Schließen"; "Neu verbinden" opens the normal prompt.
- [ ] Change the server's host key, drop the connection: input required, never accepted silently.
- [ ] Delete the host while it is reconnecting: "Verbindung verloren", only "Schließen".

## Split panes and broadcast
- [ ] Two panes, broadcast on, one pane reconnecting: typing reaches only the connected pane.
- [ ] Close a pane while it reconnects: no error, no later event.
