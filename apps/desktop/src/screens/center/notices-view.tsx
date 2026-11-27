import type { ProjectWindowProps } from "../../stores/project/projectwindow.ts";
import { useRef, type MouseEventHandler, type KeyboardEvent } from "react";
import { formatAgo } from "../../ui/format.ts";
import { HarnessIcon, Icon } from "../../ui/icons.tsx";
import { invoke } from "../../ui/tauri.ts";
import { conflictProject, opens, asks, badge, bellLabel, copies, copyLine, doneLabel, noteAgent, skillName, subLine, unreadCount, type Held, type Note } from "../../stores/center/notices-state.ts";
import { Button, onRowKey } from "../../ui/kit.tsx";

interface Props {
  windowProps?(project: string): ProjectWindowProps;
  /// The title of the chat whose thread a note is about; null for any other note.
  chatOf(item: Note): string | null;
  items: Note[]; open: boolean; shown: Note | null; answering: ReadonlySet<string>; failed: ReadonlyMap<string, string>; keeping: ReadonlyMap<string, { hash: string; was: string }>;
  toggle(): void; back(): void; dismiss(ids: string[]): void; openItem(item: Note): void; answer(item: Note, allow: boolean): void; keep(item: Note, held: Held): void;
}
const BELL = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 15V10a6 6 0 0 1 12 0v5l2 2H4zM10 20h4"/></svg>';
export function NoticesView(p: Props) {
  const count = badge(p.items), shown = p.shown;
  return <><button type="button" className="nt-bell" title="Notifications (⌘B)" aria-label={bellLabel(p.items)} aria-expanded={p.open} onClick={p.toggle}><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: BELL }} />{count && <span className="nt-n">{count}</span>}</button>{p.open && (shown ? <Skill key={shown.id} item={shown} p={p} /> : <List p={p} />)}</>;
}
function List({ p }: { p: Props }) {
  const unread = unreadCount(p.items), list = useRef<HTMLDivElement>(null);
  // ↑ and ↓ move between the rows, round the ends; X dismisses the focused row.
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    const id = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>(".nt-row")?.dataset.id : null;
    if (id && !e.shiftKey && e.key.toLowerCase() === "x") {
      e.preventDefault();
      p.dismiss([id]);
      return;
    }

    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const rows = [...list.current?.children ?? []].filter(row => row instanceof HTMLElement && row.classList.contains("nt-row"));
    if (!rows.length || !(e.target instanceof Element)) return;
    e.preventDefault();
    e.stopPropagation();
    const at = e.target.closest(".nt-row"), index = at ? rows.indexOf(at) : -1;
    const next = index < 0 ? (e.key === "ArrowDown" ? 0 : rows.length - 1) : (index + (e.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length;
    const row = rows[next];
    if (row instanceof HTMLElement) row.focus();
  };
  return <div className="nt-pop" tabIndex={-1} onKeyDown={keys}><div className="nt-h"><b>Notifications</b>{!!unread && <span>{unread} unread</span>}{!!p.items.length && <Button look="quiet" small onClick={() => p.dismiss(p.items.map(i => i.id))}>Clear</Button>}</div><div className="nt-list" ref={list}>{p.items.length ? p.items.map(i => <Row key={i.id} item={i} p={p} />) : <div className="nt-empty">No notifications</div>}</div></div>;
}
function Row({ item: i, p }: { item: Note; p: Props }) {
  const over = p.keeping.has(i.id) && !i.done ? "Keeping" : doneLabel(i), why = p.failed.get(i.id), agent = noteAgent(i);
  const project = opens(i) === "conflict" ? conflictProject(i) : opens(i) === "project" && i.target && typeof i.target === "object" && "projects" in i.target ? i.target.projects.project ?? null : null;
  const props = project ? p.windowProps?.(project) ?? null : null;
  const openWindow: MouseEventHandler = event => {
    if (event.target instanceof Element && event.target.closest("button")) return;
    props?.clicks.onClickCapture(event);
  };
  return <div {...props?.clicks} {...props?.menu} onClickCapture={openWindow} onAuxClick={openWindow} className={`nt-row${i.read ? "" : " unread"}${i.done ? " done" : ""}`} tabIndex={0} data-id={i.id} role="button" onClick={() => p.openItem(i)} onKeyDown={onRowKey(() => p.openItem(i))}><span className="nt-dot" />{agent ? <HarnessIcon agent={agent} className="dmag-ic" /> : <span />}<span className="nt-t"><b>{i.title}</b>{i.html ? <span className="nt-md" dangerouslySetInnerHTML={{ __html: i.html }} /> : i.text && <span>{i.text.split("\n").join(", ")}</span>}<span>{subLine(i, formatAgo(i.at, "since"), p.chatOf(i))}{over && <i className="nt-done">{over}</i>}</span>{asks(i) && <span className="nt-acts"><Button look="primary" small disabled={p.answering.has(i.id)} onClick={e => { e.stopPropagation(); p.answer(i, true); }}>Allow</Button><Button small disabled={p.answering.has(i.id)} onClick={e => { e.stopPropagation(); p.answer(i, false); }}>Deny</Button>{why && <span className="nt-err">{why}</span>}</span>}</span><Button look="quiet" aria-label="Dismiss" title="Dismiss (X)" onClick={e => { e.stopPropagation(); p.dismiss([i.id]); }}><Icon name="close" /></Button></div>;
}
function Head({ item, p }: { item: Note; p: Props }) { return <div className="nt-h"><Button look="quiet" small onClick={p.back}>Notifications</Button><span>{subLine(item, formatAgo(item.at, "since"), p.chatOf(item))}</span><Button small onClick={() => p.dismiss([item.id])}>Dismiss</Button></div>; }
function Skill({ item: i, p }: { item: Note; p: Props }) {
  const pending = p.keeping.get(i.id), why = p.failed.get(i.id);
  return <div className="nt-pop"><Head item={i} p={p} /><div className="nt-list nt-note"><b>{i.title}</b>{copies(i).map((c, index) => <div className="nt-copy" key={index}><span><span>{c.machine}{c.folder && <i>{c.folder}</i>}</span>{c.held && <small>{copyLine(c)}</small>}</span><span className="nt-acts">{c.here && <Button look="quiet" small onClick={() => void invoke("skill_reveal", { folder: c.folder, name: skillName(i) }).catch(e => console.warn("showing a skill:", e))}>Show in Finder</Button>}{c.held && !i.done && <Button look={c.newest ? "primary" : "secondary"} small disabled={!!pending} onClick={() => { if (c.held) p.keep(i, c.held); }}>{pending?.hash === c.held.hash ? "Keeping" : "Keep this one"}</Button>}</span></div>)}{why && <span className="nt-err">{why}</span>}<p className="nt-hint">{i.held?.length ? "Keep one copy and it replaces the others on every machine and in every agent's folder." : "Beep changes neither copy. Make them the same, or delete one, and this row settles."}</p></div></div>;
}
