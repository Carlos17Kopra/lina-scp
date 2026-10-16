# LinaSCP TP6a: Aufräumen (Backlog)

Stand: 2026-10-17. Teil 1 von TP6.

- **6a (dieses Dokument):** offene Funde aus `docs/backlog-after-tp1.md`, die Daten, Sicherheit oder sichtbares
  Verhalten betreffen.
- **6b:** Rail-Screen „Port-Forwarding“ (Tunnel ohne Terminal).
- **6c:** Identitäten des SSH-Agenten anzeigen.

Der Nutzer hat die Fragerunde übersprungen. Auswahl und Lösungen sind Defaults des Autors. Jeder Punkt bekommt
einen Test, der vorher rot ist; erledigte Punkte verschwinden aus dem Backlog.

## Punkte

1. **Veraltetes Passwort:** Wird ein gespeichertes Passwort abgelehnt und meldet sich der Nutzer danach mit einem
   neu getippten an (ohne „Speichern“), wird das alte aus dem Schlüsselbund gelöscht (fail2ban).
2. **Einstellungen speichern nacheinander:** Saves im UI laufen in einer Kette; jeder baut auf dem aktuellen Stand
   auf, ein Fehler setzt nur das eigene Feld zurück.
3. **Einstellungen in einer Transaktion:** `save_settings` schreibt alle Schlüssel in einer SQLite-Transaktion.
5. **chmod ohne bekannte Rechte:** Liefert der Server für einen Eintrag keine Rechte, wird eine relative Änderung
   („gemischt“ behalten) für ihn nicht angewendet, sondern als Fehler mit Pfad gemeldet. Eine vollständige Angabe
   gilt weiter.
6. **„Jetzt versuchen“:** Ein Klick während eines Versuchs überspringt die nächste Wartezeit nicht (Test als
   Absicherung; während eines Versuchs ist die Phase `Attempting`, der Fund betraf nur ein Fenster von
   Mikrosekunden).
7. **Doppelte Bestätigung:** `NameDialog` (doppeltes Enter) und `FirstRunDialog` (doppeltes Escape) lösen nur
   einmal aus.
8. **Nicht-UTF-8-Namen:** Entfernte Namen mit Ersatzzeichen werden als `nameLossy` markiert (wie lokal).
9. **Kopieren:** „Öffentlicher Schlüssel kopiert“ erscheint nur bei Erfolg, sonst ein roter Hinweis.
10. **„Verwendet von“:** `~` wird gegen das Home-Verzeichnis des Schlüssels aufgelöst (gelistete Schlüssel liegen
    in `~/.ssh`), nicht mehr per Endung verglichen.
11. **Gelöschte Shell:** Der Fehler beim Öffnen eines lokalen Terminals nennt die Shell aus den Einstellungen.
12. **Doppeltes „Starten“:** Läuft für eine Remote-Regel schon eine Anfrage, sendet ein zweites „Starten“ keine
    zweite.
13. **WebGL:** Scheitert `loadAddon`, wird das Addon freigegeben.
14. **Release:** Ein erneuter Lauf für denselben Tag ersetzt den Entwurf statt einen zweiten anzulegen; das .deb
    listet webkit/gtk nicht doppelt.
15. **Keep, gelöschter Ordner:** Löscht jemand einen Zielordner auf dem Server, legt die nächste Änderung ihn wieder
    an.

Nicht enthalten (bleibt im Backlog): Mirror-Aliase aus Importen, Zeroize von Antworten, `list_hosts` N+1, reine
Test- und a11y-Lücken, `has_secret` nach einem Speichern aus einer Abfrage (nur der Platzhalter „gespeichert“),
A+/A− im Host-Bereich, blockierendes I/O in `settings_set`/`sync_mirror` (der ganze Store arbeitet synchron im
Runtime; das Schreiben des Mirrors dauert Millisekunden).
