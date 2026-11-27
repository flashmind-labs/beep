import { useEffect, useLayoutEffect, useState, type CSSProperties, type RefObject } from "react";
import { Menu, MenuItem } from "react-aria-components/Menu";
import { Separator } from "react-aria-components/Separator";
import type { MenuAction } from "../../ui/menu.tsx";

export interface FilesMenuProps {
  items: readonly [string, string][];
  local: readonly MenuAction[];
  x: number;
  y: number;
  action(value: string): void;
  close(): void;
}

/// The right-click menu of the Files tab, at `x`, `y` and kept inside the view.
export function FilesMenu({ items, local, x, y, action, close, menuRef }: FilesMenuProps & { menuRef: RefObject<HTMLDivElement | null> }) {
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
  }, []);

  useEffect(() => {
    const away = (event: PointerEvent) => { if (!(event.target instanceof Node && menuRef.current?.contains(event.target))) close(); };
    const timer = setTimeout(() => document.addEventListener("pointerdown", away), 0);
    return () => { clearTimeout(timer); document.removeEventListener("pointerdown", away); };
  }, [close, menuRef]);

  const choose = (id: string) => {
    if (id.startsWith("fact:")) { action(id.slice(5)); return; }

    const item = local[Number(id.slice(6))];
    if (!item) return;
    close();
    item.on();
  };
  return <div ref={menuRef} className="agctx" style={{ ...place, overflowY: "auto" }} onKeyDownCapture={event => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    close();
  }} onClick={event => event.stopPropagation()}><Menu aria-label="File actions" autoFocus="first" onAction={key => choose(String(key))}>
    {items.map(([key, text], index) => key === "-" ? <Separator key={`rule:${index}`} /> : <MenuItem key={key} id={`fact:${key}`} textValue={text} data-fact={key} className={key === "delete" ? "warn" : undefined}>{text}</MenuItem>)}
    {!!local.length && <Separator />}
    {local.map((item, index) => <MenuItem key={`${item.label}:${index}`} id={`local:${index}`} textValue={item.label} isDisabled={!!item.disabled}>{item.label}</MenuItem>)}
  </Menu></div>;
}
