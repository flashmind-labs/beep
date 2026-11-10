import { FilePicker } from "./file-picker.tsx";
import { StashActions, StashIndicator } from "../chat/stash-hints.tsx";
import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { SHORTCUTS } from "../ui/shortcuts.ts";
import { cloneElement, Fragment, isValidElement, useLayoutEffect, useRef, type ReactNode } from "react";
import { SessionTitle } from "./title-view.tsx";
import { createPortal, flushSync } from "react-dom";
import type { Root } from "react-dom/client";
import { openBarPage } from "../bar/page-root.ts";
import { ICON } from "../ui/icons.ts";
import { harnessInner, harnessMark } from "../ui/harnessicon.ts";

type Dock = "bottom" | "right";
export interface AgentHintsModel { value: HintViewModel; project: string | null }
interface Frame { head: ReactNode; field: ReactNode; body: HTMLElement; hints: AgentHintsModel }
interface Owner { host: HTMLElement; root: Root; revision: number; frame: Frame; portals: Map<HTMLElement, { key: number; view: ReactNode }>; children: Map<string, ReactNode>; next: number; release(): void }
let current: Owner | null = null;

function draw(owner: Owner) {
  flushSync(() => owner.root.render(<Fragment><FrameView frame={owner.frame} revision={owner.revision} />{[...owner.children].map(([key, view]) => <Fragment key={`child:${key}`}>{view}</Fragment>)}{[...owner.portals].map(([host, entry]) => createPortal(entry.view, host, entry.key))}</Fragment>));
}

export function setSessionChild(key: string, view: ReactNode) {
  const owner = current;
  if (!owner) return;
  if (view === null) owner.children.delete(key); else owner.children.set(key, view);
  draw(owner);
}

// Pane views retain their own hosts while the session root owns their React
// children. Removing a view must release its portal before removing the host.
export function setSessionPortal(host: HTMLElement, view: ReactNode) {
  const owner = current;
  if (!owner) return;
  const prior = owner.portals.get(host);
  owner.portals.set(host, { key: prior?.key ?? ++owner.next, view });
  draw(owner);
}
export function hasSessionPortal(host: HTMLElement) { return current?.portals.has(host) ?? false; }

export function removeSessionPortal(host: HTMLElement) {
  const owner = current;
  if (!owner?.portals.delete(host)) return;
  draw(owner);
}

export function DraftHead({ project }: { project: string }) {
  return <><span className="tt"><b><button className="agproj" id="agproj">{project}</button></b><small id="aghere" /></span><span id="agedit" className="agedit" /></>;
}

export function SessionHead({ agent, title, folder, project, cwd, machine, usage, saveTitle }: { agent: string; title: string; folder: string; project: boolean; cwd: string; machine: string; usage: string; saveTitle(value: string): Promise<void> }) {
  const safe = /^[a-z0-9-]+$/.test(agent) ? ` ${agent}` : "";
  return <><span className={`dapp dcoderav${safe}${harnessMark(agent) ? " mark" : ""}`} dangerouslySetInnerHTML={{ __html: harnessInner(agent) }} /><span className="tt"><SessionTitle title={title} save={saveTitle} /><small className="aglocation" title={cwd}><span className="agfolder">{project ? <button className="agproj" id="agproj">{folder}</button> : <span id="agproj">{folder}</span>}<span id="agbranch" /></span><span>@</span><span id="aghere">{machine}</span></small></span><small id="aguse" className="aguse" title="Context used / capacity · session tokens · session spend / remaining credits">{usage}</small><span id="agstate" /><span id="agedit" className="agedit" /></>;
}

export function updateSessionTitle(title: string) {
  const owner = current, head = owner?.frame.head;
  if (!owner || !isValidElement<{ title: string }>(head) || head.type !== SessionHead || head.props.title === title) return;
  owner.frame = { ...owner.frame, head: cloneElement(head, { title }) };
  draw(owner);
}

export function SessionField({ mode, name, banner, attach }: { mode: "native" | "readonly" | "writable"; name: string; banner: string; attach(files: File[]): unknown }) {
  if (mode === "native") return <div className="dinput small" id="agfield"><textarea id="agnativemsg" rows={1} aria-label="Messages are sent from the parent conversation" disabled placeholder="Open the parent conversation to send a message" /><span id="agmodel" /><button className="dbtn" id="agnativeparent">Open parent</button></div>;
  if (mode === "readonly") return <div className="dmuted dblock dbanner">{banner}</div>;
  return <><div id="agmovebox" /><div id="agplan" /><div id="agtasks" /><div data-agent-queue-slot style={{ display: "contents" }} /><div data-agent-attachment-slot style={{ display: "contents" }} /><div className="dinput small" id="agfield"><FilePicker attach={attach} /><textarea id="agmsg" rows={1} autoComplete="off" spellCheck={false} placeholder={`Message ${name}`} /><span id="agmode" /><span id="agmodel" /><button className="dbtn" id="agstop" aria-label="Stop the turn" title="Stop the turn (⌘.)" /><button className="dbtn" id="agsend" aria-label="Send" dangerouslySetInnerHTML={{ __html: ICON.send }} /><div data-agent-menu-slot style={{ display: "contents" }} /></div><div id="agpickm" /></>;
}

// A draft and the session it starts share the same dock nodes. Their terminals
// stay mounted while only the header and conversation are replaced.
export function showSessionFrame(frame: Frame, inPlace: boolean, release: () => void) {
  if (!inPlace || !current?.host.isConnected) {
    const page = openBarPage(() => { const old = current; current = null; old?.release(); });
    current = { ...page, revision: 0, frame, portals: new Map(), children: new Map(), next: 0, release };
  } else { current.release = release; current.frame = frame; }
  const owner = current;
  owner.revision++;
  draw(owner);
}

function BodySlot({ body }: { body: HTMLElement }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { const slot = host.current; slot?.append(body); return () => body.remove(); }, [body]);
  return <div ref={host} style={{ display: "contents" }} />;
}

export function updateSessionHints(hints: AgentHintsModel) {
  if (!current) return;
  current.frame = { ...current.frame, hints };
  draw(current);
}

function AgentHints({ value }: { value: AgentHintsModel }) {
  const sections = SHORTCUTS.filter(section => ["Agent session", "Side pane", "Terminals"].includes(section.title));
  return <div className="agkeys"><HintView value={value.value} slim /><StashIndicator project={value.project} /><details id="agshortcuts"><summary>Show more</summary><div className="agkeys-list">{sections.map(section => <section key={section.title}><b>{section.title}</b>{section.rows.map(row => <span key={`${row.keys}:${row.does}`}><kbd>{row.keys}</kbd>{row.does}</span>)}</section>)}<StashActions /></div></details></div>;
}

function DockFrame({ dock }: { dock: Dock }) {
  return <><div className="agdiv" id={`agdiv-${dock}`} data-dock={dock} role="separator" tabIndex={0} title="Drag to resize, double-click to reset" hidden />
    <aside className="agpane" id={`agpane-${dock}`} data-dock={dock} tabIndex={-1} hidden><div className="agpane-content"><div className="agphead"><div className="agtabs" role="tablist" /><span className="agwtw" hidden /><button className="agpbtn agaddterm" data-pact="term" title="New terminal in this folder (⌃`)">+ Terminal</button><button className="agpbtn" data-pact="dock" /><button className="agpbtn" data-pact="fold" title="Hide the pane" /></div><div className="agpviews" /></div></aside>
    <button className="agpstrip" id={`agpstrip-${dock}`} data-dock={dock} hidden title="Show the pane" /></>;
}

function FrameView({ frame, revision }: { frame: Frame; revision: number }) {
  return <><div className="dhead" key={`head:${revision}`}>{frame.head}</div><div className="agshell" id="agshell"><div className="agmain" id="agmain"><div className="agconv" id="agconv" key={`conversation:${revision}`}><BodySlot body={frame.body} />{frame.field}<AgentHints value={frame.hints} /></div><DockFrame dock="bottom" /></div><DockFrame dock="right" /><div className="agdropzone" id="agdropzone" hidden /></div></>;
}
