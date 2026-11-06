// Page zoom: ⌘= (or ⌘+) in, ⌘- out, ⌘0 back to 100%, in steps of 10%.

export const ZOOM_MIN = 70;
export const ZOOM_MAX = 200;
export const ZOOM_STEP = 10;

/// Fired on `window` after the zoom changed, once the page has its new size.
export const ZOOMED = "beep:zoomed";

export type ZoomKey = "in" | "out" | "reset";

/// What a key does to the zoom, or null when it is not a zoom key.
export function zoomKey(e: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey">): ZoomKey | null {
  if (!e.metaKey || e.ctrlKey || e.altKey) return null;
  if (e.key === "=" || e.key === "+") return "in";
  if (e.key === "-") return "out";
  if (e.key === "0") return "reset";
  return null;
}

/// The level, in percent, after `key`: one step from the nearest step, kept
/// between 70% and 200%.
export function nextZoom(percent: number, key: ZoomKey): number {
  if (key === "reset") return 100;
  const at = Math.round(percent / ZOOM_STEP) * ZOOM_STEP;
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, at + (key === "in" ? ZOOM_STEP : -ZOOM_STEP)));
}
