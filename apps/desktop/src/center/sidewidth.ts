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
    // One layout per frame however many moves arrive; the last one wins.
    let next = width, frame = 0;
    const flush = () => { frame = 0; apply(next); };
    const move = (ev: PointerEvent) => {
      next = clampWidth(start + ev.clientX - from);
      if (!frame) frame = requestAnimationFrame(flush);
    };
    const up = () => {
      if (frame) { cancelAnimationFrame(frame); flush(); }
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
    const step = e.key === "ArrowLeft" ? -SIDE_STEP : e.key === "ArrowRight" ? SIDE_STEP : 0;
    if (!step) return;
    // The page's own arrow keys are for its lists, not for this.
    e.preventDefault();
    e.stopPropagation();
    apply(clampWidth(width + step));
    save(width);
  });
}
