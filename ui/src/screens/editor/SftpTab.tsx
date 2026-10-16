import { cx, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { SftpDraft } from "./drafts";
import styles from "./Editor.module.css";
import type { HostFormProps } from "./form";

/** Stored with the host; used by the SFTP file manager from TP3 on. */
export function SftpTab({ draft, edit, fieldError }: HostFormProps) {
  const set = (patch: Partial<SftpDraft>, field?: "parallel" | "uploadPerms" | "bandwidthLimit") => edit({ sftp: { ...draft.sftp, ...patch } }, field);
  return (
    <div className={styles.grid}>
      <p className={cx(styles.banner, styles.wide)}>{t.editor.sftpHint}</p>
      <TextField
        label={t.editor.remoteStart}
        mono
        spellCheck={false}
        value={draft.sftp.remoteStart}
        onChange={(remoteStart) => set({ remoteStart })}
      />
      <TextField
        label={t.editor.localStart}
        mono
        spellCheck={false}
        value={draft.sftp.localStart}
        onChange={(localStart) => set({ localStart })}
      />
      <TextField
        label={t.editor.parallel}
        mono
        inputMode="numeric"
        value={draft.sftp.parallel}
        onChange={(parallel) => set({ parallel }, "parallel")}
        error={fieldError("parallel")}
      />
      <TextField
        label={t.editor.uploadPerms}
        mono
        placeholder={t.editor.uploadPermsPlaceholder}
        value={draft.sftp.uploadPerms}
        onChange={(uploadPerms) => set({ uploadPerms }, "uploadPerms")}
        error={fieldError("uploadPerms")}
      />
      <TextField
        label={t.editor.bandwidth}
        mono
        inputMode="numeric"
        placeholder={t.editor.bandwidthPlaceholder}
        value={draft.sftp.bandwidthLimit}
        onChange={(bandwidthLimit) => set({ bandwidthLimit }, "bandwidthLimit")}
        error={fieldError("bandwidthLimit")}
      />
      <TextField
        className={styles.wide}
        label={t.editor.sftpServer}
        mono
        spellCheck={false}
        value={draft.sftp.sftpServerCmd}
        onChange={(sftpServerCmd) => set({ sftpServerCmd })}
      />
    </div>
  );
}
