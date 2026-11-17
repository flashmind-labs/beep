// A resizable column's width: dragged on its grip, moved with the arrow keys
// there, reset by a double-click, and kept between launches.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { dragHandle } from "./drag.ts";

/// A column the grip resizes: the custom property it sets on the grip's parent, where the
/// width is kept, its bounds and the grip's label.
export interface Column {
  property: `--${string}`;
  key: string;
  min: number;
  max: number;
  initial: number;
  label: string;
}

const STEP = 16;

/// The command center's sidebar.
export const SIDEBAR: Column = { property: "--side-w", key: "beep.sideWidth", min: 180, max: 420, initial: 184, label: "Resize the sidebar" };
/// A project's sessions in place of the sidebar.
export const RAIL: Column = { property: "--rail-w", key: "beep.railWidth", min: 200, max: 480, initial: 260, label: "Resize the sessions" };
/// The Assistant page's chats.
export const CHATS: Column = { property: "--asst-w", key: "beep.chatsWidth", min: 200, max: 560, initial: 300, label: "Resize the chats" };

/// The grip on a column's right edge. It sets the column's property on the element it sits in.
export function WidthGrip({ column, id = null }: { column: Column; id?: string | null }) {
  const grip = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(() => load(column));
  const current = useRef(width);
  current.current = width;

  useLayoutEffect(() => { grip.current?.parentElement?.style.setProperty(column.property, `${width}px`); }, [width, column]);

  useEffect(() => {
    const el = grip.current, parent = el?.parentElement;
    if (!el || !parent) return;
    dragHandle(el, e => {
      const from = e.clientX, start = current.current;
      parent.classList.add("resizing");
      return {
        move: ev => setWidth(clampWidth(column, start + ev.clientX - from)),
        end: () => { parent.classList.remove("resizing"); save(column, current.current); },
      };
    });
  }, [column]);

  const set = (w: number) => { setWidth(w); save(column, w); };
  return <div ref={grip} className="cc-grip" id={id ?? undefined} role="separator" aria-orientation="vertical" aria-label={column.label} tabIndex={0}
    aria-valuenow={width} aria-valuemin={column.min} aria-valuemax={column.max}
    onDoubleClick={() => set(column.initial)}
    onKeyDown={e => {
      const step = e.key === "ArrowLeft" ? -STEP : e.key === "ArrowRight" ? STEP : 0;
      if (!step) return;
      // The page's own arrow keys are for its lists, not for this.
      e.preventDefault();
      e.stopPropagation();
      set(clampWidth(column, width + step));
    }} />;
}

/// `w` as a width the column may take; anything unreadable is its default.
function clampWidth(column: Column, w: number | string | null): number {
  const n = Number(w);
  if (w === null || w === "" || !Number.isFinite(n)) return column.initial;
  return Math.round(Math.min(column.max, Math.max(column.min, n)));
}

function load(column: Column) {
  try { return clampWidth(column, localStorage.getItem(column.key)); } catch { return column.initial; }
}

function save(column: Column, w: number) {
  try { localStorage.setItem(column.key, String(w)); } catch { /* kept for this launch only */ }
}
