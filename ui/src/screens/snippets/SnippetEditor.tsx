import { useState } from "react";
import { Button, Dialog, Select, TextArea, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import { type Id, newId, type Snippet } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useHostsStore } from "../../stores/hosts";
import { useSnippetsStore } from "../../stores/snippets";
import { useUiStore } from "../../stores/ui";
import styles from "./Snippets.module.css";

const GLOBAL = "global";

/** Create (`id` undefined) or edit a snippet. `hostId` preselects the scope of a new one. */
export function SnippetEditor({ id, hostId }: { id?: Id; hostId: Id | null }) {
  const texts = t.snippets.editor;
  const existing = useSnippetsStore((state) => state.snippets.find((snippet) => snippet.id === id));
  const save = useSnippetsStore((state) => state.save);
  const remove = useSnippetsStore((state) => state.remove);
  const allHosts = useHostsStore((state) => state.hosts);
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  // Only stored hosts can own snippets: ssh-config hosts are not in the DB (FK).
  const hosts = allHosts.filter((host) => host.source === "linascp");
  const preset = existing ? existing.hostId : hostId;

  const [name, setName] = useState(existing?.name ?? "");
  const [command, setCommand] = useState(existing?.command ?? "");
  const [scope, setScope] = useState(preset && hosts.some((host) => host.id === preset) ? preset : GLOBAL);
  const [touched, setTouched] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);

  const run = async (action: () => Promise<unknown>, failed: string) => {
    setBusy(true);
    try {
      await action();
      closeModal();
    } catch (error) {
      toast.show({ tone: "red", message: failed, details: errorDetails(error) });
    } finally {
      setBusy(false);
    }
  };

  const submit = () => {
    setTouched(true);
    if (!name.trim() || !command.trim()) return;
    const snippet: Snippet = {
      id: existing?.id ?? newId(),
      name: name.trim(),
      command,
      hostId: scope === GLOBAL ? null : scope,
      createdAt: existing?.createdAt ?? 0,
      updatedAt: existing?.updatedAt ?? 0,
    };
    void run(() => save(snippet), texts.saveFailed);
  };

  const footer = confirmDelete ? (
    <>
      <span className={styles.confirm}>{texts.deleteConfirm}</span>
      <Button variant="ghost" onClick={() => setConfirmDelete(false)}>
        {t.common.cancel}
      </Button>
      <Button
        variant="danger"
        disabled={busy}
        onClick={() => existing && void run(() => remove(existing.id), texts.deleteFailed)}
      >
        {t.common.delete}
      </Button>
    </>
  ) : (
    <>
      {existing ? (
        <Button variant="danger" onClick={() => setConfirmDelete(true)}>
          {t.common.delete}
        </Button>
      ) : null}
      <Button variant="ghost" onClick={closeModal}>
        {t.common.cancel}
      </Button>
      <Button variant="primary" disabled={busy} onClick={submit}>
        {t.common.save}
      </Button>
    </>
  );

  return (
    <Dialog open size="md" title={existing ? texts.titleEdit : texts.titleNew} onClose={closeModal} footer={footer}>
      <div className={styles.form}>
        <TextField
          label={texts.name}
          value={name}
          onChange={setName}
          error={touched && !name.trim() ? texts.nameRequired : null}
          maxLength={64}
        />
        <TextArea
          label={texts.command}
          value={command}
          onChange={setCommand}
          error={touched && !command.trim() ? texts.commandRequired : null}
          hint={texts.commandHint}
          mono
          rows={6}
        />
        <Select
          label={texts.scope}
          value={scope}
          options={[
            { value: GLOBAL, label: t.snippets.global },
            ...hosts.map((host) => ({ value: host.id, label: host.name })),
          ]}
          onChange={setScope}
        />
      </div>
    </Dialog>
  );
}
