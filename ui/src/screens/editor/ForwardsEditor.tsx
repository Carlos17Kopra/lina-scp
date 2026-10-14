import { Trash2 } from "lucide-react";
import { useId } from "react";
import { Checkbox, cx, IconButton, Select, TextField } from "../../components";
import { t } from "../../i18n/de";
import { type ForwardKind, newId, type PortForward } from "../../ipc/api";
import styles from "./Editor.module.css";

const KIND_OPTIONS: ReadonlyArray<{ value: ForwardKind; label: string }> = [
  { value: "local", label: t.editor.forwardKinds.local },
  { value: "remote", label: t.editor.forwardKinds.remote },
  { value: "dynamic", label: t.editor.forwardKinds.dynamic },
];

/** Port-forward rules are stored with the host; they take effect in TP5. */
export function ForwardsEditor({
  forwards,
  onChange,
  error,
  className,
}: {
  forwards: PortForward[];
  onChange: (forwards: PortForward[]) => void;
  error?: string;
  className?: string;
}) {
  const headingId = useId();
  const update = (index: number, patch: Partial<PortForward>) =>
    onChange(forwards.map((forward, i) => (i === index ? { ...forward, ...patch } : forward)));

  return (
    <section className={cx(styles.stack, className)} aria-labelledby={headingId}>
      <div>
        <h3 id={headingId} className={styles.sectionLabel}>
          {t.editor.portForwarding}
        </h3>
        <p className={styles.hintText}>{t.editor.forwardsHint}</p>
      </div>
      {forwards.length > 0 ? (
        <ul className={styles.forwardList}>
          {forwards.map((forward, index) => (
            <li key={forward.id} className={styles.forwardRow}>
              <Select label={t.editor.forwardKind} value={forward.kind} options={KIND_OPTIONS} onChange={(kind) => update(index, { kind })} />
              <TextField
                label={t.editor.forwardBind}
                mono
                placeholder={t.editor.forwardBindPlaceholder}
                value={forward.bind}
                onChange={(bind) => update(index, { bind })}
              />
              <TextField
                label={t.editor.forwardTarget}
                mono
                placeholder={t.editor.forwardTargetPlaceholder}
                disabled={forward.kind === "dynamic"}
                value={forward.kind === "dynamic" ? "" : forward.target}
                onChange={(target) => update(index, { target })}
              />
              <Checkbox label={t.editor.autostart} checked={forward.autostart} onChange={(autostart) => update(index, { autostart })} />
              <IconButton
                label={t.editor.removeForward}
                icon={Trash2}
                size="sm"
                onClick={() => onChange(forwards.filter((_, i) => i !== index))}
              />
            </li>
          ))}
        </ul>
      ) : null}
      {error ? <p className={styles.errorText}>{error}</p> : null}
      <button
        type="button"
        className={styles.addButton}
        onClick={() => onChange([...forwards, { id: newId(), kind: "local", bind: "", target: "", autostart: false }])}
      >
        {t.editor.addForward}
      </button>
    </section>
  );
}
