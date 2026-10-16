import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useState } from "react";
import { useSftpStore } from "../../stores/sftp";
import { useTransfersStore } from "../../stores/transfers";
import { dropDirAt } from "./transferTargets";

/** Files dropped from outside the app onto the remote pane are uploaded. Returns whether a drag hovers it. */
export function useExternalDrop(tabId: string): boolean {
  const [over, setOver] = useState(false);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    const hit = (x: number, y: number) => {
      const scale = window.devicePixelRatio || 1;
      return document.elementFromPoint(x / scale, y / scale);
    };
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "leave") return setOver(false);
        const el = hit(payload.position.x, payload.position.y);
        const onRemote = el?.closest<HTMLElement>("[data-sftp-pane]")?.dataset.sftpPane === "remote";
        if (payload.type !== "drop") return setOver(onRemote);
        setOver(false);
        const tab = useSftpStore.getState().tabs.find((t) => t.id === tabId);
        if (!onRemote || !tab || tab.status !== "connected" || payload.paths.length === 0) return;
        const row = el?.closest<HTMLElement>("[data-sftp-row]")?.dataset.sftpRow ?? null;
        void useTransfersStore.getState().enqueue(tabId, "upload", payload.paths, dropDirAt(tab, "remote", row));
      })
      .then((fn) => (alive ? (unlisten = fn) : fn()));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [tabId]);
  return over;
}
