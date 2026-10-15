# LinaSCP TP2b: Snippets

Stand: 2026-10-15. Teil 2 von TP2 („SSH-Terminal komplett“). Reihenfolge:
2a Seitenleiste + Design + Sitzung (fertig) → **2b Snippets** → 2c Split-Panes + Broadcast → 2d Auto-Reconnect.

Vorlage: `shellport-design.zip`, `screens/SSH.dc.html` (Reiter „Snippets“: Suchfeld, Karten mit Name, Scope und
Befehl, gestrichelter Button „+ Snippet anlegen“) und das Datenmodell im README
(`Snippet: name, scope, command`).

## Ziel

- Gespeicherte Befehle (Snippets) lassen sich im Terminal mit einem Klick einfügen oder mit ▶ ausführen.
- Ein Snippet gilt global oder für genau einen Host. Es darf mehrzeilig sein und Platzhalter `{{name}}` enthalten,
  die vor dem Einfügen abgefragt werden.
- Snippets werden in der Terminal-Seitenleiste und in einem eigenen Screen (Rail-Eintrag „Snippets“) verwaltet.

Nicht enthalten:
- Gruppen- oder Host-Muster-Scope (`web-*`), Standardwerte für Platzhalter, Shell-Quoting der Werte.
- Snippets an mehrere Tabs gleichzeitig senden (kommt mit Broadcast in 2c).
- Tastenkürzel pro Snippet, Sortierung per Drag & Drop, Ordner/Tags.

## 1. Daten (Core)

### Modell

```rust
pub struct Snippet {
    pub id: Id,
    pub name: String,
    pub command: String,
    pub host_id: Option<Id>, // None = global
    pub created_at: i64,
    pub updated_at: i64,
}
```

Serialisiert in camelCase (`hostId`, `createdAt`, `updatedAt`), specta-Typ für die UI.

### Migration V3

```sql
CREATE TABLE snippets (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    command     TEXT NOT NULL,
    host_id     TEXT REFERENCES hosts(id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
```

Wird ein Host gelöscht, verschwinden seine Snippets. Globale Snippets bleiben unberührt.

### Validierung (`validate.rs`)

`validate_snippet(&Snippet) -> Result<()>`:
- Name: nach `trim` nicht leer, höchstens 64 Zeichen, kein `\n`, `\r` oder anderes Steuerzeichen.
- Befehl: nicht leer (nach `trim`), höchstens 8192 Bytes, kein NUL-Byte. Zeilenumbrüche und Tabs sind erlaubt.
- Fehler sind `CoreError::validation` mit dem Feldnamen (`name` | `command`), damit die UI sie am Feld zeigt.

### Store

- `list_snippets() -> Vec<Snippet>`, sortiert nach Name (case-insensitive).
- `save_snippet(&Snippet)`: Upsert. Validiert vorher. `created_at` bleibt bei Updates erhalten, `updated_at` wird
  gesetzt. Ein `host_id`, der nicht existiert, ist ein Validierungsfehler (Feld `hostId`).
- `delete_snippet(id)`. Eine unbekannte ID ist kein Fehler.

### Tauri-Commands

`snippets_list`, `snippet_save(snippet) -> Snippet`, `snippet_delete(id)`. Wie bei `host_save` erzeugt die UI
für neue Snippets die ID (`newId()` in `ipc/api.ts`), der Core macht den Upsert und gibt das gespeicherte Snippet
(mit gesetzten Zeitstempeln) zurück.

### Export und Import

- `ExportDoc` bekommt `snippets: Vec<Snippet>` mit `#[serde(default)]`. Ältere `.linascp`-Dateien bleiben
  importierbar; das Format-Version-Feld bleibt unverändert.
- Beim Import laufen Snippets durch denselben Konfliktplan wie Gruppen, Profile und Hosts (eigene `ItemKind`
  `Snippet`): bei gleicher ID gelten die Auflösungen Überspringen / Ersetzen / Duplizieren. Die Import-Vorschau
  listet Snippet-Konflikte mit dem Label „Snippet“.
- `host_id` wird über den Plan auf die neue Host-ID abgebildet (z. B. bei „Duplizieren“). Wird der Host nicht
  geschrieben (abgelehnt, übersprungen und lokal nicht vorhanden, oder gar nicht in der Datei und lokal nicht
  vorhanden), wird das Snippet **global** statt verworfen.
- Ungültige Snippets werden nicht geschrieben und zählen unter `invalid`.
- Snippets enthalten keine Geheimnisse im Sinne der Invarianten. Der Editor weist darauf hin, keine Passwörter in
  Befehle zu schreiben.

## 2. Einfügen, Ausführen, Platzhalter (UI)

### Paste-Registry

`ui/src/screens/terminal/pasteTargets.ts`: `registerPasteTarget(tabId, paste: (text: string) => void)`,
`dropPasteTarget(tabId)`, `pasteInto(tabId, text): boolean`. `TerminalView` registriert beim Mount
`(text) => { term.paste(text); term.focus(); }` und entfernt den Eintrag beim Unmount (Muster von `streams.ts`).

- **Einfügen**: `pasteInto(activeTabId, text)`. `term.paste` nutzt Bracketed Paste, wenn die Shell es aktiviert
  hat; mehrzeilige Snippets laufen dann nicht schon beim Einfügen.
- **Ausführen (▶)**: `pasteInto(activeTabId, text)` und danach `sessionWrite(sessionId, "\r")`.
- Ohne aktiven Tab, bei einem Tab mit Status ≠ `connected` (SSH) oder ohne registriertes Ziel sind Einfügen und ▶
  deaktiviert.

### Platzhalter (`ui/src/screens/terminal/snippets/placeholders.ts`)

- Syntax `{{name}}` mit `name` aus `[A-Za-z0-9_-]{1,32}`. Alles andere (z. B. `{{ x y }}`, `{{}}`) bleibt
  unverändert im Text.
- `placeholderNames(text): string[]` — eindeutige Namen in Reihenfolge des ersten Auftretens.
- `fillPlaceholders(text, values: Record<string, string>): string` — ersetzt jedes Vorkommen; Werte werden 1:1
  eingesetzt (kein Quoting, keine erneute Auflösung von `{{…}}` in Werten).
- Hat ein Snippet Platzhalter, öffnet sich vor Einfügen/Ausführen der Dialog „Platzhalter ausfüllen“: ein Feld pro
  Name, erstes Feld fokussiert, Enter bestätigt, Esc bricht ab. Leere Werte sind erlaubt. Beim Ausführen ist
  „Ausführen“ deaktiviert, solange ein Wert einen Zeilenumbruch enthält. Werte werden nicht gespeichert.

## 3. Oberfläche

### Seitenleiste

- Reiter: **Snippets · Sitzung · Design · Tunnel** (Tunnel bleibt deaktiviert). Standard-Reiter bleibt „Sitzung“.
- Panel „Snippets“:
  - Suchfeld „Snippets suchen“, filtert case-insensitive über Name und Befehl.
  - Liste: zuerst Snippets des Hosts des aktiven Tabs, dann globale, jeweils nach Name. Lokale Terminals zeigen nur
    globale.
  - Karte: Name, Scope-Label („Global“ oder Hostname), erste Befehlszeile (mit Ellipse abgeschnitten). Klick auf
    die Karte = Einfügen. Buttons ▶ „Ausführen“ und ✎ „Bearbeiten“ (sichtbar bei Hover und Fokus, immer per Tab
    erreichbar).
  - Leerzustand: „Noch keine Snippets“; bei Suche ohne Treffer „Keine Treffer“.
  - Gestrichelter Button „+ Snippet anlegen“: Scope vorbelegt mit dem Host des Tabs, bei lokalen Terminals „Global“.

### Editor-Dialog (geteilt)

- Felder: Name, Befehl (mehrzeiliges Textarea in Monospace), Scope (Select: „Global“ + alle Hosts nach Name).
- Hinweis unter dem Befehl: „Platzhalter: {{name}} · Keine Passwörter speichern“.
- Speichern ruft `snippet_save`; Validierungsfehler aus dem Core erscheinen am Feld. „Löschen“ (nur bei bestehenden
  Snippets) fragt vorher nach.

### Screen „Snippets“

- Der Rail-Eintrag „Snippets“ wird aktiv und öffnet den Screen.
- Kopf: Titel, Suchfeld, Filter „Alle / Global / Host“, Button „+ Snippet anlegen“ (Scope „Global“).
- Tabelle: Name, Scope, Befehl (erste Zeile). Klick oder Enter auf eine Zeile öffnet den Editor.
- Kein Einfügen oder Ausführen im Screen.

### Store

`useSnippetsStore` (zustand): `snippets`, `load()`, `save(snippet)`, `remove(id)`. Lädt beim App-Start und nach
jedem Speichern/Löschen neu. Nach dem Löschen eines Hosts lädt der Hosts-Store auch die Snippets neu (Cascade).
Strings in `ui/src/i18n/de.ts`.

## 4. Fehlerfälle

| Fall | Verhalten |
|---|---|
| Speichern mit leerem Namen / Befehl, zu lang, NUL | Core lehnt ab, Fehler am Feld, Dialog bleibt offen |
| Host wird gelöscht | Seine Snippets verschwinden (Cascade), Liste wird neu geladen |
| Tab getrennt / kein Tab | Einfügen und ▶ deaktiviert |
| Platzhalter-Dialog abgebrochen | Nichts wird gesendet |
| Import mit unbekanntem Host | Snippet wird global importiert |
| Import mit ungültigem Snippet | Nicht geschrieben, zählt unter `invalid` |

## 5. Tests

- **Rust (Core)**: Validierung (Grenzen 64 / 8192, Steuerzeichen, NUL, Zeilenumbrüche im Befehl erlaubt);
  Store-Roundtrip, Sortierung, `created_at` bleibt erhalten, unbekannter `host_id` abgelehnt, Cascade beim
  Host-Löschen; Migration V3 idempotent; Export enthält Snippets; Import alter Dateien ohne `snippets`;
  Konfliktauflösungen; Host-ID-Mapping bei Duplizieren; Fallback auf global; ungültige Snippets → `invalid`.
- **Bindings**: `cargo test -p linascp exports_bindings` regeneriert `bindings.ts`.
- **UI (Vitest + RTL)**: `placeholders.ts` (Namen, Duplikate, ungültige Syntax bleibt, Werte mit `{{x}}` werden
  nicht erneut aufgelöst); Paste-Registry; Panel (Filter, Sortierung, Einfügen, Ausführen sendet `\r`, deaktiviert
  bei getrenntem Tab, Platzhalter-Dialog, Abbruch sendet nichts); Editor (Speichern, Feldfehler, Löschen mit
  Rückfrage); Screen (Filter, Zeile öffnet Editor); Rail-Eintrag aktiv; TerminalView registriert und entfernt das
  Paste-Ziel.
- **Manuell**: `docs/acceptance-tp2b.md`.
