import { Pencil, Play } from "lucide-react";
import { useState } from "react";
import { IconButton, TextField } from "../../../components";
import { t } from "../../../i18n/de";
import type { Snippet } from "../../../ipc/api";
import { useHostsStore } from "../../../stores/hosts";
import type { Tab } from "../../../stores/sessions";
import { useSnippetsStore } from "../../../stores/snippets";
import { useUiStore } from "../../../stores/ui";
import { firstLine, matchesQuery } from "../../snippets/format";
import { PlaceholderDialog, type SnippetMode } from "../../snippets/PlaceholderDialog";
import { fillPlaceholders, placeholderNames } from "../../snippets/placeholders";
import styles from "../../snippets/Snippets.module.css";
import { getPasteTarget } from "../pasteTargets";

interface Pending {
  command: string;
  names: string[];
  mode: SnippetMode;
}

/** Sidebar tab "Snippets": the active host's snippets first, then the global ones. */
export function SnippetsPanel({ tab }: { tab: Tab }) {
  const texts = t.snippets;
  const snippets = useSnippetsStore((state) => state.snippets);
  const hosts = useHostsStore((state) => state.hosts);
  const openModal = useUiStore((state) => state.openModal);
  const [query, setQuery] = useState("");
  const [pending, setPending] = useState<Pending | null>(null);

  const hostId = tab.kind === "ssh" ? tab.hostId : undefined;
  const hostName = hosts.find((host) => host.id === hostId)?.name ?? "";
  const ready = tab.status === "connected";
  const visible = [
    ...snippets.filter((snippet) => hostId !== undefined && snippet.hostId === hostId),
    ...snippets.filter((snippet) => snippet.hostId === null),
  ].filter((snippet) => matchesQuery(snippet, query));

  const send = (command: string, mode: SnippetMode) => getPasteTarget(tab.id)?.[mode](command);
  const use = (snippet: Snippet, mode: SnippetMode) => {
    const names = placeholderNames(snippet.command);
    if (names.length > 0) setPending({ command: snippet.command, names, mode });
    else send(snippet.command, mode);
  };
  const confirm = (values: Record<string, string>) => {
    if (!pending) return;
    const { command, mode } = pending;
    setPending(null);
    // Send after the dialog unmounted: its focus trap would pull focus back from the terminal.
    queueMicrotask(() => send(fillPlaceholders(command, values), mode));
  };

  return (
    <div className={styles.panel}>
      <TextField type="search" label={texts.search} value={query} onChange={setQuery} />
      {visible.length === 0 ? (
        <p className={styles.empty}>{snippets.length === 0 ? texts.empty : texts.noMatch}</p>
      ) : (
        <ul className={styles.cards}>
          {visible.map((snippet) => (
            <li key={snippet.id} className={styles.card}>
              <button
                type="button"
                className={styles.insert}
                aria-label={texts.insert(snippet.name)}
                disabled={!ready}
                onClick={() => use(snippet, "insert")}
              >
                <span className={styles.cardHead}>
                  <span className={styles.name}>{snippet.name}</span>
                  <span className={styles.scope}>{snippet.hostId ? hostName : texts.global}</span>
                </span>
                <code className={styles.command}>{firstLine(snippet.command)}</code>
              </button>
              <span className={styles.actions}>
                <IconButton
                  label={texts.run}
                  icon={Play}
                  size="sm"
                  disabled={!ready}
                  onClick={() => use(snippet, "run")}
                />
                <IconButton
                  label={texts.edit}
                  icon={Pencil}
                  size="sm"
                  onClick={() => openModal({ type: "snippet", id: snippet.id })}
                />
              </span>
            </li>
          ))}
        </ul>
      )}
      <button type="button" className={styles.add} onClick={() => openModal({ type: "snippet", hostId: hostId ?? null })}>
        {texts.add}
      </button>
      {pending ? (
        <PlaceholderDialog
          names={pending.names}
          mode={pending.mode}
          onCancel={() => setPending(null)}
          onConfirm={confirm}
        />
      ) : null}
    </div>
  );
}
