import { useState } from "react";
import { Button, Segmented, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { Id, Snippet } from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { useSnippetsStore } from "../../stores/snippets";
import { useUiStore } from "../../stores/ui";
import { firstLine, matchesQuery } from "./format";
import styles from "./Snippets.module.css";

type ScopeFilter = "all" | "global" | "host";

const inScope = (snippet: Snippet, filter: ScopeFilter) =>
  filter === "all" || (filter === "global") === (snippet.hostId === null);

/** Rail screen "Snippets": manage every snippet; inserting happens in the terminal sidebar. */
export function SnippetsScreen() {
  const texts = t.snippets;
  const snippets = useSnippetsStore((state) => state.snippets);
  const hosts = useHostsStore((state) => state.hosts);
  const openModal = useUiStore((state) => state.openModal);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<ScopeFilter>("all");

  const scopeLabel = (hostId: Id | null) =>
    hostId === null ? texts.global : (hosts.find((host) => host.id === hostId)?.name ?? "");
  const rows = snippets.filter((snippet) => inScope(snippet, filter) && matchesQuery(snippet, query));

  return (
    <div className={styles.screen}>
      <header className={styles.header}>
        <h1 className={styles.title}>{texts.title}</h1>
        <TextField type="search" label={texts.search} value={query} onChange={setQuery} />
        <Segmented
          label={texts.filter.label}
          size="sm"
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: texts.filter.all },
            { value: "global", label: texts.filter.global },
            { value: "host", label: texts.filter.host },
          ]}
        />
        <Button variant="primary" onClick={() => openModal({ type: "snippet", hostId: null })}>
          {texts.add}
        </Button>
      </header>
      {rows.length === 0 ? (
        <p className={styles.empty}>{snippets.length === 0 ? texts.empty : texts.noMatch}</p>
      ) : (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>{texts.columns.name}</th>
              <th>{texts.columns.scope}</th>
              <th>{texts.columns.command}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((snippet) => (
              <tr key={snippet.id}>
                <td>
                  <button
                    type="button"
                    className={styles.link}
                    onClick={() => openModal({ type: "snippet", id: snippet.id })}
                  >
                    {snippet.name}
                  </button>
                </td>
                <td>{scopeLabel(snippet.hostId)}</td>
                <td>
                  <code className={styles.command}>{firstLine(snippet.command)}</code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
