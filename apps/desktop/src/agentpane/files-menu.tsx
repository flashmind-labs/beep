import { useEffect, useLayoutEffect, useRef } from "react";
import { arrowFocus, type MenuItem } from "../ui/menu.ts";

export interface FilesMenuProps {
  items: readonly [string, string][];
  local: readonly MenuItem[];
  x: number;
  y: number;
  action(value: string): void;
  close(): void;
}

export function FilesMenu({ items, local, x, y, action, close }: FilesMenuProps) {
  const menu = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = menu.current, parent = el?.parentElement;
    if (!el || !parent) return;
    el.style.maxHeight = `${parent.clientHeight}px`;
    const box = parent.getBoundingClientRect();
    el.style.left = `${Math.max(0, Math.min(x - box.left, box.width - el.offsetWidth - 4))}px`;
    el.style.top = `${Math.max(0, Math.min(y - box.top, box.height - el.offsetHeight - 4))}px`;
    el.querySelector<HTMLElement>("button")?.focus();
  }, []);
  useEffect(() => {
    const away = (event: PointerEvent) => { if (!menu.current?.contains(event.target as Node)) close(); };
    const timer = setTimeout(() => document.addEventListener("pointerdown", away), 0);
    return () => { clearTimeout(timer); document.removeEventListener("pointerdown", away); };
  }, [close]);
  return <div ref={menu} className="agctx" role="menu" aria-label="File actions" style={{ overflowY: "auto" }} onKeyDown={event => {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); return; }
    if (menu.current && arrowFocus(menu.current, event.key)) event.preventDefault();
  }} onClick={event => event.stopPropagation()}>{items.map(([key, text], index) => key === "-" ? <hr key={`rule:${index}`} /> : <button key={key} role="menuitem" data-fact={key} className={key === "delete" ? "warn" : undefined} onClick={event => { event.stopPropagation(); action(key); }}>{text}</button>)}{!!local.length && <hr />}{local.map((item, index) => <button key={`${item.label}:${index}`} role="menuitem" onClick={event => { event.stopPropagation(); close(); item.on(); }}>{item.label}</button>)}</div>;
}
