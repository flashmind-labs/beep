export const WINDOWS = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");

/** The app modifier: Control on Windows, Command on macOS. */
export function commandKey(e: { metaKey: boolean; ctrlKey?: boolean }, windows = WINDOWS): boolean {
  return windows ? !!e.ctrlKey : e.metaKey;
}
