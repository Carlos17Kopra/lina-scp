import { X } from "lucide-react";
import { type KeyboardEvent, type PointerEvent, type RefObject, useRef } from "react";
import { cx, IconButton } from "../../components";
import { t } from "../../i18n/de";
import { sessionReconnectNow, sessionReconnectStop } from "../../ipc/api";
import { fireAndForget } from "../../stores/connectProgress";
import { type TabGroup, useSessionsStore } from "../../stores/sessions";
import { arrange, paneIds, type Rect, type SplitDir, type SplitPath } from "./layout";
import styles from "./PaneLayout.module.css";
import { PanePicker } from "./PanePicker";
import { ReconnectBar } from "./ReconnectBar";
import { SessionEnded } from "./SessionEnded";
import { TerminalView } from "./TerminalView";

const pct = (value: number) => `${value * 100}%`;
const box = (rect: Rect) => ({ left: pct(rect.x), top: pct(rect.y), width: pct(rect.w), height: pct(rect.h) });
const KEY_STEP = 0.05;

/**
 * All panes of one tab, rendered flat (keyed by pane id) at positions from `arrange`: changing the tree
 * never remounts a running terminal.
 */
export function PaneLayout({ group, visible }: { group: TabGroup; visible: boolean }) {
  const areaRef = useRef<HTMLDivElement>(null);
  const { panes, dividers } = arrange(group.layout);
  const multi = paneIds(group.layout).length > 1;
  return (
    <div ref={areaRef} className={cx(styles.area, group.broadcast && styles.broadcasting)}>
      {panes.map(({ paneId, rect }) => (
        <Pane key={paneId} paneId={paneId} group={group} rect={rect} multi={multi} visible={visible} />
      ))}
      {dividers.map((divider) => (
        <Divider key={divider.path.join("") || "root"} groupId={group.id} areaRef={areaRef} {...divider} />
      ))}
    </div>
  );
}

function Pane({ paneId, group, rect, multi, visible }: { paneId: string; group: TabGroup; rect: Rect; multi: boolean; visible: boolean }) {
  const pane = useSessionsStore((state) => state.tabs.find((tab) => tab.id === paneId));
  const focusPane = useSessionsStore((state) => state.focusPane);
  const closeSession = useSessionsStore((state) => state.closeSession);
  if (!pane) return null;
  const focused = group.focusedPaneId === paneId;
  const ended = pane.status === "closed" || pane.status === "error" || pane.status === "lost";
  return (
    <div
      className={cx(styles.pane, multi && focused && styles.focused, group.broadcast && styles.broadcast)}
      style={box(rect)}
      data-pane={paneId}
      onMouseDown={() => focusPane(paneId)}
      onFocus={() => focusPane(paneId)}
    >
      {pane.status === "picking" ? <PanePicker pane={pane} focused={visible && focused} /> : null}
      {pane.sessionId ? <TerminalView tab={pane} visible={visible} focused={visible && focused} /> : null}
      {pane.status === "reconnecting" && pane.reconnect && pane.sessionId ? (
        <ReconnectBar
          state={pane.reconnect}
          onNow={() => fireAndForget(() => sessionReconnectNow(pane.sessionId!))}
          onStop={() => fireAndForget(() => sessionReconnectStop(pane.sessionId!))}
        />
      ) : null}
      {ended ? <SessionEnded tab={pane} /> : null}
      {multi ? (
        <IconButton
          size="sm"
          className={styles.close}
          label={t.terminal.panes.close}
          icon={X}
          onClick={() => void closeSession(paneId)}
        />
      ) : null}
    </div>
  );
}

function Divider({
  groupId,
  areaRef,
  path,
  dir,
  ratio,
  area,
}: {
  groupId: string;
  areaRef: RefObject<HTMLDivElement | null>;
  path: SplitPath;
  dir: SplitDir;
  ratio: number;
  area: Rect;
}) {
  const setRatio = useSessionsStore((state) => state.setRatio);
  const row = dir === "row";
  const style = row
    ? { left: pct(area.x + area.w * ratio), top: pct(area.y), height: pct(area.h) }
    : { top: pct(area.y + area.h * ratio), left: pct(area.x), width: pct(area.w) };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    const bounds = areaRef.current?.getBoundingClientRect();
    if (!bounds) return;
    event.preventDefault();
    const target = event.currentTarget;
    target.setPointerCapture?.(event.pointerId);
    const move = (moveEvent: globalThis.PointerEvent) => {
      const fraction = row
        ? ((moveEvent.clientX - bounds.left) / bounds.width - area.x) / area.w
        : ((moveEvent.clientY - bounds.top) / bounds.height - area.y) / area.h;
      setRatio(groupId, path, fraction);
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const grow = row ? event.key === "ArrowRight" : event.key === "ArrowDown";
    const shrink = row ? event.key === "ArrowLeft" : event.key === "ArrowUp";
    if (!grow && !shrink) return;
    event.preventDefault();
    setRatio(groupId, path, Math.round((ratio + (grow ? KEY_STEP : -KEY_STEP)) * 100) / 100);
  };

  return (
    <div
      role="separator"
      aria-label={t.terminal.panes.separator}
      aria-orientation={row ? "vertical" : "horizontal"}
      aria-valuenow={Math.round(ratio * 100)}
      aria-valuemin={15}
      aria-valuemax={85}
      tabIndex={0}
      className={cx(styles.divider, row ? styles.dividerRow : styles.dividerColumn)}
      style={style}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
    />
  );
}
