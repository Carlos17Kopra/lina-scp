# Abnahme TP4b – Vergleichen und Synchronisieren

Voraussetzung: ein SFTP-Host, lokal ein Projektordner mit Unterordnern, `.git` und einer `*.tmp`-Datei, auf dem Server
ein Zielordner und ein offenes Terminal auf denselben Host.

## Vergleichen
- [ ] Lokales Panel im Projektordner, Remote-Panel im (leeren) Zielordner, Toolbar „Synchronisieren“: „Vergleiche …“, dann eine Zeile je Datei bzw. je Ordner, den es nur lokal gibt.
- [ ] Während eines langen Vergleichs (großer Baum) bricht „Abbrechen“ ab und schließt den Dialog.
- [ ] „Ausschließen“ = `.git, *.tmp`, „Neu vergleichen“: `.git` und die `.tmp`-Datei fehlen in der Liste. Nach erneutem Öffnen steht das Muster noch da (je Host).
- [ ] Ohne Verbindung ist „Synchronisieren“ aus.
- [ ] Ein Ordner ohne Leserechte auf dem Server (`chmod 000`): der Vergleich meldet einen Fehler statt einer Liste.

## Hochladen
- [ ] „Synchronisieren“: die Übertragungen erscheinen im Übertragungs-Panel; danach erneut vergleichen: „Beide Ordner sind gleich.“
- [ ] Lokal eine Datei ändern, vergleichen: „Hochladen“. Remote eine Datei per `touch` neuer machen: „Übersprungen: remote neuer“.
- [ ] Remote eine zusätzliche Datei anlegen, „Auf der Zielseite löschen …“ an: Zeile „Löschen remote“ und rote Ansage „1 Eintrag wird auf dem Server endgültig gelöscht.“ Nach dem Synchronisieren ist sie weg.
- [ ] Eine Zeile abwählen: sie wird nicht übertragen, die Zusammenfassung zählt sie nicht mit.
- [ ] Vergleichen, dann im Terminal eine Zieldatei ändern, dann synchronisieren: statt Überschreiben öffnet sich der Konfliktdialog. Eine zum Löschen markierte, inzwischen geänderte Datei bleibt stehen, ein Hinweis nennt sie.
- [ ] Lokal ein Symlink auf einen Ordner, remote ein gleichnamiger Ordner: der Name taucht in der Liste nicht auf.

## Herunterladen und beide Richtungen
- [ ] „Herunterladen“ mit „Löschen“: Zeilen „Löschen lokal“; nach dem Synchronisieren liegen die Dateien im Papierkorb.
- [ ] „Beide Richtungen“: neuere Seite gewinnt, Dateien nur auf einer Seite werden kopiert, nichts wird gelöscht (Kästchen „Löschen“ ist aus).
