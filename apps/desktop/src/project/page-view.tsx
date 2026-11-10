import { Fragment, useLayoutEffect, useRef, type ReactElement } from "react";
import { IC } from "../ui/icons.ts";
import type { Freshness } from "../ui/pagecache.ts";
import type { ChatApproval } from "../agents/running-state.ts";
import type { ProjectActs } from "./agent.ts";
import type { ThreadsPanel } from "./threads.ts";

/// One child of a Projects page container: an element React renders, or a
/// host its feature owns. A view renders exactly one element.
export type PagePart = { kind: "view"; key: string; view: ReactElement } | { kind: "host"; host: HTMLElement };

export const hostPart = (host: HTMLElement): PagePart => ({ kind: "host", host });
export const viewPart = (key: string, view: ReactElement): PagePart => ({ kind: "view", key, view });

/// A container holding views and retained hosts as direct children. A host
/// moves only when its place changes, so a scroll, focus, selection or a
/// click inside it survives every draw.
export function Parts({ className, project, parts }: { className: string; project?: string; parts: readonly PagePart[] }) {
  const box = useRef<HTMLDivElement>(null);
  const placed = useRef<readonly HTMLElement[]>([]);

  useLayoutEffect(() => {
    const target = box.current;
    if (target) placed.current = placeParts(target, parts, placed.current);
  });

  return <div ref={box} className={className} data-project={project}>
    {parts.map(part => part.kind === "view" && <Fragment key={part.key}>{part.view}</Fragment>)}
  </div>;
}

/// Puts `parts` in order in `box`. React keeps its own elements in order, so
/// they are the children that are not hosts; only nodes out of place move.
function placeParts(box: HTMLElement, parts: readonly PagePart[], before: readonly HTMLElement[]) {
  const hosts = parts.flatMap(part => part.kind === "host" ? [part.host] : []);
  const wanted = new Set<Element>(hosts);
  for (const old of before) if (!wanted.has(old) && old.parentElement === box) old.remove();

  const own = [...box.children].filter(child => !wanted.has(child));
  let next = 0;
  const order = parts.flatMap(part => part.kind === "host" ? [part.host] : own.slice(next, ++next));

  order.forEach((node, i) => {
    const there = box.children[i];
    if (there !== node) box.insertBefore(node, there ?? null);
  });

  return hosts;
}

export function EmptyPanel({ text, bold = false }: { text: string; bold?: boolean }) {
  return <div className="cc-panel cc-empty">{bold ? <b>{text}</b> : <span>{text}</span>}</div>;
}

export function ErrorNote({ text }: { text: string }) {
  return <div className="cc-note"><span className="ddot bad"/><span className="cc-err">{text}</span></div>;
}

export function NoProjects({ add }: { add: () => void }) {
  return <div className="cc-panel cc-empty">
    <b>No projects yet</b>
    <span>A folder or a repository to clone.</span>
    <button type="button" className="pri" onClick={add} dangerouslySetInnerHTML={{ __html: `${IC.plus} Add a project` }}/>
  </div>;
}

/// The note on a list being read again: a still dot, else how old it is.
export function FreshNote({ freshness, className = "" }: { freshness: Freshness; className?: string }) {
  if (freshness.spin) return <span className={`drun sw-spin${className}`} title="Updating"/>;
  return freshness.text ? <small className={`sw-age${className}`}>{freshness.text}</small> : null;
}

/// The cards the project chat waits on, above the board.
export function NeedsYou({ cards, act }: { cards: readonly ChatApproval[]; act: ProjectActs }) {
  return <div id="sec-project-needs">
    <div className="cc-sec"><h4>Needs you</h4><small>the project chat waits on these</small></div>
    <div className="pj-needs">{cards.map(card => <NeedsCard key={card.query_id} card={card} act={act}/>)}</div>
  </div>;
}

/// One card: what it asks, its fields, and Approve, Allow for this session
/// (the same action, until the chat ends) or Deny.
function NeedsCard({ card, act }: { card: ChatApproval; act: ProjectActs }) {
  const out = act.outcome(card.query_id), a = card.approval;

  return <div className="pj-plan" style={out?.said ? { opacity: .55 } : undefined}>
    <b>{a.action}</b>
    {a.app && <small>{a.app}</small>}
    {a.fields.length > 0 && <ol>{a.fields.map((field, i) => <li key={i}>{field.label} <small>{field.value}</small></li>)}</ol>}
    <div className="acts">{out?.said ? <span className="ok">{out.said}</span> : <>
      <button type="button" className="dbtn pri small" onClick={() => act.answer(card, "allow", "Approved")}>Approve</button>
      <button type="button" className="dbtn small" onClick={() => act.answer(card, "session", "Allowed for the rest of this chat")}>Allow for this session</button>
      <button type="button" className="dbtn warn small" onClick={() => act.answer(card, "deny", "Denied")}>Deny</button>
      {out?.error && <span className="cc-err">{out.error}</span>}
    </>}</div>
  </div>;
}

/// The user's pull requests and issues, with replies and chats, as parts of
/// the detail column.
export function threadsParts(panel: ThreadsPanel): PagePart[] {
  if (panel.kind === "hidden") return [];
  if (panel.kind === "reading") return [viewPart("threads-reading", <EmptyPanel text="Reading your pull requests and issues…"/>)];

  const parts: PagePart[] = [];
  if (panel.error) parts.push(viewPart("threads-error", <div className="cc-err">{panel.error}</div>));
  parts.push(viewPart("threads-head", <div className="cc-sec">
    <h4>Your pull requests and issues<small>{panel.waiting ? `${panel.waiting} ${panel.waiting === 1 ? "needs" : "need"} a reply` : ""}</small></h4>
    <FreshNote freshness={panel.freshness}/>
    <button type="button" className="quiet small" onClick={panel.refresh}>Refresh</button>
  </div>));
  parts.push(panel.rows.length ? viewPart("threads", <div className="cc-panel">{panel.rows.map(row => <div key={`${row.thread.kind}:${row.thread.repo}:${row.thread.number}`} className="pj-task pj-issue">
    <span className="t"><a href="#" onClick={event => { event.preventDefault(); row.open(); }}>{row.code}</a>{` ${row.thread.title}`}</span>
    {row.thread.needs_reply ? <span className="chip blocked">needs reply</span> : <span className="chip done">nothing to answer</span>}
    <span className="m">{row.meta}</span>
    <span className="acts">
      {row.reply && <button type="button" className="pri" onClick={row.reply.run}>{row.reply.label}</button>}
      <button type="button" onClick={row.discuss.run}>{row.discuss.label}</button>
      <button type="button" onClick={row.open}>Open</button>
    </span>
  </div>)}</div>) : viewPart("threads-none", <EmptyPanel text="You have no open pull requests or issues here."/>));

  if (panel.chats.length) parts.push(
    viewPart("chats-head", <div className="cc-sec"><h4>Chats about one thread</h4></div>),
    viewPart("chats", <div className="cc-panel">{panel.chats.map(chat => <div key={chat.id} className="pj-task">
      <span className="t">{chat.title}</span><span className="acts"><button type="button" onClick={chat.open}>Open chat</button></span>
    </div>)}</div>));

  return parts;
}
