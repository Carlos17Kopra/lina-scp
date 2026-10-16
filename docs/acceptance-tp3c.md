# Abnahme TP3c – SFTP-Lesezeichen und Bearbeiten

Voraussetzung: ein SFTP-Host mit gespeichertem Passwort oder Schlüssel, ein Host aus `~/.ssh/config`, auf dem Server
eine Textdatei mit Rechten `0640`, ein Symlink auf eine Konfigurationsdatei und ein offenes Terminal auf denselben Host.

## Lesezeichen
- [ ] Lesezeichen-Knopf im Remote-Panel: „Diesen Ordner merken“, danach ist der Knopf gefüllt und der Ordner steht in der Liste.
- [ ] Klick auf ein Lesezeichen springt hin; „Lesezeichen entfernen“ und das ✕ an der Zeile entfernen es.
- [ ] `Ctrl+B` im Panel schaltet das Lesezeichen des aktuellen Ordners um.
- [ ] Pfeiltasten bewegen den Fokus im Menü, `Esc` schließt es und setzt den Fokus zurück auf den Knopf.
- [ ] Lokale Lesezeichen gelten für alle Hosts, Remote-Lesezeichen nur für ihren Host.
- [ ] Lesezeichen eines Hosts aus `~/.ssh/config` halten nach einem App-Neustart.
- [ ] Ein Lesezeichen auf einen gelöschten Ordner zeigt den normalen Fehler im Panel.

## Bearbeiten
- [ ] Doppelklick, `Enter`, `F4`, Kontextmenü und Toolbar „Bearbeiten“ öffnen eine Remote-Datei im GNOME-Texteditor.
- [ ] Der Download erscheint im Übertragungs-Panel, danach steht die Datei unter „Bearbeitet“ als „Synchron“.
- [ ] Speichern im Editor: nach etwa 2 s „Wird hochgeladen“, dann „Synchron“; `cat` im Terminal zeigt den neuen Inhalt.
- [ ] `stat -c %a datei` im Terminal: die Rechte sind nach dem Speichern unverändert (`640`).
- [ ] Symlink bearbeiten: nach dem Speichern ist der Link noch ein Link (`ls -l`), das Ziel hat den neuen Inhalt.
- [ ] `echo x >> datei` im Terminal, dann im Editor speichern: Dialog „Datei auf dem Server geändert“, Fokus auf „Nicht hochladen“.
- [ ] „Nicht hochladen“ lässt den Server unverändert; das nächste Speichern fragt erneut. „Überschreiben“ lädt hoch.
- [ ] WLAN aus, im Editor speichern, WLAN an: nach dem Reconnect wird die Datei hochgeladen.
- [ ] „Im Editor öffnen“ öffnet die Kopie erneut; „Bearbeitung beenden“ löscht die Kopie unter `~/.cache/linascp/edit/`.
- [ ] Tab schließen mit nicht hochgeladener Änderung (z. B. offline gespeichert): der Schließen-Dialog zählt sie mit, danach nennt ein Hinweis den Pfad der Kopie, die Kopie ist noch da.
- [ ] Eine Remote-Datei `test.desktop` oder ein Skript mit `x`-Rechten öffnet sich als Text und startet nie.
- [ ] Einstellungen → „Editor“ = `code -w` (oder `kate`): Bearbeiten öffnet dieses Programm.
- [ ] Ohne Texteditor-Zuordnung bzw. mit einem nicht existierenden Befehl: Hinweis statt Absturz.
- [ ] Lokale Datei bearbeiten (`F4` im lokalen Panel) öffnet die Datei direkt, ohne Kopie.
- [ ] Ohne Verbindung sind Bearbeiten-Auslöser im Remote-Panel aus.
