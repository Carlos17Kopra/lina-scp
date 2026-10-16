# Abnahme TP3b – SFTP-Übertragungen

Voraussetzung: ein SFTP-Host mit gespeichertem Passwort oder Schlüssel, eine Datei > 500 MB lokal, ein Ordnerbaum
mit Unterordnern, einem Symlink auf einen Ordner und einem kaputten Symlink.

## Auslösen
- [ ] F5 im lokalen Panel lädt die Auswahl in den Remote-Ordner hoch; F5 im Remote-Panel lädt herunter.
- [ ] Kontextmenü „Hochladen“ / „Herunterladen“ und der Toolbar-Knopf tun dasselbe.
- [ ] Einträge per Maus ins andere Panel ziehen: Drop auf einen Ordner überträgt in diesen Ordner, sonst in den aktuellen.
- [ ] Ein kurzer Klick startet nichts.
- [ ] Dateien aus Nautilus auf das Remote-Panel ziehen: Rahmen erscheint, Upload startet. Über dem lokalen Panel passiert nichts.

## Warteschlange
- [ ] Fortschritt, Geschwindigkeit und Status laufen; höchstens „Parallele Übertragungen“ Dateien gleichzeitig.
- [ ] Während des Uploads liegt remote nur `name.filepart`, danach nur `name`.
- [ ] Abbrechen einer Zeile entfernt die `.filepart`; fertige Dateien bleiben.
- [ ] Symlink auf Ordner und kaputter Link stehen unter „Abgeschlossen“ als übersprungen.
- [ ] Eine nicht lesbare Datei steht unter „Fehler“ mit „Keine Berechtigung“; „Wiederholen“ nach `chmod` klappt.
- [ ] „Leeren“ räumt „Abgeschlossen“ auf. Einklappen zeigt die Zusammenfassung. Ohne Jobs ist das Panel weg.
- [ ] Nach dem Ende lädt das Ziel-Panel neu.

## Konflikte
- [ ] Erneutes Hochladen derselben Datei zeigt „Datei existiert bereits“ mit Größe und Datum beider Seiten.
- [ ] Überspringen, Nur wenn neuer, Überschreiben tun das Erwartete; Esc bricht den ganzen Auftrag ab.
- [ ] „Für alle weiteren Konflikte übernehmen“ beantwortet die restlichen Konflikte des Auftrags ohne Dialog.
- [ ] Ein Konflikt in einem Hintergrund-Tab zeigt einen Punkt am Reiter, kein Dialog.

## Rechte
- [ ] Host mit „Rechte beim Upload“ = 0644: Dateien 0644, neue Ordner 0755.
- [ ] Ohne Einstellung: die lokalen Rechte werden übernommen.
- [ ] Download einer Datei mit 0600 bleibt 0600.

## Wiederverbinden
- [ ] Während eines großen Uploads WLAN aus: Leiste „Verbindung unterbrochen · Versuch n von 8“, Job „Unterbrochen“.
- [ ] WLAN an: die Übertragung macht weiter (nicht von vorn), die Datei ist identisch (`sha256sum`).
- [ ] Auto-Reconnect in den Einstellungen aus: sofort „Verbindung verloren“, „Neu verbinden“ setzt die Übertragung fort.
- [ ] Host ohne gespeichertes Passwort: nach dem Abbruch „Verbindung verloren“, „Neu verbinden“ fragt nach dem Passwort und setzt fort.

## Tabs
- [ ] Tab mit laufenden Übertragungen schließen fragt nach; „Schließen“ bricht ab und hinterlässt keine `.filepart`.
