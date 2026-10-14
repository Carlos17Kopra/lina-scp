import { useId, useMemo, useState } from "react";
import { Button, Dialog, Tabs, tabId, tabPanelId, useToast } from "../../components";
import { t } from "../../i18n/de";
import { type Id, newId } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { formatEndpoint, hostColorVar, initials } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { AuthTab } from "./AuthTab";
import { ConnectionTab } from "./ConnectionTab";
import { ConnectionTestPanel, ConnectionTestStatus } from "./ConnectionTest";
import { draftGroupId, draftToHost, type EditorTab, type HostDraft, NEW_GROUP } from "./drafts";
import type { HostFrame } from "./EditorModal";
import styles from "./Editor.module.css";
import { GeneralTab } from "./GeneralTab";
import { storeSecrets } from "./secrets";
import { SftpTab } from "./SftpTab";
import { useConnectionTest } from "./useConnectionTest";
import { errorTabs, type HostField, hasErrors, validateHost } from "./validate";

const TAB_ORDER: readonly EditorTab[] = ["general", "auth", "connection", "sftp"];
const TABS = TAB_ORDER.map((id) => ({ id, label: t.editor.tabs[id] }));

export function HostEditor({
  frame,
  onChange,
  onEditProfile,
}: {
  frame: HostFrame;
  onChange: (frame: HostFrame) => void;
  onEditProfile: (profileId?: Id) => void;
}) {
  const { draft, tab, isNew } = frame;
  const groups = useHostsStore((state) => state.groups);
  const hosts = useHostsStore((state) => state.hosts);
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const test = useConnectionTest();
  const idBase = useId();
  const [touched, setTouched] = useState<ReadonlySet<HostField>>(() => new Set());
  const [saving, setSaving] = useState(false);
  const errors = useMemo(() => validateHost(draft), [draft]);
  const invalid = hasErrors(errors);

  const edit = (patch: Partial<HostDraft>, field?: HostField) => {
    onChange({ ...frame, draft: { ...draft, ...patch } });
    if (field) setTouched((previous) => new Set(previous).add(field));
  };
  const fieldError = (field: HostField) => (touched.has(field) ? errors[field] : undefined);

  const save = async () => {
    if (invalid) return;
    setSaving(true);
    try {
      const store = useHostsStore.getState();
      let groupId = draftGroupId(draft);
      if (draft.groupId === NEW_GROUP) {
        const group = await store.saveGroup({ id: newId(), name: draft.newGroupName.trim(), sortOrder: store.groups.length });
        groupId = group.id;
        // Keep the created group on a failed host save so a retry does not create it again.
        onChange({ ...frame, draft: { ...draft, groupId: group.id, newGroupName: "" } });
      }
      const saved = await store.saveHost(draftToHost(draft, groupId));
      if (draft.authSource === "custom" && !(await storeSecrets({ kind: "host", id: saved.id }, draft.auth))) {
        toast.show({ tone: "amber", message: t.editor.secretFailed });
      }
      useHostsStore.getState().select(saved.id);
      closeModal();
    } catch (error) {
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });
    } finally {
      setSaving(false);
    }
  };

  const runTest = () => void test.run(draftToHost(draft, draftGroupId(draft)), draft.authSource === "custom" ? draft.auth : null);

  const groupName = groups.find((group) => group.id === draft.groupId)?.name ?? t.hosts.noGroup;
  const address = draft.address.trim();
  const user = draft.authSource === "custom" ? draft.user.trim() : "";
  const subtitle = address ? t.editor.hostSubtitle(formatEndpoint(user, address, Number(draft.port) || 22), groupName) : groupName;
  const jumpName = hosts.find((host) => host.id === draft.jumpHostId)?.name;
  const invalidTabs = errorTabs(errors).map((id) => t.editor.tabs[id]).join(", ");

  return (
    <Dialog
      open
      size="lg"
      title={isNew ? t.editor.newHost : t.editor.editHost}
      subtitle={subtitle}
      onClose={closeModal}
      icon={
        <span className={styles.headIcon} style={{ color: hostColorVar(draft.color) }} aria-hidden="true">
          {initials(draft.name)}
        </span>
      }
      footer={
        <>
          <ConnectionTestStatus state={test.state} disabled={invalid} onRun={runTest} onCancel={test.cancel} />
          <span className={styles.spacer} />
          {invalid ? <span className={styles.invalidHint}>{t.editor.invalidHint(invalidTabs)}</span> : null}
          <Button variant="ghost" onClick={closeModal}>
            {t.common.cancel}
          </Button>
          <Button variant="primary" disabled={invalid || saving} onClick={() => void save()}>
            {t.common.save}
          </Button>
        </>
      }
    >
      <div className={styles.body}>
        <div className={styles.layout}>
          <Tabs
            label={t.editor.sections}
            tabs={TABS}
            value={tab}
            onChange={(id) => onChange({ ...frame, tab: id as EditorTab })}
            orientation="vertical"
            idBase={idBase}
            className={styles.tabs}
          />
          <div role="tabpanel" id={tabPanelId(idBase, tab)} aria-labelledby={tabId(idBase, tab)} className={styles.panel}>
            {tab === "general" ? <GeneralTab draft={draft} edit={edit} fieldError={fieldError} /> : null}
            {tab === "auth" ? <AuthTab draft={draft} edit={edit} fieldError={fieldError} onEditProfile={onEditProfile} /> : null}
            {tab === "connection" ? <ConnectionTab draft={draft} edit={edit} fieldError={fieldError} /> : null}
            {tab === "sftp" ? <SftpTab draft={draft} edit={edit} fieldError={fieldError} /> : null}
          </div>
        </div>
        <ConnectionTestPanel state={test.state} jumpName={jumpName} onAnswer={test.answer} />
      </div>
    </Dialog>
  );
}
