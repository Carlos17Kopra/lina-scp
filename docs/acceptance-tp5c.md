# Abnahme TP5c – Einstellungen

## Screen
- [ ] Zahnrad in der Rail und `Ctrl+,` öffnen den Screen „Einstellungen“ (kein Dialog); das Zahnrad ist markiert.
- [ ] Design, ssh-Modus (mit Bestätigung), Renderer, Schriftgröße, Farbschema, Kompatibilität, Auto-Reconnect,
      Editor, Export und Import funktionieren wie vorher.

## Terminal
- [ ] Scrollback auf 1 000: in einem offenen Terminal `seq 5000`, nach oben scrollen: nur noch etwa 1 000 Zeilen.
- [ ] Cursor „Balken“ und „Cursor blinkt“ aus: das offene Terminal zeigt sofort einen ruhigen Balken.
- [ ] Nach einem Neustart sind die Werte noch da.

## Lokales Terminal
- [ ] Shell `/bin/sh`: ein neues lokales Terminal startet `sh` (`echo $0`), ein schon offenes bleibt.
- [ ] Shell `/gibt/es/nicht`: „Speichern fehlgeschlagen“ mit Grund; der alte Wert bleibt.
- [ ] Feld leeren: neue lokale Terminals nehmen wieder `$SHELL`.

## Neue Hosts
- [ ] Benutzer `deploy`, Port `2222`, Keepalive `0`: „Neuer Host“ zeigt diese Werte; bestehende Hosts bleiben.
- [ ] Port `abc` oder `70000`: das Feld springt auf den alten Wert zurück, nichts wird gespeichert.

## Export
- [ ] Export und Import in ein leeres Profil: Scrollback, Cursor und Standardwerte kommen mit, die Shell nicht.
