import { useState, Fragment, type ReactNode } from "react";
import { IC, Icon } from "../ui/icons.tsx";
import { spoken } from "../ui/format.ts";
import type { GroupSlot } from "./state.ts";
import { Button, useLazyCount } from "../ui/kit.tsx";
import { invoke } from "../ui/tauri.ts";
import { SideToggle } from "./side-toggle.tsx";

export type SideLink = Parameters<typeof SidebarItem>[0] & { count: number };
export type SideFooter = { text: string; dot: "off" | "bad" | "on" | "dial" | null; count?: number; title?: string; action: "add" | null };
export function SidebarItem({ label, svg, current, shortcut, open, count, hot, className }: { label: string; svg: string; current?: boolean; shortcut?: string; open(): void; count?: number; hot?: boolean; className?: string }) {
  return <button type="button" className={`cc-si${className ? ` ${className}` : ""}`} title={label} aria-label={spoken(label, count)} aria-current={current ? "true" : undefined} onClick={open}><span className="sidebar-icon" style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: svg }} /><span className="tt">{label}</span>{!!count && <>{" "}<span className={`n${hot ? " hot" : ""}`}>{count}</span></>}{shortcut && <kbd data-key={shortcut} />}</button>;
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
const STAR_KEY = "beep.starPrompt.dismissed";
const REPO_URL = "https://github.com/flashmind-labs/beep";

/// Asks once for a star. The row opens the repository in the browser; the
/// cross hides it for good. Nothing is starred on the user's behalf.
function StarPrompt() {
  const [shown, setShown] = useState(() => { try { return localStorage.getItem(STAR_KEY) === null; } catch { return false; } });
  const dismiss = () => { setShown(false); try { localStorage.setItem(STAR_KEY, "1"); } catch { /* Storage can be unavailable. */ } };
  if (!shown) return null;
  return <div className="cc-star"><Button look="quiet" className="open" onClick={() => { void invoke("open_url", { url: REPO_URL }).catch(e => console.warn("opening the repository:", e)); dismiss(); }}><Icon name="star" size={12} />Star on GitHub</Button><Button look="quiet" title="Dismiss" aria-label="Dismiss" onClick={dismiss}><Icon name="close" /></Button></div>;
}
export function SidebarShell({ links, groups, footer, addMachine }: { links: SideLink[]; groups: ReactNode; footer: SideFooter; addMachine(): void }) {
  return <><div className="cc-head" data-tauri-drag-region="" /><div className="cc-brand"><b>Beep</b><SideToggle /></div><div className="cc-side-list" id="side-content"><div className="cc-grp cc-nav">{links.map(link => <SidebarItem key={link.label} {...link} />)}</div>{groups}<StarPrompt /><div className="cc-foot"><span title={footer.title}>{footer.dot && <span className={`ddot${footer.dot === "on" ? "" : ` ${footer.dot}`}`} />}{footer.dot && " "}{footer.count !== undefined ? <><b>{footer.count}</b>{footer.text}</> : footer.text}</span>{footer.action === "add" && <Button look="quiet" title="Add a machine" aria-label="Add a machine" onClick={event => { event.stopPropagation(); addMachine(); }} dangerouslySetInnerHTML={{ __html: IC.plus }} />}</div></div></>;
}
