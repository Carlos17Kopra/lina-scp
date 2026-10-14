import { Pencil, Plus } from "lucide-react";
import { Button, OptionCards, Select, TextField } from "../../components";
import { t } from "../../i18n/de";
import type { Id } from "../../ipc/api";
import { methodLabel } from "../../model/hostFormat";
import { useHostsStore } from "../../stores/hosts";
import { AuthMethodFields } from "./AuthMethodFields";
import type { AuthSource } from "./drafts";
import styles from "./Editor.module.css";
import type { HostFormProps } from "./form";

const SOURCE_OPTIONS: ReadonlyArray<{ value: AuthSource; label: string; hint: string }> = [
  { value: "profile", label: t.editor.useProfile, hint: t.editor.useProfileHint },
  { value: "custom", label: t.editor.custom, hint: t.editor.customHint },
];

export function AuthTab({ draft, edit, fieldError, onEditProfile }: HostFormProps & { onEditProfile: (id?: Id) => void }) {
  const profiles = useHostsStore((state) => state.profiles);
  const selected = profiles.find((entry) => entry.profile.id === draft.profileId);
  const profileOptions = [
    { value: "", label: t.editor.chooseProfile, disabled: true },
    ...profiles.map(({ profile }) => ({ value: profile.id, label: `${profile.name} · ${methodLabel(profile.auth.method)}` })),
  ];

  return (
    <div className={styles.stack}>
      <OptionCards
        label={t.editor.credentials}
        value={draft.authSource}
        options={SOURCE_OPTIONS}
        onChange={(authSource) => edit({ authSource }, "profileId")}
      />
      {draft.authSource === "profile" ? (
        <>
          {profiles.length === 0 ? (
            <p className={styles.banner}>{t.editor.noProfiles}</p>
          ) : (
            <Select
              label={t.editor.profile}
              value={draft.profileId}
              options={profileOptions}
              onChange={(profileId) => edit({ profileId }, "profileId")}
              error={fieldError("profileId")}
            />
          )}
          {profiles.length === 0 && fieldError("profileId") ? <p className={styles.errorText}>{fieldError("profileId")}</p> : null}
          {selected ? (
            <dl className={styles.summary}>
              <dt>{t.editor.user}</dt>
              <dd className="mono">{selected.profile.user}</dd>
              <dt>{t.editor.method}</dt>
              <dd>{methodLabel(selected.profile.auth.method)}</dd>
              <dt>{t.editor.key}</dt>
              <dd className="mono">{selected.profile.auth.keyPath ?? t.hosts.none}</dd>
              <dt>{t.editor.usedBy}</dt>
              <dd>{t.editor.hostCount(selected.usedBy)}</dd>
            </dl>
          ) : null}
          <div className={styles.row}>
            <Button icon={Pencil} disabled={!selected} onClick={() => onEditProfile(draft.profileId)}>
              {t.editor.editProfileButton}
            </Button>
            <Button icon={Plus} onClick={() => onEditProfile()}>
              {t.editor.newProfileButton}
            </Button>
          </div>
        </>
      ) : (
        <>
          <TextField label={t.editor.user} mono spellCheck={false} value={draft.user} onChange={(user) => edit({ user })} />
          <AuthMethodFields auth={draft.auth} onChange={(auth) => edit({ auth }, "keyPath")} keyPathError={fieldError("keyPath")} />
        </>
      )}
    </div>
  );
}
