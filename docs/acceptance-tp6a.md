# TP6a acceptance (cleanup)

Manual checks for `docs/superpowers/specs/2026-10-17-linascp-tp6a-cleanup-design.md`.

- [ ] Host with a stored, now wrong password: connect, type the right one without „Speichern“. The next connect
      asks again instead of trying the old password.
- [ ] Settings: change theme and font size quickly one after another; after a restart both are kept.
- [ ] Settings → Lokales Terminal: set a shell, delete the file, open a local terminal. The notice names the shell
      from the settings.
- [ ] SFTP: a server file with a non-UTF-8 name shows as lossy (as local ones do) and is left out of operations.
- [ ] Tunnel tab: click „Starten“ twice quickly on a remote rule with a fixed port. It becomes active, no error.
- [ ] Keep up to date: delete a target folder on the server, change a file in it locally twice. The second change
      creates the folder again.
- [ ] Schlüsselbund: a key used by a host with `~/.ssh/…` shows „Verwendet von“; copying without a clipboard shows
      a red notice.
- [ ] SFTP „Neuer Ordner“: Enter twice quickly creates one folder; the first-run dialog with Escape twice saves
      once.
- [ ] Release: re-running the workflow for a tag leaves one draft; `dpkg-deb -f *.deb Depends` lists webkit and gtk
      once.
