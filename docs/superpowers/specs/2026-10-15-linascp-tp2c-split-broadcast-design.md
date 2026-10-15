# LinaSCP TP2c: Split-Panes und Broadcast

Stand: 2026-10-15. Teil 3 von TP2 („SSH-Terminal komplett“). Reihenfolge:
2a Seitenleiste (fertig) → 2b Snippets (fertig) → **2c Split-Panes + Broadcast** → 2d Auto-Reconnect.

Vorlage: `shellport-design.zip`, `screens/SSH.dc.html`. Dort gibt es die Buttons „Vertikal teilen“ und „Horizontal
teilen“, den Umschalter „Broadcast“ mit `aria-pressed`, zwei Bereiche mit verschiedenen Hosts in einem Tab und die
Statuszeile „2 Bereiche · Broadcast an“.

## Ziel

- Ein Terminal-Tab lässt sich in bis zu 4 Bereiche teilen. Die Bereiche sind beliebig verschachtelt, nebeneinander
  oder untereinander, und die Trennlinien lassen sich verschieben.
- Jeder Bereich hat seine eigene Sitzung. Beim Teilen wählt man, was der neue Bereich öffnet: den gleichen Host, einen
  anderen Host oder ein lokales Terminal.
- Mit Broadcast geht die Eingabe des fokussierten Bereichs an alle verbundenen Bereiche desselben Tabs.

Nicht enthalten:
- Layouts speichern oder wiederherstellen.
- Fokus-Navigation nach Geometrie.
- Bereiche zwischen Tabs verschieben.
- Broadcast über mehrere Tabs.
- Broadcast nur an ausgewählte Bereiche.
- Ein Kontextmenü.

## 1. Modell (UI, `stores/sessions.ts`)

Der bestehende `Tab`-Datensatz steht für genau eine Sitzung und ist jetzt ein **Bereich**. Alles, was an seiner `id`
hängt, bleibt unverändert: Streams, Session-Events, Paste-Ziele, `TerminalView` und `SessionEnded`.
`TabStatus` bekommt den neuen Wert `"picking"`. Ein solcher Bereich hat noch keine Sitzung (`sessionId` fehlt) und
zeigt die Auswahl.

Neu ist eine Ebene darüber:

```ts
type Layout =
  | { type: "pane"; paneId: string }
  | { type: "split"; dir: "row" | "column"; ratio: number; a: Layout; b: Layout };

interface TabGroup {
  id: string;
  layout: Layout;
  focusedPaneId: string;
  broadcast: boolean;
}
```

- `dir: "row"` heißt nebeneinander, `"column"` heißt untereinander. `ratio` ist der Anteil von `a` und liegt
  zwischen 0,15 und 0,85.
- Der Store hält `tabs: Tab[]` (alle Bereiche) und `groups: TabGroup[]`. `activeId` ist die ID der aktiven Gruppe.
- `addTab(tab)` legt wie bisher eine neue Gruppe mit genau einem Bereich an und aktiviert sie. Das gilt für alle
  bisherigen Aufrufer, also Verbinden und `openLocal`.
- Reine Funktionen in `ui/src/screens/terminal/layout.ts`:
  - `paneIds(layout)`: Bereiche in Baum-Reihenfolge.
  - `splitPane(layout, paneId, dir, newPaneId)`: Der neue Bereich kommt rechts bzw. unten dazu, `ratio` ist 0,5.
  - `removePane(layout, paneId)`: Der Geschwister-Teilbaum rückt an die Stelle des Splits. Für den letzten Bereich
    gibt die Funktion `null` zurück.
  - `setRatio(layout, path, ratio)`: Der Wert wird auf 0,15–0,85 begrenzt.
  - `neighbour(layout, paneId, ±1)`: der nächste bzw. vorige Bereich, zyklisch.
  - `MAX_PANES = 4`.
- Store-Aktionen:
  - `split(dir)` teilt den fokussierten Bereich der aktiven Gruppe. Es legt einen `"picking"`-Bereich an und
    fokussiert ihn. Bei `MAX_PANES` passiert nichts.
  - `focusPane(paneId)`.
  - `focusNeighbour(±1)`.
  - `setRatio(groupId, path, ratio)`.
  - `closePane(paneId)`: schließt die Sitzung (best effort) und entfernt den Bereich. War es der letzte, schließt der
    Tab.
  - `closeGroup(groupId)`: schließt alle Sitzungen der Gruppe.
  - `toggleBroadcast()`.
  - `attachSession(paneId, patch)`: macht aus einem `"picking"`-Bereich eine Sitzung.
- Bleibt nur ein Bereich übrig, wird `broadcast` auf `false` gesetzt.
- Wird der fokussierte Bereich entfernt, geht der Fokus auf `neighbour(…, -1)`. Gibt es keinen, geht er auf den
  ersten Bereich.
- Layouts werden nicht gespeichert.

## 2. Oberfläche

### Tab-Leiste

- Ein Eintrag pro Gruppe:
  - Titel und Statuspunkt kommen vom fokussierten Bereich.
  - Bei mehreren Bereichen kommt ein Zähler dazu, zum Beispiel „web-prod-01 · 2“.
  - Das × schließt die ganze Gruppe.
- Neue Buttons rechts:
  - „Nebeneinander teilen“ (`Ctrl+Shift+E`)
  - „Untereinander teilen“ (`Ctrl+Shift+O`)
  - „Broadcast“ mit `aria-pressed` (`Ctrl+Shift+A`)
- Bei 4 Bereichen sind die Teilen-Buttons deaktiviert. Broadcast ist erst ab 2 Bereichen aktivierbar.
- Ist Broadcast an, steht neben dem Button „Broadcast an · N Bereiche“.

### Bereiche

- Die Bereiche werden flach gerendert (nach ID, absolut positioniert aus `arrange(layout)`), damit ein Split ein
  laufendes Terminal nie neu aufbaut. Jede Trennlinie (`role="separator"`, `aria-orientation`) lässt sich mit der
  Maus ziehen und per Tastatur (Pfeiltasten, 5-%-Schritte) verschieben. Das Ändern der Größe läuft über den bestehenden
  ResizeObserver mit Fit-Addon.
- Ein Klick in einen Bereich fokussiert ihn:
  - Der fokussierte Bereich bekommt einen Akzent-Rahmen, wenn die Gruppe mehr als einen Bereich hat.
  - Ist Broadcast an, haben alle Bereiche einen orangen Rahmen.
- Hat die Gruppe mehr als einen Bereich, zeigt jeder Bereich oben rechts ein × („Bereich schließen“).
- Ein Bereich mit beendeter Sitzung zeigt `SessionEnded`, bis er geschlossen wird.

### Auswahl im neuen Bereich (`PanePicker`)

- Erster Eintrag ist „Gleicher Host (Name)“. Er ist vorausgewählt und wird mit Enter bestätigt. Kommt der Bereich von
  einem lokalen Terminal, heißt der Eintrag „Lokales Terminal“.
- Darunter folgen ein Suchfeld und eine Host-Liste (SSH-Hosts nach Name, Suche über Name und Adresse) sowie der
  Eintrag „Lokales Terminal“.
- Die Pfeiltasten wählen, Enter öffnet, Esc schließt den Bereich.
- Wählt man einen Host, startet `useConnectStore.start(hostId, "ssh", { paneId })` den normalen Connect-Ablauf mit
  Overlay, Host-Key-Abfrage, Prompts und Fehlerseiten:
  - Bei Erfolg landet die Sitzung über `attachSession` in diesem Bereich statt in einem neuen Tab.
  - Bei Abbruch oder endgültigem Fehler bleibt der Bereich auf `"picking"`.
  - Ist der Bereich während des Verbindens geschlossen worden, wird die neue Sitzung sofort wieder geschlossen.
- Wählt man „Lokales Terminal“, ruft das `openLocal({ paneId })` auf. Das öffnet die lokale Shell in diesem Bereich.

### Seitenleiste und Snippets

Sie beziehen sich auf den fokussierten Bereich der aktiven Gruppe. In einem `"picking"`-Bereich ist der
Sitzungs-Reiter leer („Kein Terminal“) und Einfügen und ▶ sind gesperrt.

### Tastenkürzel

| Kürzel | Aktion |
|---|---|
| `Ctrl+Shift+E` | fokussierten Bereich nebeneinander teilen |
| `Ctrl+Shift+O` | fokussierten Bereich untereinander teilen |
| `Ctrl+Shift+A` | Broadcast umschalten (nicht `I`: öffnet in WebKitGTK-Debug-Builds die DevTools) |
| `Ctrl+Shift+←` / `Ctrl+Shift+→` | vorigen / nächsten Bereich fokussieren |
| `Ctrl+Shift+W` | aktive Gruppe (Tab) schließen |

- Alle Kürzel gelten nur in der Terminal-Ansicht und auch, wenn der Fokus im Terminal liegt (`TERMINAL_SAFE`).
- Ist ein Modal offen, sind sie inaktiv.
- Sie greifen in der Capture-Phase, damit xterm sie nicht bekommt.

## 3. Broadcast

- Der Schalter gilt pro Gruppe (`TabGroup.broadcast`).
- Ist er an, schickt `TerminalView` die Eingabe des fokussierten Bereichs (`onData` / `onBinary`) zusätzlich an alle
  anderen Bereiche derselben Gruppe mit Status `"connected"` und `sessionId`, jeweils per `sessionWrite`:
  - Das gilt für Tastatur, Einfügen, Mittelklick und Snippets, auch für ▶, denn das läuft über `term.input`.
  - Die Reihenfolge pro Sitzung garantiert die bestehende Write-Queue je Sitzung.
- Nicht übertragen werden:
  - Größenänderungen;
  - Eingaben aus einem nicht fokussierten Bereich, etwa die Antwort auf eine Terminal-Abfrage;
  - etwas an Bereiche mit Status `"picking"`, `"closed"` oder `"error"`.
- Schließt man Bereiche, bis nur einer übrig ist, schaltet sich Broadcast aus.

## 4. Fehlerfälle

| Fall | Verhalten |
|---|---|
| Teilen bei 4 Bereichen | Buttons deaktiviert, Kürzel ohne Wirkung |
| Verbindung im Bereich abgebrochen oder fehlgeschlagen | Bereich zeigt wieder die Auswahl |
| Bereich während des Verbindens geschlossen | Neue Sitzung wird sofort geschlossen, kein neuer Tab |
| Sitzung in einem Bereich endet | `SessionEnded` in diesem Bereich, die anderen laufen weiter |
| Broadcast an, ein Bereich getrennt | Er wird übersprungen |
| Letzter Bereich geschlossen | Tab schließt |
| WebGL-Kontext geht verloren (bis zu 4 Kontexte) | DOM-Fallback wie bisher, pro Bereich |

## 5. Tests

- `layout.ts`: Teilen, Entfernen (Geschwister rückt nach, letzter Bereich → `null`), Reihenfolge, Nachbar zyklisch,
  Ratio-Grenzen, `MAX_PANES`.
- Store:
  - `addTab` legt eine Gruppe an.
  - `split` fokussiert den neuen `"picking"`-Bereich und greift nicht bei 4 Bereichen.
  - `closePane` schließt die Sitzung, rückt das Layout nach und schließt die Gruppe beim letzten Bereich.
  - `closeGroup` schließt alle Sitzungen.
  - Broadcast schaltet sich bei einem Bereich aus.
  - Der Fokus folgt beim Entfernen.
- Connect: `start` mit `paneId` setzt bei Erfolg die Sitzung in den Bereich, ohne neuen Tab. Bei Abbruch bleibt der
  Bereich auf `"picking"`. Ein während des Verbindens geschlossener Bereich schließt die Sitzung.
- `TerminalView`: Mit Broadcast gehen `onData` und `onBinary` an alle verbundenen Sitzungen der Gruppe, ohne Broadcast
  nur an die eigene. Aus einem nicht fokussierten Bereich wird nicht übertragen.
- `PanePicker`: Vorauswahl, Suche, Pfeiltasten, Enter, Esc, lokales Terminal.
- Tab-Leiste und Bereiche: Buttons, Zähler, Sperre bei 4, `aria-pressed`, Trennlinie verschiebt `ratio`, × je
  Bereich.
- Kürzel: Zuordnung in `shortcutFor` und Wirkung nur in der Terminal-Ansicht.
- Bestehende Tests bleiben grün, denn ein Tab mit einem Bereich verhält sich wie bisher.
- Manuell: `docs/acceptance-tp2c.md`.
