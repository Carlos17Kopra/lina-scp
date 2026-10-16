# Abnahme TP5b – Schlüsselbund, Schlüssel erzeugen, ssh-copy-id

Voraussetzung: in `~/.ssh` mindestens ein Schlüsselpaar, ein Host mit Passwort-Anmeldung.

## Screen „Schlüsselbund“
- [ ] Rail „Schlüsselbund“: je Schlüssel eine Karte mit Typ, `SHA256:`-Fingerprint, Kommentar, „Mit Passphrase“ /
      „Ohne Passphrase“ und Pfad.
- [ ] Ein Schlüssel, den ein Host (Pfad mit `~/`) und ein Login-Profil nutzen: „Verwendet von: <host>, Profil <name>“.
- [ ] „Öffentlichen Schlüssel kopieren“: die Zwischenablage enthält die `.pub`-Zeile.
- [ ] Eine Datei ohne `.pub` daneben erscheint nicht.

## Neuer Schlüssel
- [ ] „Neuer Schlüssel“: Name `id_ed25519` (oder `…_linascp`, wenn belegt), Kommentar `user@rechner`.
- [ ] Ungleiche Passphrasen: rote Meldung, „Erzeugen“ ist aus.
- [ ] Erzeugen mit Passphrase: die Karte erscheint mit „Mit Passphrase“; `ls -l ~/.ssh/<name>*` zeigt `-rw-------`
      und `-rw-r--r--`; `ssh-keygen -y -f ~/.ssh/<name>` fragt nach der Passphrase.
- [ ] Typ „RSA 4096“: Name wechselt zu `id_rsa…`, `ssh-keygen -l -f ~/.ssh/<name>.pub` zeigt 4096.
- [ ] Ein vorhandener Name: „Erzeugen fehlgeschlagen: … existiert bereits“, die Datei bleibt unverändert.
- [ ] Host-Editor → Authentifizierung → Schlüssel → „Neu erzeugen“: nach dem Erzeugen steht der Pfad im Feld; mit
      Passphrase verbindet der gespeicherte Host ohne Nachfrage.

## Auf Host eintragen
- [ ] „Auf Host eintragen …“, Host wählen, „Eintragen“: Passwort-Abfrage im Dialog, danach „Schlüssel eingetragen.“
- [ ] Danach verbindet ein Host mit diesem Schlüssel ohne Passwort.
- [ ] Ein zweites Mal: „Der Schlüssel war schon eingetragen.“; `~/.ssh/authorized_keys` enthält die Zeile einmal.
- [ ] Falsches Passwort: „Eintragen fehlgeschlagen: …“.
