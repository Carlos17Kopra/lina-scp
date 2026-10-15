# TP2c acceptance: Split panes and broadcast

No DB migration in TP2c.

## Split
- [ ] SSH tab → "Nebeneinander teilen" (Ctrl+Shift+E): a new pane opens on the right with the picker; "Gleicher Host (…)" is preselected; Enter connects (overlay as usual) and the session appears in that pane.
- [ ] The left terminal keeps its scrollback and running program while splitting (e.g. `top` keeps running).
- [ ] "Untereinander teilen" (Ctrl+Shift+O) on the new pane nests a split below it; a 4th pane is possible, then both split buttons are disabled.
- [ ] Picker: search by name/address, arrow keys + Enter, "Lokales Terminal" opens a local shell in the pane, Esc closes the pane.
- [ ] Cancel the connect overlay of a pane: back in the terminal view, the pane shows the picker again.
- [ ] Drag a divider; also focus it (Tab) and use the arrow keys. Terminals refit.
- [ ] Click into a pane / Ctrl+Shift+←/→: the focused pane gets the accent frame and the keyboard.
- [ ] Tab title shows "web-prod-01 · 3"; the sidebar (Sitzung, Design, Snippets) follows the focused pane; a picking pane shows "Kein Terminal".
- [ ] `exit` in one pane: "Sitzung beendet" in that pane only; "Neu verbinden" reconnects in the same pane.
- [ ] × on a pane closes it, the neighbour takes its place; × on the tab / Ctrl+Shift+W closes all its sessions.

## Broadcast
- [ ] Broadcast is disabled with one pane; with two panes Ctrl+Shift+A or the button turns it on: orange frames, "Broadcast an · 2 Bereiche".
- [ ] Typing `hostname` + Enter in the focused pane runs in all connected panes; a picking or ended pane receives nothing.
- [ ] Snippets (insert and ▶) and Ctrl+Shift+V reach all panes while broadcast is on.
- [ ] Closing panes down to one switches broadcast off.
