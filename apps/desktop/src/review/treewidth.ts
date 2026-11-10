// The diff file list's width, kept between launches: dragged by its grip,
// stepped with the arrow keys, reset by a double-click, and kept within the
// body so the diff has room.
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
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

function save(width: number) {
  try { localStorage.setItem(KEY, String(width)); } catch { /* kept for this view only */ }
}

/// Leaves 320px for the diff and 1px for the divider.
const maxWidth = (body: HTMLElement | null) => Math.max(MIN, Math.min(MAX, (body?.clientWidth || MAX + 321) - 321));

export function useTreeWidth(body: HTMLElement | null, grip: HTMLElement | null) {
  const [preferred, setPreferred] = useState(load);
  const [max, setMax] = useState(() => maxWidth(body));
  const [resizing, setResizing] = useState(false);
  const width = Math.round(Math.min(max, preferred));
  // The drag reads the width as it starts, after the grip was wired.
  const shown = useRef(width);
  shown.current = width;

  useEffect(() => {
    if (!body) return;
    const measure = () => setMax(maxWidth(body));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(body);
    return () => observer.disconnect();
  }, [body]);

  const set = (next: number) => {
    const clamped = Math.round(Math.max(MIN, Math.min(maxWidth(body), next)));
    setPreferred(clamped);
    return clamped;
  };

  // The grip lives as long as the view, so it is wired once.
  useEffect(() => {
    if (!grip) return;
    dragHandle(grip, e => {
      const from = e.clientX, start = shown.current;
      let last = start;
      setResizing(true);
      return {
        move: ev => { last = Math.round(Math.max(MIN, Math.min(maxWidth(grip.parentElement), start + ev.clientX - from))); setPreferred(last); },
        end: () => { setResizing(false); save(last); },
      };
    });
  }, [grip]);

  return {
    width, min: MIN, max, resizing,
    reset: () => save(set(DEFAULT)),
    onKey: (e: KeyboardEvent<HTMLElement>) => {
      const step = e.key === "ArrowLeft" ? -16 : e.key === "ArrowRight" ? 16 : 0;
      if (!step || e.altKey || e.ctrlKey || e.metaKey) return;
      e.preventDefault();
      e.stopPropagation();
      save(set(width + step));
    },
  };
}
