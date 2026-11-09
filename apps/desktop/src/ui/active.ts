// Whether this window's page is on screen, and whether the bar's views are,
// which the command center draws in a pane it mounts and unmounts. Timers
// and previews stop while either is off. Rust says when the main window or
// the bar hides and shows: a hidden WKWebView may still call itself visible.
let windowHidden: boolean | null = null;
let nativeHidden: boolean | null = null;
let paneOff = false;
let lastEvent = "initial";
const told: (() => void)[] = [];

/// Native events and interaction take precedence over WebKit's visibility
/// and focus, which can both stay stale. Before either, use the page's state.
export const pageShown = () => windowHidden === null
  ? !!document.hasFocus?.() || document.visibilityState === "visible"
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
/// time `paneShown()` changes.
export function onShownChange(fn: (shown: boolean) => void, pane = false) {
  const now = pane ? paneShown : pageShown;
  let was = now();
  told.push(() => {
    const is = now();
    if (is !== was) { was = is; fn(is); }
  });
}

const changed = () => { for (const fn of told) fn(); };

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
  for (const [name, hidden] of [["center-hidden", true], ["center-shown", false], ["bar-hidden", true], ["bar-shown", false]] as const) {
    void window.__TAURI__.event.listen(name, () => { nativeHidden = hidden; setHidden(hidden, name); });
  }

  document.addEventListener("visibilitychange", () => { lastEvent = "visibilitychange"; changed(); });
  // Focus, a key or a click means the window is on screen.
  for (const name of ["focus", "keydown", "pointerdown"]) window.addEventListener?.(name, () => { setHidden(false, name); changed(); }, true);
}
