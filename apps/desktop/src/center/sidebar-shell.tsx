import { useState, Fragment, type ReactNode } from "react";
import { IC, Icon } from "../ui/icons.tsx";
import type { GroupSlot } from "./state.ts";
import { Button, useLazyCount } from "../ui/kit.tsx";

export type SideLink = Parameters<typeof SidebarItem>[0] & { count: number };
export type SideFooter = { text: string; dot: "off" | "bad" | "on" | "dial" | null; count?: number; title?: string; action: "add" | null };
export function SidebarItem({ label, svg, current, shortcut, open, count, hot, className }: { label: string; svg: string; current?: boolean; shortcut?: string; open(): void; count?: number; hot?: boolean; className?: string }) {
  return <button type="button" className={`cc-si${className ? ` ${className}` : ""}`} title={label} aria-label={label} aria-current={current ? "true" : undefined} onClick={open}><span className="sidebar-icon" style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: svg }} /><span className="tt">{label}</span>{!!count && <span className={`n${hot ? " hot" : ""}`}>{count}</span>}{shortcut && <kbd>{shortcut}</kbd>}</button>;
}
export function SidebarGroup({ name, group }: { name: string; group: GroupSlot<unknown> }) {
  const items = group.items ?? [], { count, height, end, first } = useLazyCount<HTMLDivElement>(items.length, 40, 40, row => {
    const initial = row.firstElementChild;
    return initial?.classList.contains("cc-pjthreads") ? initial.firstElementChild : initial;
  });
  const none = typeof group.empty === "function" ? group.empty() : group.empty;
  const action = group.action;
  if (!items.length && !none) return null;
  return <div className={`cc-grp ${name}${name === "views" ? " cc-nav" : ""}`}>
    {group.title && <h3>{group.onTitle ? <button type="button" className="cc-gt" onClick={group.onTitle}>{group.title}</button> : group.title}{action && <button type="button" title={action.title === false ? undefined : action.label} aria-label={action.label} onClick={event => { event.stopPropagation(); action.run(); }}><Icon name="plus" /></button>}</h3>}
    {!items.length && none && <span className="cc-si none">{none}</span>}
    <div ref={first} style={{ display: "contents" }}>{items.slice(0, count).map(item => <Fragment key={group.key(item)}>{group.render(item)}</Fragment>)}</div>
    {count < items.length && <div ref={end} style={{ height: height ? `${(items.length - count) * height}px` : undefined }} />}
  </div>;
}
export function SidebarShell({ host, links, groups, footer, addMachine }: { host: HTMLElement; links: SideLink[]; groups: ReactNode; footer: SideFooter; addMachine(): void }) {
  const [collapsed, setCollapsed] = useState(() => host.closest(".cwin")?.classList.contains("side-collapsed") ?? false);
  const toggle = () => { const win = host.closest(".cwin"); if (win) setCollapsed(win.classList.toggle("side-collapsed")); };
  return <><div className="cc-head" data-tauri-drag-region="" /><div className="cc-brand"><b>Beep</b><button type="button" className="cc-side-toggle" title={collapsed ? "Expand sidebar" : "Collapse sidebar"} aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"} aria-expanded={!collapsed} aria-controls="side-content" onClick={toggle}><span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" /></svg></span></button></div><div className="cc-side-list" id="side-content"><div className="cc-grp cc-nav">{links.map(link => <SidebarItem key={link.label} {...link} />)}</div>{groups}<div className="cc-foot"><span title={footer.title}>{footer.dot && <span className={`ddot${footer.dot === "on" ? "" : ` ${footer.dot}`}`} />}{footer.dot && " "}{footer.count !== undefined ? <><b>{footer.count}</b>{footer.text}</> : footer.text}</span>{footer.action === "add" && <Button look="quiet" title="Add a machine" aria-label="Add a machine" onClick={event => { event.stopPropagation(); addMachine(); }} dangerouslySetInnerHTML={{ __html: IC.plus }} />}</div></div></>;
}
