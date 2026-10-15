# TP2b acceptance: Snippets

Before the first start of this build: `cp ~/.local/share/linascp/linascp.db ~/.local/share/linascp/linascp.db.before-tp2b`
(migration V3; older builds refuse the migrated DB).

## Sidebar
- [ ] Terminal → open the sidebar (Ctrl+Shift+B) → tab "Snippets" is first; "Sitzung" is still selected by default.
- [ ] "+ Snippet anlegen" in an SSH tab: scope is preset to the host; save → card appears at the top.
- [ ] In a tab of a host from `~/.ssh/config` the scope is preset to "Global".
- [ ] Click on a card inserts the command at the prompt without running it; the terminal has focus.
- [ ] ▶ runs the command (Enter is sent after the text).
- [ ] Multi-line snippet with bracketed paste (bash ≥ 5.1 / zsh): click inserts all lines without running; ▶ runs them.
- [ ] Snippet with `{{unit}}` twice and `{{lines}}`: dialog shows two fields; Enter confirms; Esc cancels and sends nothing.
- [ ] Search filters by name and command; "Keine Treffer" without matches.
- [ ] Local terminal shows only global snippets.
- [ ] Disconnected tab: card and ▶ are disabled.

## Screen
- [ ] Rail "Snippets" is active and opens the screen with all snippets (name, scope, first command line).
- [ ] Filter Alle / Global / Host and search work; clicking a name opens the editor.
- [ ] Editor: empty name/command shows the field errors; a 9 KB command shows a red toast and the dialog stays open.
- [ ] Delete asks "Snippet wirklich löschen?" first.

## Data
- [ ] Delete a host that owns snippets → its snippets disappear from sidebar and screen.
- [ ] Export → import into a fresh profile: snippets come back, bound to their host.
- [ ] Import the export again: snippets show up as conflicts ("Snippet", gleiche ID).
- [ ] Imported snippets appear in sidebar and screen right after the import (no restart).
- [ ] Two equally named snippets for different hosts both import; a local snippet with that name stays untouched.
- [ ] An export from v0.1.0 (no snippets) still imports.
