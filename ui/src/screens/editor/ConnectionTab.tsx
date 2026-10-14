import { Select, TextArea, TextField } from "../../components";
import { t } from "../../i18n/de";
import { useHostsStore } from "../../stores/hosts";
import { NO_JUMP } from "./drafts";
import styles from "./Editor.module.css";
import type { HostFormProps } from "./form";
import { ForwardsEditor } from "./ForwardsEditor";

export function ConnectionTab({ draft, edit, fieldError }: HostFormProps) {
  const hosts = useHostsStore((state) => state.hosts);
  const jumpOptions = [
    { value: NO_JUMP, label: t.editor.noJump },
    ...hosts.filter((host) => host.id !== draft.id).map((host) => ({ value: host.id, label: host.name })),
  ];
  return (
    <div className={styles.grid}>
      <Select
        className={styles.wide}
        label={t.editor.jumpHost}
        value={draft.jumpHostId}
        options={jumpOptions}
        onChange={(jumpHostId) => edit({ jumpHostId })}
      />
      <TextField
        label={t.editor.keepalive}
        mono
        inputMode="numeric"
        value={draft.keepalive}
        onChange={(keepalive) => edit({ keepalive }, "keepalive")}
        error={fieldError("keepalive")}
      />
      <TextField
        label={t.editor.timeout}
        mono
        inputMode="numeric"
        value={draft.timeout}
        onChange={(timeout) => edit({ timeout }, "timeout")}
        error={fieldError("timeout")}
      />
      <TextField
        className={styles.wide}
        label={t.editor.startCommand}
        mono
        spellCheck={false}
        placeholder={t.editor.startCommandPlaceholder}
        value={draft.startCommand}
        onChange={(startCommand) => edit({ startCommand })}
      />
      <TextArea
        className={styles.wide}
        label={t.editor.env}
        mono
        spellCheck={false}
        hint={t.editor.envHint}
        value={draft.env}
        onChange={(env) => edit({ env }, "env")}
        error={fieldError("env")}
      />
      <ForwardsEditor
        className={styles.wide}
        forwards={draft.forwards}
        onChange={(forwards) => edit({ forwards }, "forwards")}
        error={fieldError("forwards")}
      />
    </div>
  );
}
