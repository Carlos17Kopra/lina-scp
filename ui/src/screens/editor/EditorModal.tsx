import { useState } from "react";
import type { Id } from "../../ipc/api";
import { newHost, newProfile } from "../../model/defaults";
import { useHostsStore } from "../../stores/hosts";
import { useSettingsStore } from "../../stores/settings";
import { type EditorTarget, useUiStore } from "../../stores/ui";
import { type EditorTab, type HostDraft, hostToDraft, type ProfileDraft, profileToDraft } from "./drafts";
import { HostEditor } from "./HostEditor";
import { ProfileEditor } from "./ProfileEditor";

export interface HostFrame {
  kind: "host";
  draft: HostDraft;
  tab: EditorTab;
  isNew: boolean;
}

export interface ProfileFrame {
  kind: "profile";
  draft: ProfileDraft;
  isNew: boolean;
}

export type EditorFrame = HostFrame | ProfileFrame;

function hostFrame(id?: Id): HostFrame {
  const host = id === undefined ? undefined : useHostsStore.getState().hosts.find((h) => h.id === id);
  return { kind: "host", draft: hostToDraft(host ?? newHostFromSettings()), tab: "general", isNew: !host };
}

/** "Neuer Host" starts with the defaults from the settings. */
function newHostFromSettings() {
  const { newHostUser, newHostPort, newHostKeepaliveS, newHostTimeoutS } = useSettingsStore.getState().settings;
  return newHost({ color: "accent", user: newHostUser, port: newHostPort, keepaliveS: newHostKeepaliveS, timeoutS: newHostTimeoutS });
}

function profileFrame(id?: Id): ProfileFrame {
  const profile = id === undefined ? undefined : useHostsStore.getState().profiles.find((p) => p.profile.id === id)?.profile;
  return { kind: "profile", draft: profileToDraft(profile ?? newProfile()), isNew: !profile };
}

/**
 * Host ↔ login-profile editor. Frames form a navigation stack: a profile opened from the host's
 * auth tab is pushed on top; "Zurück zum Host" pops it and the unsaved host draft reappears.
 */
export function EditorModal({ target }: { target: EditorTarget }) {
  const [stack, setStack] = useState<EditorFrame[]>(() => [target.kind === "host" ? hostFrame(target.id) : profileFrame(target.id)]);
  const top = stack[stack.length - 1];
  if (!top) return null;

  const replaceTop = (frame: EditorFrame | ((previous: EditorFrame) => EditorFrame)) =>
    setStack((current) => {
      const previous = current[current.length - 1];
      if (!previous) return current;
      return [...current.slice(0, -1), typeof frame === "function" ? frame(previous) : frame];
    });
  const pop = () => setStack((current) => current.slice(0, -1));

  if (top.kind === "host") {
    return (
      <HostEditor
        frame={top}
        onChange={(next) =>
          replaceTop(typeof next === "function" ? (previous) => (previous.kind === "host" ? next(previous) : previous) : next)
        }
        onEditProfile={(profileId) => setStack((current) => [...current, profileFrame(profileId)])}
      />
    );
  }

  const fromHost = stack[stack.length - 2]?.kind === "host";
  return (
    <ProfileEditor
      key={top.draft.id}
      frame={top}
      onChange={replaceTop}
      onBack={fromHost ? pop : undefined}
      onSaved={(profile) => {
        if (!fromHost) {
          useUiStore.getState().closeModal();
          return;
        }
        setStack((current) => {
          const host = current[current.length - 2];
          if (host?.kind !== "host") return current;
          const draft = { ...host.draft, authSource: "profile" as const, profileId: profile.id };
          return [...current.slice(0, -2), { ...host, draft }];
        });
      }}
    />
  );
}
