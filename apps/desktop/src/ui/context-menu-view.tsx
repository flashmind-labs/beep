import { Menu, MenuItem } from "react-aria-components";
import type { MenuItem as Action } from "./menu.ts";

export function ContextMenu({ title, items, choose }: { title: string; items: readonly Action[]; choose(index: number): void }) {
  return <><span className="lbl">{title}</span><Menu aria-label={title} onAction={key => choose(Number(key))}>{items.map((item, index) => <MenuItem id={String(index)} key={index} textValue={item.label} className={item.warn ? "warn" : ""}>{item.label}</MenuItem>)}</Menu></>;
}
