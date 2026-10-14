import { type KeyboardEvent, useId, useRef } from "react";
import { cx } from "../cx";
import { nextIndex } from "../roving";
import styles from "./Tabs.module.css";

export interface TabItem {
  id: string;
  label: string;
  disabled?: boolean;
}

export interface TabsProps {
  /** Accessible name of the tablist. */
  label: string;
  tabs: readonly TabItem[];
  value: string;
  onChange: (id: string) => void;
  orientation?: "horizontal" | "vertical";
  /** Prefix for tab/panel ids; pass it when rendering the matching `role="tabpanel"`. */
  idBase?: string;
  className?: string;
}

export function tabId(idBase: string, id: string): string {
  return `${idBase}-tab-${id}`;
}

export function tabPanelId(idBase: string, id: string): string {
  return `${idBase}-panel-${id}`;
}

export function Tabs({ label, tabs, value, onChange, orientation = "horizontal", idBase, className }: TabsProps) {
  const generatedId = useId();
  const base = idBase ?? generatedId;
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = tabs.findIndex((tab) => tab.id === value);
  const tabStop = selectedIndex >= 0 ? selectedIndex : tabs.findIndex((tab) => !tab.disabled);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = nextIndex(event.key, index, tabs.length, orientation, (i) => Boolean(tabs[i]?.disabled));
    const tab = next === null ? undefined : tabs[next];
    if (next === null || !tab) return;
    event.preventDefault();
    onChange(tab.id);
    refs.current[next]?.focus();
  };

  return (
    <div
      role="tablist"
      aria-label={label}
      aria-orientation={orientation}
      className={cx(styles.list, styles[orientation], className)}
    >
      {tabs.map((tab, index) => {
        const selected = tab.id === value;
        return (
          <button
            key={tab.id}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="tab"
            id={tabId(base, tab.id)}
            aria-selected={selected}
            aria-controls={tabPanelId(base, tab.id)}
            tabIndex={index === tabStop ? 0 : -1}
            disabled={tab.disabled}
            className={cx(styles.tab, selected && styles.selected)}
            onClick={() => onChange(tab.id)}
            onKeyDown={(event) => onKeyDown(event, index)}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}
