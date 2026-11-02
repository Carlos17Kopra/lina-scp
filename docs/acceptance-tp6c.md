# Abnahme TP6c – Identitäten des SSH-Agenten, Standardschlüssel

Voraussetzung: ein laufender `ssh-agent` (GNOME: gcr-ssh-agent) mit mindestens einem geladenen Schlüssel aus
`~/.ssh`, ein Host mit Anmeldung „SSH-Agent“, auf dem dieser Schlüssel eingetragen ist.

## Schlüsselbund
- [ ] Unter den Schlüsseldateien steht der Abschnitt „SSH-Agent“ mit einer Karte je Identität: Kommentar (oder
      Fingerprint), Typ, Fingerprint.
- [ ] Die Datei des geladenen Schlüssels zeigt „Im SSH-Agent geladen“, andere nicht.
- [ ] „Öffentlichen Schlüssel kopieren“ legt die OpenSSH-Zeile in die Zwischenablage.
- [ ] „Auf Host eintragen …“ öffnet den bekannten Dialog und trägt die Identität ein.
- [ ] `ssh-add -D` im Terminal, dann „Neu laden“: „Der SSH-Agent hat keine Schlüssel geladen.“
- [ ] App mit `env -u SSH_AUTH_SOCK bun run tauri dev` starten: „Kein SSH-Agent gefunden (SSH_AUTH_SOCK ist nicht
      gesetzt).“
- [ ] Mit `SSH_AUTH_SOCK=/tmp/gibtsnicht` gestartet: roter Fehler mit Grund.

## Standardschlüssel nach dem Agenten
- [ ] Agent leer (`ssh-add -D`), `~/.ssh/id_ed25519` ohne Passphrase ist auf dem Host eingetragen: Verbinden klappt,
      das Log zeigt „Offering public key: …/id_ed25519“.
- [ ] Ohne `SSH_AUTH_SOCK` gestartet: dasselbe klappt.
- [ ] Ist der Standardschlüssel verschlüsselt und nicht im Agenten: keine Passphrase-Abfrage, das Log zeigt
      „Skipping encrypted key …“, die Anmeldung schlägt fehl.
- [ ] Ist der Schlüssel im Agenten und auf dem Host nicht eingetragen, wird er nicht zweimal angeboten
      („Skipping …: already offered by the agent“).
- [ ] Host-Editor, Methode „SSH-Agent“: der Hinweis nennt `~/.ssh/id_*` ohne Passphrase.
