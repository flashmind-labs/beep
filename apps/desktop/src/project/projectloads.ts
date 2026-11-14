// The projects opened last, and which of them are read ahead.

/// Projects remembered as opened last.
const RECENT_MAX = 8;
/// Of those, how many are read ahead while the window is idle.
const PREFETCH_RECENT = 3;

/// `list` with `name` first, at most `RECENT_MAX` long.
export function touchRecent(list: readonly string[], name: string): string[] {
  return [name, ...list.filter(n => n !== name)].slice(0, RECENT_MAX);
}

/// The projects to read ahead: the last opened, not the one on screen,
/// that still exist.
export function prefetchOrder(list: readonly string[], shown: string | null, exists: (name: string) => boolean): string[] {
  return list.filter(name => name !== shown && exists(name)).slice(0, PREFETCH_RECENT);
}

/// The saved list, or none when it does not parse.
export function parseRecent(text: string | null): string[] {
  try {
    const v: unknown = JSON.parse(text ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string").slice(0, RECENT_MAX) : [];
  } catch {
    return [];
  }
}
