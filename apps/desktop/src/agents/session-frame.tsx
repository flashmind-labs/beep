import { FilePicker } from "./file-picker.tsx";
import { StashActions, StashIndicator } from "../chat/stash-hints.tsx";
import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { SHORTCUTS } from "../ui/shortcuts.ts";
import { cloneElement, createRef, isValidElement, memo, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type MouseEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { createStore, useStore, type StoreApi } from "zustand";
import { SessionTitle } from "./title-view.tsx";
import { fixedTitle } from "../project/projectbar.ts";
import { createPortal, flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-app.tsx";
import type { ChatBody } from "../ui/turn.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { PaneDock, PaneDropZone, PaneMain, PaneShell } from "../agentpane/dock-view.tsx";
import { MoveBox, MoveHere } from "./move-view.tsx";
import { FieldAttachments, FieldControl, FieldMenu, FieldQueue, modelPickHost, StopButton, useFieldDrop, useUploadPending, WorkLine } from "./composer-ui.tsx";
import { cancelContinue, continuing } from "./continue-here.ts";
import { onLogDrawn, promptMarks, revealLogUnit } from "./logwindow.ts";
import { conversationAppearance } from "../ui/conversation-appearance.ts";
import { syncJump } from "./logview.ts";
import { Button } from "../ui/kit.tsx";
import type { SessionUsage } from "./agents.ts";

interface AgentHintsModel { value: HintViewModel; project: string | null }
interface Frame { head: ReactNode; field: ReactNode; body: ChatBody; hints: AgentHintsModel }
/// What the session page draws: the frame, and the views put in it.
interface FrameState { frame: Frame; revision: number; slots: readonly Slot[] }
interface Owner { page: BarPageSlot; store: StoreApi<FrameState>; revision: number; frame: Frame; portals: Map<HTMLElement, Slot>; children: Map<string, Slot>; next: number; release(): void }
let current: Owner | null = null;

// Each child and portal is its own store, so a pane redrawing one tab strip
// renders that strip alone instead of the whole session frame.
interface Slot { key: number; host: HTMLElement | null; view: StoreApi<ReactNode> }

function slot(owner: Owner, host: HTMLElement | null, view: ReactNode): Slot {
  return { key: ++owner.next, host, view: createStore<ReactNode>(() => view) };
}

const SlotView = memo(function SlotView({ slot }: { slot: Slot }) {
  const view = useStore(slot.view);
  return slot.host ? createPortal(view, slot.host) : view;
});

function draw(owner: Owner) {
  // Something replaced the bar's contents: the page box mounts again.
  if (!owner.page.host.isConnected) owner.page.show({ view: "session", props: { store: owner.store } });
  flushSync(() => owner.store.setState({ frame: owner.frame, revision: owner.revision, slots: [...owner.children.values(), ...owner.portals.values()] }));
}

/// The bar's session page, drawn from its frame's store.
export function SessionFrame({ store }: { store: StoreApi<FrameState> }) {
  const { frame, revision, slots } = useStore(store);
  return <><FrameView frame={frame} revision={revision} />{slots.map(entry => <SlotView key={entry.key} slot={entry} />)}</>;
}

function update(entry: Slot, view: ReactNode) {
  if (entry.view.getState() === view) return;
  flushSync(() => entry.view.setState(view, true));
}

export function setSessionChild(key: string, view: ReactNode) {
  const owner = current;
  if (!owner) return;
  const prior = owner.children.get(key);
  if (view === null) { if (owner.children.delete(key)) draw(owner); return; }
  if (prior) { update(prior, view); return; }
  owner.children.set(key, slot(owner, null, view));
  draw(owner);
}

// Views with their own hosts, as the editor buttons in the head, while the
// session root owns their React children. Removing one must release its
// portal before removing the host.
export function setSessionPortal(host: HTMLElement, view: ReactNode) {
  const owner = current;
  if (!owner) return;
  const prior = owner.portals.get(host);
  if (prior) { update(prior, view); return; }
  owner.portals.set(host, slot(owner, host, view));
  draw(owner);
}

export function removeSessionPortal(host: HTMLElement) {
  const owner = current;
  if (!owner?.portals.delete(host)) return;
  draw(owner);
}

export function DraftHead({ project, openProject, machine }: { project: string; openProject(): void; machine: StoreApi<string> }) {
  const here = useStore(machine);
  return <><span className="tt"><b><button className="agproj" id="agproj" onClick={openProject}>{project}</button></b><small id="aghere">{here}</small></span><span ref={editHost} id="agedit" className="agedit" /></>;
}

interface SessionHeadProps {
  agent: string; title: string; folder: string; branch: string; cwd: string; machine: string; usage: SessionUsage | null;
  /// The session's machine, harness and id, which the move views follow.
  moveKey: string;
  /// Opens the session's project from its folder name; null outside a project.
  openProject: (() => void) | null;
  saveTitle(value: string): Promise<void>;
}

export function SessionHead({ agent, title, folder, branch, openProject, cwd, machine, usage, moveKey, saveTitle }: SessionHeadProps) {
  return <><HarnessIcon agent={agent} className="dapp dcoderav" /><span className="tt"><SessionTitle title={title} save={fixedTitle(title) ? null : saveTitle} /><small className="aglocation" title={cwd}><span className="agfolder">{openProject ? <button className="agproj" id="agproj" onClick={openProject}>{folder}</button> : <span id="agproj">{folder}</span>}<span id="agbranch">{branch}</span></span><span>@</span><MoveHere moveKey={moveKey} machine={machine} /></small></span><small id="aguse" className="aguse" title="Context used / capacity · session tokens · session spend / remaining credits">{usage && <UsageBits usage={usage} />}</small><span id="agstate"><FieldControl slot="state" /></span><span ref={editHost} id="agedit" className="agedit" /></>;
}

/// A session's usage in its header: a meter of the context, then the tokens and the spend.
function UsageBits({ usage }: { usage: SessionUsage }) {
  const { context, tokens, spend } = usage;
  const fill: CSSProperties & { "--pct": string } = { "--pct": `${Math.min(100, context?.percent ?? 0)}%` };
  return <>{context && <span className="ctx" data-level={context.percent >= 90 ? "full" : context.percent >= 70 ? "high" : undefined}><i style={fill} /><b>{context.percent}%</b><em>of {context.size}</em></span>}{tokens && <span><b>{tokens}</b><em>tokens</em></span>}{spend && <span><b>{spend}</b></span>}</>;
}

/// Redraws the session head with `change`; false when the frame shows no session head.
export function updateSessionHead(change: Partial<Pick<SessionHeadProps, "title" | "folder" | "branch" | "usage" | "machine">>) {
  const owner = current, head = owner?.frame.head;
  if (!owner || !isValidElement<SessionHeadProps>(head) || head.type !== SessionHead) return false;
  if (Object.entries(change).every(([key, value]) => head.props[key as keyof typeof change] === value)) return true;
  owner.frame = { ...owner.frame, head: cloneElement(head, change) };
  draw(owner);
  return true;
}

const editHost = createRef<HTMLSpanElement>();

/// Where the head shows the Open in editor button, while it shows.
export const sessionEditHost = () => editHost.current?.isConnected ? editHost.current : null;

/// The log body the frame shows, a session's or a draft's, while it is in the page.
export function sessionBody(): ChatBody | null {
  const body = current?.frame.body;
  return body?.isConnected ? body : null;
}

/// A tap in the phone's list of commands and steers leaves the field and its keyboard as they are.
function keepPhoneField(event: MouseEvent) {
  if (document.body.classList.contains("mobile")) event.preventDefault();
}

/// The Continue here button of a read-only session Beep can take over: its key and what a click does.
interface ContinueHereProps { target: string; start(): void }

interface SessionFieldProps {
  mode: "native" | "readonly" | "writable"; name: string; banner: string; moveKey: string;
  continueHere: ContinueHereProps | null;
  attach(files: File[]): unknown;
  send(input: HTMLTextAreaElement): void;
  stop(): void;
  openParent(): void;
}

/// The session's message field while the session page shows it.
export const agentMessage = createRef<HTMLTextAreaElement>();
/// The new-agent field while an empty agent view (a draft) shows it.
export const draftMessage = createRef<HTMLTextAreaElement>();

/// The field that takes the keys back from the pane: the session's or the draft's, while shown.
export const shownMessage = () => [agentMessage.current, draftMessage.current].find(field => field?.isConnected) ?? null;

function ContinueHere({ target, start }: ContinueHereProps) {
  const state = useStore(continuing);
  const shown = state.kind !== "none" && state.key === target ? state : null;
  if (shown?.kind === "waiting") return <><Button id="agcontinue" disabled><span className="dspin" />Waiting for the turn to end</Button><Button look="quiet" onClick={cancelContinue}>Cancel</Button></>;
  if (shown?.kind === "closing") return <Button id="agcontinue" disabled><span className="dspin" />Closing it there</Button>;
  return <>{shown?.kind === "failed" && <span className="derr">{shown.message}</span>}<Button id="agcontinue" onClick={start}>Continue here</Button></>;
}

const FIELD_HEIGHT = "beep.fieldHeight";
const keptFieldHeight = (): number => { try { return Math.max(0, Number(localStorage.getItem(FIELD_HEIGHT)) || 0); } catch { return 0; } };

/// The top edge of the message field: dragging it makes the field taller, up to
/// half the conversation, and a double-click returns it to one line. The height
/// is kept for this window.
function FieldGrip() {
  const grip = useRef<HTMLSpanElement>(null), [height, setHeight] = useState(keptFieldHeight);

  useLayoutEffect(() => { grip.current?.parentElement?.style.setProperty("--field-h", `${height}px`); }, [height]);

  const keep = (next: number) => {
    setHeight(next);
    try { localStorage.setItem(FIELD_HEIGHT, String(next)); } catch { /* kept for this launch only */ }
  };
  const drag = (event: ReactPointerEvent<HTMLSpanElement>) => {
    const field = event.currentTarget.parentElement, area = field?.querySelector("textarea");
    if (!field || !area) return;

    event.preventDefault();
    const from = event.clientY, start = area.offsetHeight, limit = Math.round((field.parentElement?.clientHeight ?? window.innerHeight) / 2);
    const move = (e: PointerEvent) => keep(Math.max(0, Math.min(limit, start + from - e.clientY)));
    const done = () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", done); };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", done);
  };

  return <span ref={grip} className="agfield-grip" role="separator" aria-orientation="horizontal" aria-label="Resize the message field" title="Drag to resize, double-click to reset" onPointerDown={drag} onDoubleClick={() => keep(0)} />;
}

export function SessionField({ mode, name, banner, moveKey, continueHere, attach, send, stop, openParent }: SessionFieldProps) {
  const input = agentMessage, dropping = useFieldDrop(), uploading = useUploadPending();
  if (mode === "native") return <div className="dinput small" id="agfield"><textarea id="agnativemsg" rows={1} aria-label="Messages are sent from the parent conversation" disabled placeholder="Open the parent conversation to send a message" /><span id="agmodel"><FieldControl slot="model" /></span><Button id="agnativeparent" onClick={openParent}>Open parent</Button></div>;
  if (mode === "readonly") return continueHere ? <div className="dmuted dbanner dtakeover"><span>{banner}</span><ContinueHere {...continueHere} /></div> : <div className="dmuted dblock dbanner">{banner}</div>;
  return <><MoveBox moveKey={moveKey} /><div id="agplan"><FieldControl slot="plan" /></div><div className="ag-work"><div className="ag-worklist" onMouseDown={keepPhoneField}><div id="agtasks"><FieldControl slot="tasks" /></div><div data-agent-queue-slot style={{ display: "contents" }}><FieldQueue /></div></div><div data-agent-work-line style={{ display: "contents" }}><WorkLine /></div></div><div className={dropping ? "dinput small dropping" : "dinput small"} id="agfield"><FieldGrip /><FilePicker attach={attach} /><div data-agent-attachment-slot style={{ display: "contents" }}><FieldAttachments /></div><textarea ref={input} id="agmsg" rows={1} readOnly={uploading} aria-busy={uploading} style={uploading ? { opacity: 0.5, cursor: "wait" } : undefined} autoComplete="off" spellCheck={false} placeholder={`Message ${name}`} /><span id="agmode"><FieldControl slot="mode" /></span><span id="agmodel"><FieldControl slot="model" /></span><StopButton stop={stop} /><Button id="agsend" aria-label="Send" disabled={uploading} onClick={() => { if (input.current) send(input.current); }}><Icon name="send" /></Button><div data-agent-menu-slot style={{ display: "contents" }}><FieldMenu /></div></div><div id="agpickm" ref={modelPickHost}><FieldControl slot="pick" /></div></>;
}

// A draft and the session it starts share the same dock nodes. Their terminals
// stay mounted while only the header and conversation are replaced.
export function showSessionFrame(frame: Frame, inPlace: boolean, release: () => void) {
  if (!inPlace || !current?.page.host.isConnected) {
    const page = openBarPage(() => { const old = current; current = null; old?.release(); });
    const store = createStore<FrameState>(() => ({ frame, revision: 1, slots: [] }));
    const owner: Owner = { page, store, revision: 1, frame, portals: new Map(), children: new Map(), next: 0, release };
    current = owner;
    flushSync(() => page.show({ view: "session", props: { store } }));
    return;
  }

  // The page it replaces lets go of what it held, as a draft its field.
  const owner = current, previous = owner.release;
  owner.release = release;
  previous();
  owner.frame = frame;
  owner.revision++;
  draw(owner);
}

function BodySlot({ body }: { body: ChatBody }) {
  const host = useRef<HTMLDivElement>(null);
  const scrollbar = useStore(conversationAppearance, state => state.scrollbar);
  useLayoutEffect(() => { const slot = host.current; slot?.prepend(body); return () => body.remove(); }, [body]);
  return <div ref={host} className="agbodyw">{scrollbar && <ConversationRail body={body}/>}</div>;
}

/// The conversation's scrollbar: where the view is, and a tick at each message
/// the user sent. A tick jumps to its message; the thumb drags.
interface RailView { thumb: { top: number; height: number } | null; ticks: { key: string; top: number; prompt: string }[] }
const NO_RAIL: RailView = { thumb: null, ticks: [] };
/// The thumb's least height, in pixels, so a long log keeps one to grab.
const THUMB_MIN = 24;

function ConversationRail({ body }: { body: ChatBody }) {
  const rail = useRef<HTMLDivElement>(null);
  const promptMarksEnabled = useStore(conversationAppearance, state => state.promptMarks);
  const [view, setView] = useState(NO_RAIL);
  const [peek, setPeek] = useState<string | null>(null);

  useEffect(() => {
    let frame = 0;
    const read = () => {
      frame = 0;
      const box = rail.current, total = body.scrollHeight, shown = body.clientHeight;
      if (!box || total <= shown + 1) return setView(NO_RAIL);
      const height = box.clientHeight, scale = height / total;
      const thumbHeight = Math.max(THUMB_MIN, shown * scale);
      const thumbTop = (body.scrollTop / (total - shown)) * (height - thumbHeight);
      const ticks = (promptMarksEnabled ? promptMarks(body) : []).map(mark => ({ key: mark.key, prompt: mark.prompt, top: Math.min(height - 2, mark.top * scale) }));
      setView({ thumb: { top: thumbTop, height: thumbHeight }, ticks });
    };
    const later = () => { frame ||= requestAnimationFrame(read); };
    const sized = new ResizeObserver(later);
    sized.observe(body);
    body.addEventListener("scroll", later, { passive: true });
    const drawn = onLogDrawn(body, later);
    later();
    return () => { cancelAnimationFrame(frame); sized.disconnect(); body.removeEventListener("scroll", later); drawn(); };
  }, [body, promptMarksEnabled]);

  const jump = (key: string) => {
    revealLogUnit(body, key);
    syncJump(body);
  };

  // A press on the track brings that part of the log into view; one on the thumb drags it.
  const press = (event: ReactPointerEvent<HTMLDivElement>) => {
    const box = rail.current, thumb = view.thumb;
    if (!box || !thumb || event.button !== 0 || (event.target as HTMLElement).closest(".tk")) return;
    event.preventDefault();
    const top = box.getBoundingClientRect().top, travel = box.clientHeight - thumb.height;
    const room = body.scrollHeight - body.clientHeight;
    const onThumb = event.clientY - top >= thumb.top && event.clientY - top <= thumb.top + thumb.height;
    const grab = onThumb ? event.clientY - top - thumb.top : thumb.height / 2;
    const follow = (y: number) => {
      body.stuck = false;
      body.scrollTop = travel > 0 ? ((y - top - grab) / travel) * room : 0;
    };
    if (!onThumb) follow(event.clientY);
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);
    const move = (moved: PointerEvent) => follow(moved.clientY);
    const up = () => { target.removeEventListener("pointermove", move); target.removeEventListener("pointerup", up); target.removeEventListener("pointercancel", up); };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
    target.addEventListener("pointercancel", up);
  };

  // The message under the pointer, beside its tick.
  const ticks = promptMarksEnabled ? view.ticks : [];
  const shown = ticks.find(tick => tick.key === peek) ?? null;

  return <div className={view.thumb ? "agrail" : "agrail off"} ref={rail} onPointerDown={press}>
    {view.thumb && <div className="th" style={{ top: view.thumb.top, height: view.thumb.height }}/>}
    {ticks.map(tick => <button type="button" className="tk" key={tick.key} style={{ top: tick.top }} aria-label={`Jump to your message: ${tick.prompt.slice(0, 80)}`}
      onClick={() => jump(tick.key)} onPointerEnter={() => setPeek(tick.key)} onPointerLeave={() => setPeek(null)} onFocus={() => setPeek(tick.key)} onBlur={() => setPeek(null)}/>)}
    {shown && <div className="agpeek" style={{ top: shown.top }} aria-hidden="true">{shown.prompt}</div>}
  </div>;
}

const sameHints = (a: AgentHintsModel, b: AgentHintsModel) => a.project === b.project && a.value.window === b.value.window
  && a.value.items.length === b.value.items.length && a.value.items.every((item, i) => item.join("\n") === b.value.items[i].join("\n"));

export function updateSessionHints(hints: AgentHintsModel) {
  if (!current || sameHints(current.frame.hints, hints)) return;
  current.frame = { ...current.frame, hints };
  draw(current);
}

const AgentHints = memo(function AgentHints({ value }: { value: AgentHintsModel }) {
  const sections = SHORTCUTS.filter(section => ["Agent session", "Side pane", "Terminals"].includes(section.title));
  return <div className="agkeys"><HintView value={value.value} slim /><StashIndicator project={value.project} /><details id="agshortcuts"><summary>Show more</summary><div className="agkeys-list">{sections.map(section => <section key={section.title}><b>{section.title}</b>{section.rows.map(row => <span key={`${row.keys}:${row.does}`}><kbd>{row.keys}</kbd>{row.does}</span>)}</section>)}<StashActions /></div></details></div>;
});

const FrameView = memo(function FrameView({ frame, revision }: { frame: Frame; revision: number }) {
  return <><div className="dhead ag-head" data-tauri-drag-region="" key={`head:${revision}`}>{frame.head}</div><PaneShell><PaneMain><div className="agconv" id="agconv" key={`conversation:${revision}`}><BodySlot body={frame.body} />{frame.field}<AgentHints value={frame.hints} /></div><PaneDock dock="bottom" /></PaneMain><PaneDock dock="right" /><PaneDropZone /></PaneShell></>;
});
