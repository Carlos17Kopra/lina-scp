import { useMemo } from "react";
import { useToast } from "../../components";
import { t } from "../../i18n/de";
import type { Host } from "../../ipc/api";
import { errorDetails } from "../../ipc/errors";
import { useConnectStore } from "../../stores/connect";
import { useHostsStore } from "../../stores/hosts";
import { useUiStore } from "../../stores/ui";
import { findQuickConnectHost, type QuickTarget, quickConnectHost } from "./quickConnect";

export interface HostActions {
  connect(host: Host): void;
  edit(host: Host): void;
  remove(host: Host): Promise<void>;
}

export function useHostActions(): HostActions {
  const toast = useToast();
  return useMemo(
    () => ({
      connect: (host) => {
        void useConnectStore.getState().start(host.id, "ssh");
      },
      edit: (host) => useUiStore.getState().openModal({ type: "editor", target: { kind: "host", id: host.id } }),
      remove: async (host) => {
        try {
          await useHostsStore.getState().deleteHost(host.id);
          toast.show({ tone: "neutral", message: t.hosts.deleted(host.name) });
        } catch (error) {
          toast.show({ tone: "red", message: t.hosts.deleteFailed, details: errorDetails(error) });
        }
      },
    }),
    [toast],
  );
}

/**
 * Reuses the quick-connect host of the same `user@host:port` or saves a new one, then starts the SSH connection.
 * Only the find-or-save step is awaited; connect errors belong to the connect flow.
 */
export async function startQuickConnect(target: QuickTarget): Promise<void> {
  const hosts = useHostsStore.getState();
  const host = findQuickConnectHost(hosts.hosts, target) ?? (await hosts.saveHost(quickConnectHost(target)));
  void useConnectStore.getState().start(host.id, "ssh");
}
