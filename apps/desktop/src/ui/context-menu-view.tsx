import type { ReactNode } from "react";
import { Header, Menu, MenuItem, MenuSection, Separator } from "react-aria-components";
import type { MenuItem as Item, MenuLine } from "./menu.tsx";

/// The menu of `items`; lines are numbered in order, as `menuLines` counts them.
export function ContextMenu({ title, items, choose }: { title: string; items: readonly Item[]; choose(index: number): void }) {
  let next = 0;
  const line = (item: MenuLine) => {
    const id = String(next++);
    const warn = item.kind !== "toggle" && item.warn;
    const disabled = item.kind !== "toggle" && !!item.disabled;
    return <MenuItem id={id} key={id} textValue={item.label} isDisabled={disabled} className={warn ? "warn" : ""}>{item.label}</MenuItem>;
  };

  // Toggles sit in sections of their own, so only they read as checkboxes.
  const runs = (lines: readonly MenuLine[], label: string | null, at: string) => toggleRuns(lines).map((run, i) => {
    const toggles = run[0].kind === "toggle";
    const first = next;
    const checked = run.flatMap((item, k) => item.kind === "toggle" && item.checked ? [String(first + k)] : []);
    const header = label !== null && i === 0;
    return <MenuSection key={`${at}:${i}`} aria-label={label !== null && !header ? label : undefined} selectionMode={toggles ? "multiple" : "none"} selectedKeys={toggles ? checked : undefined}>
      {header ? <Header className="lbl">{label}</Header> : null}
      {run.map(line)}
    </MenuSection>;
  });

  const body: ReactNode[] = [];
  let loose: MenuLine[] = [];
  const flush = () => {
    if (loose.some(item => item.kind === "toggle")) body.push(...runs(loose, null, `loose:${body.length}`));
    else body.push(...loose.map(line));
    loose = [];
  };
  items.forEach((item, at) => {
    if (item.kind === "section" || item.kind === "separator") flush();
    if (item.kind === "separator") body.push(<Separator key={`rule:${at}`} />);
    else if (item.kind === "section") { if (item.items.length) body.push(...runs(item.items, item.label, `section:${at}`)); }
    else loose.push(item);
  });
  flush();

  return <><span className="lbl">{title}</span><Menu aria-label={title} autoFocus="first" onAction={key => choose(Number(key))}>{body}</Menu></>;
}

/// `lines` split where they turn from actions to toggles or back.
function toggleRuns(lines: readonly MenuLine[]) {
  const runs: MenuLine[][] = [];
  for (const item of lines) {
    const last = runs.at(-1);
    if (last && (last[0].kind === "toggle") === (item.kind === "toggle")) last.push(item);
    else runs.push([item]);
  }
  return runs;
}
