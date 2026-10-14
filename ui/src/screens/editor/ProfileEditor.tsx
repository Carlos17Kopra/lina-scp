import { ArrowLeft } from "lucide-react";
import { useMemo, useState } from "react";
import { Button, cx, Dialog, TextField, useToast } from "../../components";
import { t } from "../../i18n/de";
import type { LoginProfile } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { initials } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { AuthMethodFields } from "./AuthMethodFields";
import { draftToProfile, type ProfileDraft } from "./drafts";
import type { ProfileFrame } from "./EditorModal";
import styles from "./Editor.module.css";
import { storeSecrets } from "./secrets";
import { hasErrors, type ProfileField, validateProfile } from "./validate";

export function ProfileEditor({
  frame,
  onChange,
  onBack,
  onSaved,
}: {
  frame: ProfileFrame;
  onChange: (frame: ProfileFrame) => void;
  /** Set when the profile was opened from a host ("Zurück zum Host"). */
  onBack?: () => void;
  onSaved: (profile: LoginProfile) => void;
}) {
  const { draft, isNew } = frame;
  const usedBy = useHostsStore((state) => state.profiles.find((entry) => entry.profile.id === draft.id)?.usedBy ?? 0);
  const closeModal = useUiStore((state) => state.closeModal);
  const toast = useToast();
  const [touched, setTouched] = useState<ReadonlySet<ProfileField>>(() => new Set());
  const [saving, setSaving] = useState(false);
  const errors = useMemo(() => validateProfile(draft), [draft]);
  const invalid = hasErrors(errors);

  const edit = (patch: Partial<ProfileDraft>, field?: ProfileField) => {
    onChange({ ...frame, draft: { ...draft, ...patch } });
    if (field) setTouched((previous) => new Set(previous).add(field));
  };
  const fieldError = (field: ProfileField) => (touched.has(field) ? errors[field] : undefined);

  const save = async () => {
    if (invalid) return;
    setSaving(true);
    try {
      const saved = await useHostsStore.getState().saveProfile(draftToProfile(draft));
      if (!(await storeSecrets({ kind: "profile", id: saved.id }, draft.auth))) {
        toast.show({ tone: "amber", message: t.editor.secretFailed });
      }
      onSaved(saved);
    } catch (error) {
      toast.show({ tone: "red", message: t.shell.saveFailed, details: errorDetails(error) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open
      size="lg"
      title={isNew ? t.editor.newProfile : t.editor.editProfile}
      subtitle={t.editor.profileSubtitle(draft.user.trim() || t.hosts.none, usedBy)}
      onClose={closeModal}
      icon={
        <span className={cx(styles.headIcon, styles.profileIcon)} aria-hidden="true">
          {initials(draft.name)}
        </span>
      }
      footer={
        <>
          {onBack ? (
            <Button variant="ghost" icon={ArrowLeft} onClick={onBack}>
              {t.editor.backToHost}
            </Button>
          ) : null}
          <span className={styles.spacer} />
          <Button variant="ghost" onClick={closeModal}>
            {t.common.cancel}
          </Button>
          <Button variant="primary" disabled={invalid || saving} onClick={() => void save()}>
            {t.common.save}
          </Button>
        </>
      }
    >
      <div className={styles.stack}>
        <p className={styles.banner}>{t.editor.profileInfo(usedBy)}</p>
        <div className={styles.grid}>
          <TextField
            label={t.editor.profileName}
            value={draft.name}
            onChange={(name) => edit({ name }, "name")}
            error={fieldError("name")}
          />
          <TextField
            label={t.editor.user}
            mono
            spellCheck={false}
            value={draft.user}
            onChange={(user) => edit({ user }, "user")}
            error={fieldError("user")}
          />
        </div>
        <AuthMethodFields auth={draft.auth} onChange={(auth) => edit({ auth }, "keyPath")} keyPathError={fieldError("keyPath")} />
      </div>
    </Dialog>
  );
}
