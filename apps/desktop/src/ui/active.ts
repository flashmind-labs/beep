// Whether this window's page is on screen, and whether the bar's views are,
// which the command center draws in a pane it mounts and unmounts. Timers
// and previews stop while either is off. Rust says when the main window or
// the bar hides and shows: a hidden WKWebView may still call itself visible.
let windowHidden = false;
let paneOff = false;
const told: (() => void)[] = [];

/// Whether the window shows. A focused page shows, whatever the flags say:
/// a missed shown event or a stale visibilityState left sessions unread (#449).
export const pageShown = () => !!document.hasFocus?.() || !windowHidden && document.visibilityState === "visible";

/// Whether the bar's views show: the window does and, in the command
/// center, the pane is mounted.
export const paneShown = () => pageShown() && !paneOff;

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
  if (paneOff !== on) return;
  paneOff = !on;
  changed();
}

function setHidden(hidden: boolean) {
  if (windowHidden === hidden) return;
  windowHidden = hidden;
  changed();
}

// Each goes only to its own window. Tests load the pure helpers that read
// this with no page, so it hooks in only where there is one.
if (typeof window !== "undefined") {
  for (const [name, hidden] of [["center-hidden", true], ["center-shown", false], ["bar-hidden", true], ["bar-shown", false]] as const) {
    void window.__TAURI__.event.listen(name, () => setHidden(hidden));
  }
  document.addEventListener("visibilitychange", changed);
  // Focus, a key or a click means the window is on screen.
  for (const name of ["focus", "keydown", "pointerdown"]) window.addEventListener?.(name, () => { setHidden(false); changed(); }, true);
}
