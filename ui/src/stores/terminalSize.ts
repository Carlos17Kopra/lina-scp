const RAIL_PX = 64;
const TAB_BAR_PX = 48;
const PADDING_PX = 16;

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/**
 * PTY size requested before the terminal exists (connect pipeline / local terminal).
 * `TerminalView` fits the real size right after it attaches and sends a resize.
 */
export function estimateTerminalSize(
  fontSize: number,
  width: number = window.innerWidth,
  height: number = window.innerHeight,
): { cols: number; rows: number } {
  const cellWidth = fontSize * 0.6;
  const cellHeight = Math.ceil(fontSize * 1.2);
  return {
    cols: clamp(Math.floor((width - RAIL_PX - PADDING_PX) / cellWidth), 20, 500),
    rows: clamp(Math.floor((height - TAB_BAR_PX - PADDING_PX) / cellHeight), 5, 300),
  };
}
