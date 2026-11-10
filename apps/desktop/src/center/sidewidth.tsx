// The command center's sidebar width: dragged on its right edge, moved with
// the arrow keys there, reset by a double-click, and kept between launches.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { dragHandle } from "../ui/drag.ts";

const SIDE_MIN = 180;
const SIDE_MAX = 420;
const SIDE_DEFAULT = 184;
const SIDE_STEP = 16;
const KEY = "beep.sideWidth";

/// The grip on the sidebar's right edge. It sets `--side-w` on the window it sits in.
export function SideGrip() {
  const grip = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(load);
  const current = useRef(width);
  current.current = width;

  useLayoutEffect(() => { grip.current?.parentElement?.style.setProperty("--side-w", `${width}px`); }, [width]);

  useEffect(() => {
    const el = grip.current, win = el?.parentElement;
    if (!el || !win) return;
    dragHandle(el, e => {
      const from = e.clientX, start = current.current;
      win.classList.add("resizing");
      return {
        move: ev => setWidth(clampWidth(start + ev.clientX - from)),
        end: () => { win.classList.remove("resizing"); save(current.current); },
      };
    });
  }, []);

  const set = (w: number) => { setWidth(w); save(w); };
  return <div ref={grip} className="cc-grip" id="side-grip" role="separator" aria-orientation="vertical" aria-label="Resize the sidebar" tabIndex={0}
    aria-valuenow={width} aria-valuemin={SIDE_MIN} aria-valuemax={SIDE_MAX}
    onDoubleClick={() => set(SIDE_DEFAULT)}
    onKeyDown={e => {
      const step = e.key === "ArrowLeft" ? -SIDE_STEP : e.key === "ArrowRight" ? SIDE_STEP : 0;
      if (!step) return;
      // The page's own arrow keys are for its lists, not for this.
      e.preventDefault();
      e.stopPropagation();
      set(clampWidth(width + step));
    }} />;
}

/// `w` as a width the sidebar may take; anything unreadable is the default.
function clampWidth(w: number | string | null): number {
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
