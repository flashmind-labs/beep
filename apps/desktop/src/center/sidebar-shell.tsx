import { useEffect, useRef, useState, Fragment, type ReactNode } from "react";
import { IC } from "../ui/icons.tsx";
import type { GroupSlot } from "./state.ts";

export interface SideLink { label: string; svg: string; count: number; current?: boolean; hot?: boolean; shortcut?: string; className?: string; open(): void }
export type SideFooter = { text: string; dot: "off" | "bad" | "on" | "wait" | null; count?: number; action: "signin" | "add" | null };
export function SidebarItem({ label, svg, current, shortcut, open }: { label: string; svg: string; current: boolean; shortcut?: string; open(): void }) {
  return <button type="button" className="cc-si" title={label} aria-label={label} aria-current={current ? "true" : undefined} onClick={open}><span className="sidebar-icon" style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: svg }} /><span className="tt">{label}</span>{shortcut && <kbd>{shortcut}</kbd>}</button>;
}
export function SidebarGroup({ name, group }: { name: string; group: GroupSlot<unknown> }) {
  const items = group.items ?? [], [count, setCount] = useState(40), [height, setHeight] = useState(0), end = useRef<HTMLDivElement>(null), first = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = end.current;
    if (!el || typeof IntersectionObserver === "undefined") { if (typeof IntersectionObserver === "undefined") setCount(items.length); return; }
    let root: Element | null = el.parentElement;
    while (root && !/(auto|scroll)/.test(getComputedStyle(root).overflowY)) root = root.parentElement;
    const observer = new IntersectionObserver(entries => {
      const initial = first.current?.firstElementChild;
      const firstRow = initial?.classList.contains("cc-pjthreads") ? initial.firstElementChild : initial;
      if (!height && firstRow) { const measured = (el.getBoundingClientRect().top - firstRow.getBoundingClientRect().top) / Math.min(count, items.length); if (measured > 0) setHeight(measured); }
      if (entries.some(entry => entry.isIntersecting)) setCount(n => Math.min(items.length, n + 40));
    }, { root, rootMargin: "800px 0px" });
    observer.observe(el);
    return () => observer.disconnect();
  }, [count, height, items.length]);
  const none = typeof group.empty === "function" ? group.empty() : group.empty;
  const action = group.action;
  if (!items.length && !none) return null;
  return <div className={`cc-grp ${name}${name === "views" ? " cc-nav" : ""}`}>
    {group.title && <h3>{group.onTitle ? <button type="button" className="cc-gt" onClick={group.onTitle}>{group.title}</button> : group.title}{action && <button type="button" title={action.title === false ? undefined : action.label} aria-label={action.label} onClick={event => { event.stopPropagation(); action.run(); }} dangerouslySetInnerHTML={{ __html: IC.plus }} />}</h3>}
    {!items.length && none && <span className="cc-si none">{none}</span>}
    <div ref={first} style={{ display: "contents" }}>{items.slice(0, count).map(item => <Fragment key={group.key(item)}>{group.render(item)}</Fragment>)}</div>
    {count < items.length && <div ref={end} style={{ height: height ? `${(items.length - count) * height}px` : undefined }} />}
  </div>;
}
export function SidebarShell({ host, phone, links, groups, footer, signIn, addMachine }: { host: HTMLElement; phone: boolean; links: SideLink[]; groups: ReactNode; footer: SideFooter; signIn(): void; addMachine(): void }) {
  const [collapsed, setCollapsed] = useState(() => host.closest(".cwin")?.classList.contains("side-collapsed") ?? false);
  const toggle = () => { const win = host.closest(".cwin"); if (win) setCollapsed(win.classList.toggle("side-collapsed")); };
  return <><div className="cc-head" data-tauri-drag-region="" /><div className="cc-brand"><b>Beep</b>{!phone && <button type="button" className="cc-side-toggle" title={collapsed ? "Expand sidebar" : "Collapse sidebar"} aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"} aria-expanded={!collapsed} aria-controls="side-content" onClick={toggle}><span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" /></svg></span></button>}</div><div className="cc-side-list" id="side-content"><div className="cc-grp cc-nav">{links.map(link => <button key={link.label} type="button" className={`cc-si${link.className ? ` ${link.className}` : ""}`} title={link.label} aria-label={link.label} aria-current={link.current ? "true" : undefined} onClick={link.open}><span className="sidebar-icon" style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: link.svg }} /><span className="tt">{link.label}</span>{!!link.count && <span className={`n${link.hot ? " hot" : ""}`}>{link.count}</span>}{link.shortcut && <kbd>{link.shortcut}</kbd>}</button>)}</div>{groups}<div className="cc-foot"><span>{footer.dot && <span className={`ddot${footer.dot === "on" ? "" : ` ${footer.dot}`}`} />}{footer.dot && " "}{footer.count !== undefined ? <><b>{footer.count}</b>{footer.text}</> : footer.text}</span>{footer.action === "signin" && <button type="button" className="quiet small" onClick={signIn}>Sign in</button>}{footer.action === "add" && <button type="button" className="dbtn quiet" title="Add a machine" aria-label="Add a machine" onClick={event => { event.stopPropagation(); addMachine(); }} dangerouslySetInnerHTML={{ __html: IC.plus }} />}</div></div></>;
}
