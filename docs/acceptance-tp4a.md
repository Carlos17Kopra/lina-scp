# Abnahme TP4a – Rechte, Bandbreite, SFTP-Server-Befehl

Voraussetzung: ein SFTP-Host, auf dem der Benutzer `sudo -n /usr/lib/openssh/sftp-server` ohne Passwort darf
(z. B. `user ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server` in `/etc/sudoers.d/`), eine große Datei (≥ 20 MB)
und ein offenes Terminal auf denselben Host.

## Rechte
- [ ] Kontextmenü „Eigenschaften …“ und `F9` öffnen den Dialog, im lokalen und im Remote-Panel.
- [ ] Eine Datei: Name, Art, Größe, Besitzer und Datum stehen da; Raster und Oktalfeld zeigen die Rechte aus der Spalte „Rechte“.
- [ ] „Übernehmen“ ist aus, bis sich etwas ändert. Ein Kästchen ändert das Oktalfeld, `0755` im Oktalfeld setzt die Kästchen.
- [ ] Nach „Übernehmen“ zeigt die Spalte „Rechte“ den neuen Wert, `stat -c %a` im Terminal ebenso.
- [ ] Zwei Dateien mit `644` und `600` markieren: „Gruppe: Lesen“ ist gemischt, das Oktalfeld leer. Nur „Andere: Ausführen“ anklicken und übernehmen: die erste Datei hat `645`, die zweite `601`.
- [ ] Ordner mit Unterordnern und Dateien, `0644` und „Auch für enthaltene Dateien und Ordner“: Ordner `755`, Dateien `644`.
- [ ] Ein Symlink im Ordner auf eine Datei außerhalb: deren Rechte bleiben beim rekursiven Ändern gleich.
- [ ] `/etc/passwd` (fremde Datei) zusammen mit einer eigenen Datei ändern: Meldung „1 Eintrag konnte nicht geändert werden …“ mit dem Pfad, die eigene Datei ist geändert, der Dialog bleibt offen.

## Bandbreitenlimit
- [ ] Host-Tab „SFTP“: kein Feld „Übertragungsmodus“ mehr; der Hinweis sagt, dass die Einstellungen für neue SFTP-Tabs gelten.
- [ ] Bandbreitenlimit `1024` KB/s, neuer SFTP-Tab, große Datei hochladen: etwa 1 MB/s im Übertragungs-Panel.
- [ ] Zwei Dateien parallel: zusammen etwa 1 MB/s, nicht je 1 MB/s.
- [ ] Limit leeren, neuer Tab: wieder volle Geschwindigkeit.

## SFTP-Server-Befehl
- [ ] „SFTP-Server“ = `sudo -n /usr/lib/openssh/sftp-server`, verbinden: `/root` lässt sich öffnen, eine neue Datei dort gehört `root`.
- [ ] „SFTP-Server“ = `sudo -n /usr/bin/env true` (nicht erlaubt): der Schritt „Subsystem“ schlägt fehl, die Meldung nennt den SFTP-Server-Befehl.
- [ ] Ein Zeilenumbruch im Feld lässt sich nicht speichern.
- [ ] Mit Befehl: Verbindung trennen (WLAN aus/an), der Reconnect arbeitet weiter als root.
