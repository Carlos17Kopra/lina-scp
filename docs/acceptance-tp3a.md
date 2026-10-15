# Abnahme TP3a: SFTP – Verbinden, Navigieren, Dateioperationen

## Verbinden
- [ ] „Dateien“ in Host-Karte und Detail-Panel öffnet den SFTP-Ladebildschirm (amber), Schritte bis „Verzeichnis laden“.
- [ ] Host ohne SFTP: „Dateien“ ist deaktiviert.
- [ ] Unbekannter Host-Key: Dialog wie bei SSH; Ablehnen bricht ab.
- [ ] `remoteStart` gesetzt → Remote-Panel startet dort; nicht lesbar → Fehlerseite bei „Verzeichnis laden“.
- [ ] `localStart` gesetzt → lokales Panel startet dort, sonst im Home.
- [ ] Jump-Host-Host funktioniert.
- [ ] SFTP-View ohne Sitzung zeigt die Host-Auswahl (nur SFTP-Hosts).

## Navigieren
- [ ] Doppelklick/Enter öffnet Ordner und Links auf Ordner; „..“ und Backspace gehen hoch.
- [ ] Breadcrumbs klickbar; Klick auf freie Fläche oder Strg+L → Pfad-Feld; Enter navigiert, Esc bricht ab.
- [ ] Ordner ohne Berechtigung: Fehlerzeile, alter Pfad bleibt.
- [ ] Sortierung per Spaltenkopf, Ordner immer oben, `file2` vor `file10`.
- [ ] Strg+H / „Versteckte“ blendet Punktdateien ein und aus.
- [ ] Größe, Datum (TT.MM.JJJJ hh:mm), Rechte (`rwxr-xr-x`), Besitzer stimmen mit `ls -l` überein.
- [ ] Kaputter Symlink hat Warnsymbol.

## Tastatur
- [ ] Tab wechselt das Panel; ↑/↓, Pos1/Ende, Bild↑/↓; Shift erweitert; Leertaste/Strg+Klick toggelt; Strg+A.
- [ ] Tippen springt zum Namen; F2, F7, Entf/F8, Shift+Entf (lokal), Strg+R.
- [ ] Tasten im Pfad-Feld und in Dialogen lösen keine Panel-Aktionen aus.
- [ ] Rechtsklick-Menü: Öffnen, Umbenennen, Löschen, Neuer Ordner, Neu laden.

## Dateioperationen
- [ ] Neuer Ordner lokal und remote; existierender Name → Fehler am Feld.
- [ ] Umbenennen lokal und remote; Ziel existiert → Fehler am Feld, nichts überschrieben.
- [ ] Remote löschen (Datei, Ordner mit Inhalt, Symlink auf Ordner: Ziel bleibt erhalten) mit rotem Dialog.
- [ ] Lokal Entf → Papierkorb (in Nautilus sichtbar und wiederherstellbar); Shift+Entf → endgültig.
- [ ] Lokal auf USB-Stick Entf → verständliche Meldung, Shift+Entf funktioniert.

## Verbindungsverlust
- [ ] WLAN aus / sshd neu starten → Banner „Verbindung verloren“, Remote-Panel ausgegraut, lokal bedienbar.
- [ ] „Neu verbinden“ → derselbe Tab, letzter Remote-Pfad wieder geladen.
- [ ] Tab schließen trennt die Verbindung (auf dem Server keine sftp-server-Prozesse mehr).
- [ ] „Terminal“ in der Toolbar öffnet eine SSH-Sitzung zum selben Host.
