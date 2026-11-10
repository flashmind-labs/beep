import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { arrowFocus, type MenuItem } from "../ui/menu.ts";

export interface FilesMenuProps {
  items: readonly [string, string][];
  local: readonly MenuItem[];
  x: number;
  y: number;
  action(value: string): void;
  close(): void;
}

/// The right-click menu of the Files tab, at `x`, `y` and kept inside the view.
export function FilesMenu({ items, local, x, y, action, close, menuRef }: FilesMenuProps & { menuRef: RefObject<HTMLDivElement | null> }) {
  const first = useRef<HTMLButtonElement>(null);
  const [place, setPlace] = useState<CSSProperties>({});

  useLayoutEffect(() => {
    const el = menuRef.current, parent = el?.parentElement;
    if (!el || !parent) return;
    const box = parent.getBoundingClientRect(), height = Math.min(el.offsetHeight, parent.clientHeight);
    setPlace({
      maxHeight: parent.clientHeight,
      left: Math.max(0, Math.min(x - box.left, box.width - el.offsetWidth - 4)),
      top: Math.max(0, Math.min(y - box.top, box.height - height - 4)),
    });
    first.current?.focus();
  }, []);

  useEffect(() => {
    const away = (event: PointerEvent) => { if (!(event.target instanceof Node && menuRef.current?.contains(event.target))) close(); };
    const timer = setTimeout(() => document.addEventListener("pointerdown", away), 0);
    return () => { clearTimeout(timer); document.removeEventListener("pointerdown", away); };
  }, [close, menuRef]);

  const firstItem = items.findIndex(([key]) => key !== "-");
  return <div ref={menuRef} className="agctx" role="menu" aria-label="File actions" style={{ ...place, overflowY: "auto" }} onKeyDown={event => {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); return; }
    if (menuRef.current && arrowFocus(menuRef.current, event.key)) event.preventDefault();
  }} onClick={event => event.stopPropagation()}>{items.map(([key, text], index) => key === "-" ? <hr key={`rule:${index}`} /> : <button key={key} ref={index === firstItem ? first : undefined} role="menuitem" data-fact={key} className={key === "delete" ? "warn" : undefined} onClick={event => { event.stopPropagation(); action(key); }}>{text}</button>)}{!!local.length && <hr />}{local.map((item, index) => <button key={`${item.label}:${index}`} role="menuitem" onClick={event => { event.stopPropagation(); close(); item.on(); }}>{item.label}</button>)}</div>;
}
