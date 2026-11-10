import { useLayoutEffect, useRef } from "react";
import { formatAgo } from "../ui/format.ts";
import { IC } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import { asks, badge, bellLabel, copies, copyLine, doneLabel, skillName, subLine, unreadCount, type Held, type Item } from "./notices-state.ts";

interface Props {
  items: Item[]; open: boolean; shown: Item | null; answering: ReadonlySet<string>; failed: ReadonlyMap<string, string>; keeping: ReadonlyMap<string, { hash: string; was: string }>;
  toggle(): void; back(): void; dismiss(ids: string[]): void; openItem(item: Item): void; answer(item: Item, allow: boolean): void; keep(item: Item, held: Held): void; reply(item: Item, text: string): void;
}
const BELL = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 21h4"/></svg>';
export function NoticesView(p: Props) {
  const count = badge(p.items), shown = p.shown;
  return <><button type="button" className="nt-bell" title="Notifications (⌘B)" aria-label={bellLabel(p.items)} aria-expanded={p.open} onClick={p.toggle}><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: BELL }} />{count && <span className="nt-n">{count}</span>}</button>{p.open && (shown ? shown.server ? <Opened key={shown.id} item={shown} p={p} /> : <Skill key={shown.id} item={shown} p={p} /> : <List p={p} />)}</>;
}
function List({ p }: { p: Props }) {
  const unread = unreadCount(p.items);
  return <div className="nt-pop" tabIndex={-1}><div className="nt-h"><b>Notifications</b>{!!unread && <span>{unread} unread</span>}{!!p.items.length && <button type="button" className="dbtn small quiet" onClick={() => p.dismiss(p.items.map(i => i.id))}>Clear</button>}</div><div className="nt-list">{p.items.length ? p.items.map(i => <Row key={i.id} item={i} p={p} />) : <div className="nt-empty">No notifications</div>}</div></div>;
}
function Row({ item: i, p }: { item: Item; p: Props }) {
  const over = p.keeping.has(i.id) && !i.done ? "Keeping" : doneLabel(i), why = p.failed.get(i.id);
  return <div className={`nt-row${i.read ? "" : " unread"}${i.done ? " done" : ""}`} tabIndex={0} data-id={i.id} role="button" onClick={() => p.openItem(i)} onKeyDown={e => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); p.openItem(i); } }}><span className="nt-dot" /><span className="nt-t"><b>{i.title}</b>{!i.server && i.text && <span>{i.text.split("\n").join(", ")}</span>}<span>{subLine(i, formatAgo(i.at, "short"))}{over && <i className="nt-done">{over}</i>}</span>{asks(i) && <span className="nt-acts"><button type="button" className="dbtn pri small" disabled={p.answering.has(i.id)} onClick={e => { e.stopPropagation(); p.answer(i, true); }}>Allow</button><button type="button" className="dbtn small" disabled={p.answering.has(i.id)} onClick={e => { e.stopPropagation(); p.answer(i, false); }}>Deny</button>{why && <span className="nt-err">{why}</span>}</span>}</span><button type="button" className="dbtn quiet" aria-label="Dismiss" title="Dismiss" onClick={e => { e.stopPropagation(); p.dismiss([i.id]); }} dangerouslySetInnerHTML={{ __html: IC.x }} /></div>;
}
function Head({ item, p }: { item: Item; p: Props }) { return <div className="nt-h"><button type="button" className="dbtn small quiet" onClick={p.back}>Notifications</button><span>{subLine(item, formatAgo(item.at, "short"))}</span><button type="button" className="dbtn small" onClick={() => p.dismiss([item.id])}>Dismiss</button></div>; }
function Opened({ item, p }: { item: Item; p: Props }) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  return <div className="nt-pop"><Head item={item} p={p} /><div className="nt-list nt-note"><b>{item.title}</b><p>{item.text}</p></div><div className="nt-rep"><input ref={input} className="nt-ask" type="text" placeholder="Reply to turn this into a chat" aria-label="Reply" autoComplete="off" spellCheck={false} onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); p.reply(item, e.currentTarget.value); } }} /><button type="button" className="dbtn pri small" onClick={() => p.reply(item, input.current?.value ?? "")}>Send</button></div></div>;
}
function Skill({ item: i, p }: { item: Item; p: Props }) {
  const pending = p.keeping.get(i.id), why = p.failed.get(i.id);
  return <div className="nt-pop"><Head item={i} p={p} /><div className="nt-list nt-note"><b>{i.title}</b>{copies(i).map((c, index) => <div className="nt-copy" key={index}><span><span>{c.machine}{c.folder && <i>{c.folder}</i>}</span>{c.held && <small>{copyLine(c)}</small>}</span><span className="nt-acts">{c.here && <button type="button" className="dbtn small quiet" onClick={() => void invoke("skill_reveal", { folder: c.folder, name: skillName(i) }).catch(e => console.warn("showing a skill:", e))}>Show in Finder</button>}{c.held && !i.done && <button type="button" className={`dbtn ${c.newest ? "pri small" : "small"}`} disabled={!!pending} onClick={() => { if (c.held) p.keep(i, c.held); }}>{pending?.hash === c.held.hash ? "Keeping" : "Keep this one"}</button>}</span></div>)}{why && <span className="nt-err">{why}</span>}<p className="nt-hint">{i.held?.length ? "Keep one copy and it replaces the others on every machine and in every agent's folder." : "Beep changes neither copy. Make them the same, or delete one, and this row settles."}</p></div></div>;
}
