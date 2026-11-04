// The command center's sidebar width: dragged on its right edge, moved with
// the arrow keys there, reset by a double-click, and kept between launches.

export const SIDE_MIN = 180;
export const SIDE_MAX = 420;
export const SIDE_DEFAULT = 184;
export const SIDE_STEP = 16;
const KEY = "beep.sideWidth";

/// `w` as a width the sidebar may take; anything unreadable is the default.
export function clampWidth(w: number | string | null): number {
  const n = Number(w);
  if (w === null || w === "" || !Number.isFinite(n)) return SIDE_DEFAULT;
  return Math.round(Math.min(SIDE_MAX, Math.max(SIDE_MIN, n)));
}

/// The width after `key` on the handle, or null for a key it ignores.
export function stepWidth(w: number, key: string): number | null {
  if (key === "ArrowLeft") return clampWidth(w - SIDE_STEP);
  if (key === "ArrowRight") return clampWidth(w + SIDE_STEP);
  return null;
}

function load() {
  try { return clampWidth(localStorage.getItem(KEY)); } catch { return SIDE_DEFAULT; }
}

function save(w: number) {
  try { localStorage.setItem(KEY, String(w)); } catch { /* kept for this launch only */ }
}

/// Wires the handle `grip` to the window `win`, whose `--side-w` sets the
/// sidebar's width.
export function initSideResize(win: HTMLElement, grip: HTMLElement) {
  let width = load();
  const apply = (w: number) => {
    width = w;
    win.style.setProperty("--side-w", `${w}px`);
    grip.setAttribute("aria-valuenow", String(w));
  };
  grip.setAttribute("aria-valuemin", String(SIDE_MIN));
  grip.setAttribute("aria-valuemax", String(SIDE_MAX));
  apply(width);

  grip.addEventListener("pointerdown", e => {
    if (e.button !== 0) return;
    e.preventDefault();
    grip.setPointerCapture(e.pointerId);
    const from = e.clientX, start = width;
    win.classList.add("resizing");
    const move = (ev: PointerEvent) => apply(clampWidth(start + ev.clientX - from));
    const up = () => {
      grip.removeEventListener("pointermove", move);
      grip.removeEventListener("pointerup", up);
      grip.removeEventListener("pointercancel", up);
      win.classList.remove("resizing");
      save(width);
    };
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", up);
    grip.addEventListener("pointercancel", up);
  });
  grip.addEventListener("dblclick", () => { apply(SIDE_DEFAULT); save(width); });
  grip.addEventListener("keydown", e => {
    const next = stepWidth(width, e.key);
    if (next === null) return;
    // The page's own arrow keys are for its lists, not for this.
    e.preventDefault();
    e.stopPropagation();
    apply(next);
    save(width);
  });
}
