import { type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { cx } from "../../components";
import { t } from "../../i18n/de";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { type Tab, useSessionsStore } from "../../stores/sessions";
import styles from "./PaneLayout.module.css";

interface Choice {
  key: string;
  label: string;
  detail?: string;
  open(): void;
}

/** What a new split pane opens: same host (preselected), another SSH host, or a local shell. */
export function PanePicker({ pane, focused }: { pane: Tab; focused: boolean }) {
  const texts = t.terminal.panes;
  const hosts = useHostsStore((state) => state.hosts);
  const openLocal = useSessionsStore((state) => state.openLocal);
  const closeSession = useSessionsStore((state) => state.closeSession);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listId = useId();

  // The focused pane (after a split, click, Ctrl+Shift+←/→) gets the keyboard.
  useEffect(() => {
    if (focused) inputRef.current?.focus();
  }, [focused]);

  const connect = (hostId: string) => void useConnectStore.getState().start(hostId, "ssh", { paneId: pane.id });
  const local: Choice = { key: "local", label: texts.local, open: () => void openLocal({ paneId: pane.id }) };
  const origin = pane.origin?.kind === "ssh" ? hosts.find((host) => host.id === pane.origin?.hostId) : undefined;
  const needle = query.trim().toLowerCase();
  const hostChoices: Choice[] = hosts
    .filter((host) => host.protocolSsh)
    .filter((host) => !needle || host.name.toLowerCase().includes(needle) || host.address.toLowerCase().includes(needle))
    .map((host) => ({ key: host.id, label: host.name, detail: host.address, open: () => connect(host.id) }));
  const first: Choice = origin
    ? { key: "same", label: texts.sameHost(origin.name), detail: origin.address, open: () => connect(origin.id) }
    : local;
  const choices = needle ? [...hostChoices, local] : [first, ...hostChoices, ...(first === local ? [] : [local])];
  const selected = Math.min(index, choices.length - 1);
  const optionId = (key: string) => `${listId}-${key}`;

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setIndex((selected + step + choices.length) % choices.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      choices[selected]?.open();
    } else if (event.key === "Escape") {
      event.preventDefault();
      void closeSession(pane.id);
    }
  };

  return (
    <div className={styles.picker}>
      <p className={styles.pickerTitle}>{texts.choose}</p>
      <input
        ref={inputRef}
        className={styles.search}
        role="combobox"
        aria-label={texts.search}
        aria-expanded="true"
        aria-controls={listId}
        aria-activedescendant={choices[selected] ? optionId(choices[selected].key) : undefined}
        placeholder={texts.search}
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          setIndex(0);
        }}
        onKeyDown={onKeyDown}
      />
      <ul id={listId} role="listbox" aria-label={texts.choices} className={styles.choices}>
        {choices.map((choice, position) => (
          <li
            key={choice.key}
            id={optionId(choice.key)}
            role="option"
            aria-selected={position === selected}
            className={cx(styles.choice, position === selected && styles.selected)}
            onMouseDown={(event) => event.preventDefault()}
            onClick={choice.open}
          >
            <span>{choice.label}</span>
            {choice.detail ? <span className={styles.detail}>{choice.detail}</span> : null}
          </li>
        ))}
      </ul>
    </div>
  );
}
