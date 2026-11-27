// Whether this window's page is on screen, and whether the bar's views are,
// which the command center draws in a pane it mounts and unmounts. Timers
// and previews stop while either is off. Rust says when a command center
// window hides, minimizes, is covered or goes to the background and when it
// shows again, and when the bar hides and shows: a hidden WKWebView may still
// call itself visible.
let windowHidden: boolean | null = null;
let nativeHidden: boolean | null = null;
let paneOff = false;
let lastEvent = "initial";
const told: (() => void)[] = [];

/// Native events and interaction take precedence over WebKit's visibility
/// and focus, which can both stay stale. Before either, use the page's state.
export const pageShown = () => windowHidden === null
  ? typeof document !== "undefined" && (!!document.hasFocus?.() || document.visibilityState === "visible")
  : !windowHidden;

/// Whether the bar's views show: the window does and, in the command
/// center, the pane is mounted.
export const paneShown = () => pageShown() && !paneOff;

/// The separate inputs to visibility, for diagnosing a session that stops reading.
export const visibilitySnapshot = () => ({
  windowHidden, nativeHidden, paneOff, visibility: document.visibilityState,
  focused: !!document.hasFocus?.(), pageShown: pageShown(), paneShown: paneShown(), lastEvent,
});

/// Runs `fn` each time the window shows or hides, or with `pane` each
/// time `paneShown()` changes. Returns a callback that stops listening.
export function onShownChange(fn: (shown: boolean) => void, pane = false): () => void {
  const now = pane ? paneShown : pageShown;
  let was = now();
  const listener = () => {
    const is = now();
    if (is !== was) { was = is; fn(is); }
  };
  told.push(listener);
  return () => {
    const at = told.indexOf(listener);
    if (at >= 0) told.splice(at, 1);
  };
}

const changed = () => { for (const fn of told) fn(); };

/// Waits for ambient views: telemetry, lists, badges, sync progress.
export const AMBIENT_MS = 1000;
export const SLOW_MS = 2000;
const ambients = new Set<() => void>();

/// Runs `fn` at most once per `ms`, after the call that started the wait,
/// while the page shows and `when()` holds. Calls made while off screen mark
/// it dirty and it runs once when shown again. `run(true)` runs it now, for
/// approvals and the user's own actions.
export function ambient(fn: () => unknown, ms: number, when: () => boolean = () => true): (urgent?: boolean) => void {
  let dirty = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const shown = () => pageShown() && when();
  const go = () => {
    if (timer !== null) { clearTimeout(timer); timer = null; }
    dirty = false;
    try { void Promise.resolve(fn()).catch(error => console.warn("ambient:", error)); } catch (error) { console.warn("ambient:", error); }
  };
  const resume = () => { if (dirty && timer === null && shown()) timer = setTimeout(go, 0); };
  ambients.add(resume);
  told.push(resume);

  return (urgent = false) => {
    dirty = true;
    if (!shown()) return;

    if (urgent) { go(); return; }
    timer ??= setTimeout(go, ms);
  };
}

/// Calls during a read share its promise and request one fresh read after it.
export function single(load: () => Promise<void>) {
  let running: Promise<void> | undefined;
  let dirty = false;
  return async () => {
    if (running) { dirty = true; return running; }
    running = (async () => {
      do { dirty = false; await load(); } while (dirty);
    })();

    try { await running; }
    finally { running = undefined; }
  };
}

/// What `latest().run` gives a read that a newer one, or `cancel()`, replaced.
export const STALE: unique symbol = Symbol("stale");

/// Guards a read against stale responses: only the newest `run` gets its
/// answer or its error; older ones, and any pending at `cancel()`, get `STALE`.
export function latest() {
  let current = 0;

  return {
    async run<T>(read: Promise<T>): Promise<T | typeof STALE> {
      const mine = ++current;

      try {
        const value = await read;
        return mine === current ? value : STALE;
      } catch (error) {
        if (mine !== current) return STALE;
        throw error;
      }
    },
    cancel() { current++; },
  };
}

/// Runs `task` one at a time: calls made while it runs share one more run that starts when
/// it ends, so a burst of changes never drops a read that is under way and ends with a read
/// that began after the last of them. Resolves when the run the call belongs to ends.
export function serial(task: () => Promise<void>) {
  let current: Promise<void> | null = null;
  let following: Promise<void> | null = null;

  const start = () => {
    const run: Promise<void> = task().finally(() => { if (current === run) current = null; });
    current = run;
    return run;
  };
  const after = () => { following = null; return start(); };

  return () => {
    if (!current) return start();
    following ??= current.then(after, after);
    return following;
  };
}

/// A route or tab changed: dirty ambient views that now show run once.
export function shownChanged() { for (const resume of ambients) resume(); }

/// The command center mounted or unmounted the pane.
export function setPaneShown(on: boolean) {
  lastEvent = on ? "pane-mounted" : "pane-unmounted";
  if (paneOff !== on) return;
  paneOff = !on;
  changed();
}

function setHidden(hidden: boolean, event: string) {
  lastEvent = event;
  if (windowHidden === hidden) return;
  windowHidden = hidden;
  changed();
}

// Each goes only to its own window. Tests load the pure helpers that read
// this with no page, so it hooks in only where there is one.
if (typeof window !== "undefined") {
  // `event.listen` would also hear other windows hiding and pause this one.
  const own = window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.();
  if (own) {
    for (const [name, hidden] of [["center-hidden", true], ["center-shown", false], ["bar-hidden", true], ["bar-shown", false]] as const) {
      void own.listen(name, () => { nativeHidden = hidden; setHidden(hidden, name); });
    }
  }

  document.addEventListener("visibilitychange", () => { lastEvent = "visibilitychange"; changed(); });
  // Focus, a key or a click means the window is on screen.
  for (const name of ["focus", "keydown", "pointerdown"]) window.addEventListener?.(name, () => { setHidden(false, name); changed(); }, true);

  // Spinners stop while the window is hidden (style.css). Test pages may have no root.
  const mark = () => document.documentElement?.toggleAttribute?.("data-hidden", !pageShown());
  told.push(mark);
  mark();
}
