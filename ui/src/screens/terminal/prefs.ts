import type { Host, Id, Settings } from "../../ipc/api";
import { useHostsStore } from "../../stores/hosts";
import { useSettingsStore } from "../../stores/settings";

type HostPrefs = Pick<Host, "terminalScheme" | "terminalFontSize">;
type GlobalPrefs = Pick<Settings, "terminalScheme" | "terminalFontSize">;

/** Host override per field, else the global setting. */
export function effectivePrefs(host: HostPrefs | undefined, settings: GlobalPrefs): { scheme: string; fontSize: number } {
  return {
    scheme: host?.terminalScheme ?? settings.terminalScheme,
    fontSize: host?.terminalFontSize ?? settings.terminalFontSize,
  };
}

/** The tab's host, or `undefined` for local tabs and deleted hosts. */
export function useHost(hostId: Id | undefined): Host | undefined {
  return useHostsStore((state) => (hostId === undefined ? undefined : state.hosts.find((host) => host.id === hostId)));
}

export function useTerminalPrefs(hostId: Id | undefined): { scheme: string; fontSize: number } {
  const host = useHost(hostId);
  const settings = useSettingsStore((state) => state.settings);
  return effectivePrefs(host, settings);
}
