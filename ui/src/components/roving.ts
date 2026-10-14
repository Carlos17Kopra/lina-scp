export type Orientation = "horizontal" | "vertical" | "both";

function firstEnabled(start: number, step: 1 | -1, count: number, isDisabled: (index: number) => boolean) {
  for (let i = start; i >= 0 && i < count; i += step) {
    if (!isDisabled(i)) return i;
  }
  return null;
}

/**
 * Roving-tabindex navigation: index to move to for `key`, skipping disabled items and wrapping around.
 * Returns `null` when `key` is not a navigation key for `orientation`.
 */
export function nextIndex(
  key: string,
  current: number,
  count: number,
  orientation: Orientation,
  isDisabled: (index: number) => boolean = () => false,
): number | null {
  if (key === "Home") return firstEnabled(0, 1, count, isDisabled);
  if (key === "End") return firstEnabled(count - 1, -1, count, isDisabled);
  const horizontal = orientation !== "vertical";
  const vertical = orientation !== "horizontal";
  const forward = (horizontal && key === "ArrowRight") || (vertical && key === "ArrowDown");
  const backward = (horizontal && key === "ArrowLeft") || (vertical && key === "ArrowUp");
  if (!forward && !backward) return null;
  const step = forward ? 1 : -1;
  for (let i = 1; i <= count; i += 1) {
    const candidate = (((current + step * i) % count) + count) % count;
    if (!isDisabled(candidate)) return candidate;
  }
  return null;
}
