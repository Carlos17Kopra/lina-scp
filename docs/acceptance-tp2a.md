# Manual acceptance: TP2a (terminal sidebar, color schemes, session info)

Run `export PATH="$HOME/.cargo/bin:$PATH"; bun run tauri dev`.

- [ ] Ctrl+Shift+B and the tab-bar button open/close the sidebar; the state survives an app restart.
- [ ] Toggling the sidebar refits the terminal (`tput cols` changes) without garbled output.
- [ ] Sitzung (SSH): host, jump host, login method(s) + profile, host key + copy, kex, cipher/MAC, size, keepalive,
      "Verbunden seit" ticks every minute; after `exit` it shows "getrennt".
- [ ] Sitzung (local): Lokale Shell, size, Geöffnet seit.
- [ ] Design: every scheme changes the open terminal immediately; A−/A+ stop at 10/20 px.
- [ ] "Nur diesen Host" on host A changes only A's tabs; a tab of host B keeps the global scheme; back to
      "Alle Sitzungen" restores the global look for A.
- [ ] Editing host A in the editor keeps its terminal override.
- [ ] Settings dialog: Farbschema select changes all tabs without an override.
- [ ] Export with an override, import into a fresh profile: the override is kept.
