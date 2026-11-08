// The diff file list's width, kept between launches.
import { dragHandle } from "../ui/drag.ts";

const MIN = 180;
const MAX = 600;
const DEFAULT = 250;
const KEY = "beep.diffTreeWidth";

function load() {
  try {
    const saved = localStorage.getItem(KEY);
    const width = Number(saved);
    if (saved && Number.isFinite(width)) return Math.max(MIN, Math.min(MAX, width));
  } catch { /* use the default when storage is unavailable */ }
  return DEFAULT;
}

export function initTreeResize(body: HTMLElement, grip: HTMLElement) {
  let preferred = load();
  // Leave 320px for the diff and 1px for the divider.
  const maxWidth = () => Math.max(MIN, Math.min(MAX, (body.clientWidth || MAX + 321) - 321));
  const apply = () => {
    const max = maxWidth();
    const width = Math.round(Math.min(max, preferred));
    body.style.setProperty("--dv-tree-width", `${width}px`);
    grip.setAttribute("aria-valuenow", String(width));
    grip.setAttribute("aria-valuemax", String(max));
    return width;
  };
  const set = (width: number) => {
    preferred = Math.round(Math.max(MIN, Math.min(maxWidth(), width)));
    apply();
  };
  const save = () => {
    try { localStorage.setItem(KEY, String(preferred)); } catch { /* kept for this view only */ }
  };
  grip.tabIndex = 0;
  grip.title = "Drag to resize, double-click to reset";
  grip.setAttribute("role", "separator");
  grip.setAttribute("aria-label", "Resize changed files");
  grip.setAttribute("aria-orientation", "vertical");
  grip.setAttribute("aria-valuemin", String(MIN));
  apply();
  new ResizeObserver(apply).observe(body);

  dragHandle(grip, e => {
    const from = e.clientX, start = apply();
    body.classList.add("resizing");
    return {
      move: ev => set(start + ev.clientX - from),
      end: () => { body.classList.remove("resizing"); save(); },
    };
  });
  grip.addEventListener("dblclick", () => { set(DEFAULT); save(); });
  grip.addEventListener("keydown", e => {
    const step = e.key === "ArrowLeft" ? -16 : e.key === "ArrowRight" ? 16 : 0;
    if (!step || e.altKey || e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    e.stopPropagation();
    set(apply() + step);
    save();
  });
}
