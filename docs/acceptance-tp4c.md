# Abnahme TP4c – Remote-Ordner aktuell halten

Voraussetzung: ein SFTP-Host, lokal ein Projektordner, auf dem Server ein Zielordner und ein offenes Terminal auf
denselben Host.

## Starten und Beenden
- [ ] Ohne Verbindung ist „Aktuell halten“ aus.
- [ ] „Aktuell halten“: der Dialog zeigt `lokal → remote`, den Hinweis zu Löschungen und das Muster vom
      Synchronisieren. „Starten“: über dem Übertragungs-Panel steht „Hält <remote> aktuell · 0 hochgeladen“.
- [ ] Ein nicht vorhandener Remote-Ordner (vorher im Terminal löschen): der Dialog meldet „Starten fehlgeschlagen“.
- [ ] „Beenden“: die Leiste verschwindet; eine danach gespeicherte Datei wird nicht hochgeladen.
- [ ] Tab schließen beendet es ebenfalls.

## Hochladen
- [ ] Beim Start vorhandene Dateien werden nicht hochgeladen.
- [ ] Eine Datei im Editor speichern: nach etwa 4 s steht sie auf dem Server, der Zähler steigt.
- [ ] Einen neuen Ordner mit Dateien anlegen (`cp -r`): er erscheint als **ein** Auftrag im Übertragungs-Panel.
- [ ] Eine ausgeschlossene Datei (`*.tmp`) wird nicht hochgeladen.
- [ ] Eine große Datei langsam schreiben (`dd … bs=1M count=200 oflag=sync`): hochgeladen wird erst der fertige Stand.
- [ ] Im Terminal eine Serverdatei ändern, dann lokal speichern: der Konfliktdialog öffnet sich statt zu überschreiben.
- [ ] Lokal eine Datei löschen: sie bleibt auf dem Server.

## Fehler
- [ ] Den lokalen Ordner umbenennen: die Leiste zeigt rot „Fehler: …“. Zurück umbenennen: der
      Fehler verschwindet, Änderungen werden wieder hochgeladen.
- [ ] Verbindung trennen (z. B. Netzwerk aus), lokal speichern, Verbindung zurück: nach dem Reconnect wird
      hochgeladen.
