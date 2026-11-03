// Reads done for one project at a time while the user moves between
// projects: each project's read runs once at a time, its answer is kept
// under the project it was asked for, and the page redraws only while that
// project still shows. Also the projects opened last, which the window
// reads ahead while idle. Pure state, no DOM, so the tests check it.

/// What `keyedReads` needs: `read` asks for one project, `got` and
/// `failed` keep its answer or why it failed, `shown` is the project on
/// screen now and `redraw` draws it again.
export interface KeyedReadDeps<T> {
  read: (key: string) => Promise<T>;
  got: (key: string, value: T) => void;
  failed: (key: string, error: unknown) => void;
  shown: () => string | null;
  redraw: () => void;
}

/// Reads keyed by project. A second ask while one is on its way gets the
/// same read, unless it needs one that starts after now (`after`, as once
/// something changed): that runs once when the read on its way ends.
export function keyedReads<T>(deps: KeyedReadDeps<T>) {
  const reading = new Map<string, Promise<void>>();
  const next = new Map<string, Promise<void>>();

  function load(key: string, after = false): Promise<void> {
    const on = reading.get(key);
    if (on && !after) return on;
    if (on) {
      let queued = next.get(key);
      if (!queued) {
        queued = on.then(() => { next.delete(key); return load(key); });
        next.set(key, queued);
      }
      return queued;
    }
    const run = (async () => {
      try {
        deps.got(key, await deps.read(key));
      } catch (e) {
        deps.failed(key, e);
      } finally {
        reading.delete(key);
      }
      if (deps.shown() === key) deps.redraw();
    })();
    reading.set(key, run);
    return run;
  }

  return { load, loading: (key: string) => reading.has(key) };
}

/// Projects remembered as opened last.
export const RECENT_MAX = 8;
/// Of those, how many are read ahead while the window is idle.
export const PREFETCH_RECENT = 3;

/// `list` with `name` first, at most `max` long.
export function touchRecent(list: readonly string[], name: string, max = RECENT_MAX): string[] {
  return [name, ...list.filter(n => n !== name)].slice(0, max);
}

/// The projects to read ahead: the last opened, not the one on screen,
/// that still exist.
export function prefetchOrder(list: readonly string[], shown: string | null, exists: (name: string) => boolean, n = PREFETCH_RECENT): string[] {
  return list.filter(name => name !== shown && exists(name)).slice(0, n);
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
