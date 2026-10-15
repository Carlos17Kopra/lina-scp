import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";
import { t } from "../../i18n/de";
import { sessionResize, sessionWrite } from "../../ipc/api";
import { fireAndForget } from "../../stores/connectProgress";
import { broadcastPeers, type Tab, useSessionsStore } from "../../stores/sessions";
import { useSettingsStore } from "../../stores/settings";
import { getStream } from "../../stores/streams";
import { dropPasteTarget, registerPasteTarget } from "./pasteTargets";
import { useUiStore } from "../../stores/ui";
import { copyText, readText } from "./clipboard";
import { useTerminalPrefs } from "./prefs";
import { clipboardShortcut, shouldUseWebgl } from "./terminalConfig";
import styles from "./TerminalView.module.css";
import { TERMINAL_FONT_FAMILY, terminalTheme } from "./theme";

/**
 * One xterm.js terminal bound to a session. Output comes from the tab's `OutputStream` (acked after
 * xterm consumed it), input/resizes go to the session. WebGL is attached only while `visible`.
 */
/**
 * Answers xterm sends on its own (device attributes, cursor position, status, DECRQM mode, focus in/out, OSC
 * color queries). They belong to the focused pane's program only and are never broadcast.
 * ponytail: pattern match; a modified F3 (`ESC[1;2R`) is not broadcast either — flag typed input if that matters.
 */
const TERMINAL_REPLY = /^(?:\x1b\[[?>=]?[\d;]*(?:c|R|n|\$y)|\x1b\[[IO]|\x1b\]\d+;[^\x07\x1b]*(?:\x07|\x1b\\))$/;

export function TerminalView({ tab, visible, focused = false }: { tab: Tab; visible: boolean; focused?: boolean }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const webglRef = useRef<WebglAddon | null>(null);
  const [webglFailed, setWebglFailed] = useState(false);
  const appTheme = useUiStore((state) => state.theme);
  const { scheme, fontSize } = useTerminalPrefs(tab.hostId);
  const renderer = useSettingsStore((state) => state.settings.terminalRenderer);
  const initial = useRef({ scheme, appTheme, fontSize });
  initial.current = { scheme, appTheme, fontSize };
  const sessionId = tab.sessionId;

  // Terminal lifetime = session lifetime.
  useEffect(() => {
    const container = containerRef.current;
    if (!container || !sessionId) return undefined;
    const term = new Terminal({
      allowProposedApi: true,
      allowTransparency: false,
      cursorBlink: true,
      fontFamily: TERMINAL_FONT_FAMILY,
      fontSize: initial.current.fontSize,
      scrollback: 10_000,
      theme: terminalTheme(initial.current.scheme, initial.current.appTheme),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(new WebLinksAddon());
    term.open(container);
    termRef.current = term;
    fitRef.current = fit;

    const reportSize = (cols: number, rows: number) => useSessionsStore.getState().updateTab(tab.id, { cols, rows });
    // Own session first, then (broadcast) every connected peer; each session keeps its own write order.
    const send = (data: string | Uint8Array) => {
      fireAndForget(() => sessionWrite(sessionId, data));
      if (typeof data === "string" && TERMINAL_REPLY.test(data)) return;
      for (const peer of broadcastPeers(tab.id)) fireAndForget(() => sessionWrite(peer, data));
    };
    const subscriptions = [
      term.onData((data) => send(data)),
      term.onBinary((data) => send(Uint8Array.from(data, (char) => char.charCodeAt(0) & 0xff))),
      term.onResize(({ cols, rows }) => {
        reportSize(cols, rows);
        fireAndForget(() => sessionResize(sessionId, cols, rows));
      }),
      term.onSelectionChange(() => {
        if (term.hasSelection()) void copyText(term.getSelection());
      }),
    ];

    const paste = () => {
      void readText().then((text) => {
        if (text) term.paste(text);
      });
    };
    term.attachCustomKeyEventHandler((event) => {
      const action = clipboardShortcut(event);
      if (!action) return true;
      if (event.type === "keydown") {
        event.preventDefault();
        if (action === "copy") {
          if (term.hasSelection()) void copyText(term.getSelection());
        } else {
          paste();
        }
      }
      return false;
    });
    // Middle click pastes the clipboard; cancelling mouseup stops WebKit's own (PRIMARY) paste.
    const onMouseUp = (event: MouseEvent) => {
      if (event.button !== 1) return;
      event.preventDefault();
      paste();
    };
    container.addEventListener("mouseup", onMouseUp, true);
    registerPasteTarget(tab.id, {
      insert: (text) => {
        term.paste(text);
        term.focus();
      },
      // term.input feeds onData, so Enter is queued behind the pasted text (and outside the paste brackets).
      run: (text) => {
        term.paste(text);
        term.input("\r");
        term.focus();
      },
    });

    const stream = getStream(tab.id);
    const detach = stream?.attach((bytes) => term.write(bytes, () => stream.ack(bytes.length)));

    return () => {
      detach?.();
      dropPasteTarget(tab.id);
      container.removeEventListener("mouseup", onMouseUp, true);
      for (const subscription of subscriptions) subscription.dispose();
      webglRef.current?.dispose();
      webglRef.current = null;
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [tab.id, sessionId]);

  // The focused pane (click, Ctrl+Shift+←/→, after a split) gets the keyboard.
  useEffect(() => {
    if (focused) termRef.current?.focus();
  }, [focused, sessionId]);

  // WebGL renderer only while visible; DOM fallback after a failure or with the "dom" setting.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    if (!shouldUseWebgl(renderer, visible, webglFailed)) {
      webglRef.current?.dispose();
      webglRef.current = null;
      return;
    }
    if (webglRef.current) return;
    try {
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        addon.dispose();
        if (webglRef.current === addon) webglRef.current = null;
        setWebglFailed(true);
      });
      term.loadAddon(addon);
      webglRef.current = addon;
    } catch {
      setWebglFailed(true);
    }
  }, [renderer, visible, webglFailed, sessionId]);

  // Fit to the container while visible (hidden terminals have no size).
  useEffect(() => {
    const container = containerRef.current;
    const fit = fitRef.current;
    const term = termRef.current;
    if (!container || !fit || !term || !sessionId || !visible) return undefined;
    const refit = () => {
      try {
        fit.fit();
      } catch {
        // not measurable yet
      }
    };
    refit();
    // The PTY was opened with an estimated size; `onResize` only fires when xterm's size changes.
    fireAndForget(() => sessionResize(sessionId, term.cols, term.rows));
    useSessionsStore.getState().updateTab(tab.id, { cols: term.cols, rows: term.rows });
    const observer = new ResizeObserver(refit);
    observer.observe(container);
    return () => observer.disconnect();
  }, [visible, sessionId]);

  useEffect(() => {
    const term = termRef.current;
    if (term) term.options.theme = terminalTheme(scheme, appTheme);
  }, [scheme, appTheme, sessionId]);

  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.fontSize = fontSize;
    if (visible) {
      try {
        fitRef.current?.fit();
      } catch {
        // not measurable yet
      }
    }
  }, [fontSize, sessionId, visible]);

  return (
    <div className={styles.frame} style={{ background: terminalTheme(scheme, appTheme).background }} data-terminal="">
      <div ref={containerRef} className={styles.host} aria-label={t.terminal.label(tab.title)} />
    </div>
  );
}
