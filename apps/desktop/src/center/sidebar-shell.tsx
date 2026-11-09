import { useLayoutEffect, useRef, useState } from "react";
import { patchChildren } from "../ui/patch.ts";
import { IC } from "../ui/icons.ts";

export interface SideLink { label: string; svg: string; count: number; current?: boolean; hot?: boolean; shortcut?: string; className?: string; open(): void }
export type SideFooter = { text: string; dot: "off" | "bad" | "on" | null; count?: number; action: "signin" | "add" | null };
function Groups({ nodes }: { nodes: HTMLElement[] }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (host.current) patchChildren(host.current, nodes); });
  return <div className="sidebar-groups" ref={host} style={{ display: "contents" }} />;
}
export function SidebarShell({ host, phone, links, groups, footer, signIn, addMachine }: { host: HTMLElement; phone: boolean; links: SideLink[]; groups: HTMLElement[]; footer: SideFooter; signIn(): void; addMachine(): void }) {
  const [collapsed, setCollapsed] = useState(() => host.closest(".cwin")?.classList.contains("side-collapsed") ?? false);
  const toggle = () => { const win = host.closest(".cwin"); if (win) setCollapsed(win.classList.toggle("side-collapsed")); };
  return <><div className="cc-head" data-tauri-drag-region="" /><div className="cc-brand"><b>Beep</b>{!phone && <button type="button" className="cc-side-toggle" title={collapsed ? "Expand sidebar" : "Collapse sidebar"} aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"} aria-expanded={!collapsed} aria-controls="side-content" onClick={toggle}><span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" /></svg></span></button>}</div><div className="cc-side-list" id="side-content"><div className="cc-grp cc-nav">{links.map(link => <button key={link.label} type="button" className={`cc-si${link.className ? ` ${link.className}` : ""}`} title={link.label} aria-label={link.label} aria-current={link.current ? "true" : undefined} onClick={link.open}><span className="sidebar-icon" style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: link.svg }} /><span className="tt">{link.label}</span>{!!link.count && <span className={`n${link.hot ? " hot" : ""}`}>{link.count}</span>}{link.shortcut && <kbd>{link.shortcut}</kbd>}</button>)}</div><Groups nodes={groups} /><div className="cc-foot"><span>{footer.dot && <span className={`ddot${footer.dot === "on" ? "" : ` ${footer.dot}`}`} />}{footer.dot && " "}{footer.count !== undefined ? <><b>{footer.count}</b>{footer.text}</> : footer.text}</span>{footer.action === "signin" && <button type="button" className="quiet small" onClick={signIn}>Sign in</button>}{footer.action === "add" && <button type="button" className="dbtn quiet" title="Add a machine" aria-label="Add a machine" onClick={event => { event.stopPropagation(); addMachine(); }} dangerouslySetInnerHTML={{ __html: IC.plus }} />}</div></div></>;
}
