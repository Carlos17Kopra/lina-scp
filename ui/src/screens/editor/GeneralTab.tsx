import { Checkbox, cx, Select, TextArea, TextField } from "../../components";
import { t } from "../../i18n/de";
import { useHostsStore } from "../../stores/hosts";
import { ColorSwatches } from "./ColorSwatches";
import { NEW_GROUP, NO_GROUP } from "./drafts";
import styles from "./Editor.module.css";
import type { HostFormProps } from "./form";

export function GeneralTab({ draft, edit, fieldError }: HostFormProps) {
  const groups = useHostsStore((state) => state.groups);
  const groupOptions = [
    { value: NO_GROUP, label: t.editor.noGroup },
    ...groups.map((group) => ({ value: group.id, label: group.name })),
    { value: NEW_GROUP, label: t.editor.newGroup },
  ];
  return (
    <div className={styles.grid}>
      {draft.source === "sshConfig" ? <p className={cx(styles.banner, styles.wide)}>{t.editor.sshConfigHint}</p> : null}
      <TextField
        className={styles.wide}
        label={t.editor.name}
        value={draft.name}
        onChange={(name) => edit({ name }, "name")}
        error={fieldError("name")}
      />
      <TextField
        label={t.editor.address}
        mono
        spellCheck={false}
        value={draft.address}
        onChange={(address) => edit({ address }, "address")}
        error={fieldError("address")}
      />
      <TextField
        label={t.editor.port}
        mono
        inputMode="numeric"
        value={draft.port}
        onChange={(port) => edit({ port }, "port")}
        error={fieldError("port")}
      />
      <Select label={t.editor.group} value={draft.groupId} options={groupOptions} onChange={(groupId) => edit({ groupId })} />
      <TextField label={t.editor.tags} hint={t.editor.tagsHint} value={draft.tags} onChange={(tags) => edit({ tags })} />
      {draft.groupId === NEW_GROUP ? (
        <TextField
          label={t.editor.newGroupName}
          value={draft.newGroupName}
          onChange={(newGroupName) => edit({ newGroupName }, "newGroupName")}
          error={fieldError("newGroupName")}
        />
      ) : null}
      <fieldset className={cx(styles.fieldset, styles.wide)}>
        <legend className={styles.legend}>{t.editor.protocols}</legend>
        <div className={styles.row}>
          <Checkbox label={t.editor.protocolSsh} checked={draft.protocolSsh} onChange={(protocolSsh) => edit({ protocolSsh })} />
          <Checkbox label={t.editor.protocolSftp} checked={draft.protocolSftp} onChange={(protocolSftp) => edit({ protocolSftp })} />
        </div>
      </fieldset>
      <div className={cx(styles.fieldset, styles.wide)}>
        <span className={styles.legend} aria-hidden="true">
          {t.editor.color}
        </span>
        <ColorSwatches value={draft.color} onChange={(color) => edit({ color })} />
      </div>
      <TextArea className={styles.wide} label={t.editor.notes} value={draft.notes} onChange={(notes) => edit({ notes })} />
    </div>
  );
}
