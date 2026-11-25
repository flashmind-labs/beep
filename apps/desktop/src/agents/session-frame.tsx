import type { HintViewModel } from "../bar/hints-view.tsx";
import { readWindowPreference, writeWindowPreference } from "../ui/window-layout.ts";
import { appendQuote, ComposeField } from "../chat/compose.tsx";
import { DictateButton } from "../chat/dictation.tsx";
import { FilePicker } from "./file-picker.tsx";
import { cloneElement, createRef, isValidElement, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type MouseEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { createStore, useStore, type StoreApi } from "zustand";
import { SessionTitle } from "./title-view.tsx";
import { fixedTitle } from "../project/projectbar.ts";
import { createPortal, flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-page.ts";
import type { ChatBody } from "../ui/turn.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { RunningPages } from "../agentpane/running-view.tsx";
import { MoveBar, MoveBox, MoveHere } from "./move-view.tsx";
import { FieldAttachments, FieldControl, FieldMenu, FieldQueue, modelPickHost, StopButton, useFieldDrop, WorkLine } from "./composer-ui.tsx";
import { cancelContinue, continuing } from "./continue-here.ts";
import { ComposerPresence, PluginSlot, SessionHeaderSlot } from "../plugins/slot-view.tsx";
import { Button, Spinner } from "../ui/kit.tsx";
import type { ProjectWindowProps } from "../project/projectwindow.ts";
import type { Persona, SessionUsage } from "./agents.ts";
import { PersonaTag } from "../project/customagents-view.tsx";

export interface AgentHintsModel { value: HintViewModel; project: string | null }
export interface Frame { head: ReactNode; field: ReactNode; body: ChatBody; hints: AgentHintsModel }
/// What the session page draws: the frame, and the views put in it.
export interface FrameState { frame: Frame; revision: number; slots: readonly Slot[] }
interface Owner { page: BarPageSlot; store: StoreApi<FrameState>; revision: number; frame: Frame; portals: Map<HTMLElement, Slot>; children: Map<string, Slot>; next: number; release(): void }
let current: Owner | null = null;

// Each child and portal is its own store, so a pane redrawing one tab strip
// renders that strip alone instead of the whole session frame.
export interface Slot { key: number; host: HTMLElement | null; view: StoreApi<ReactNode> }

function slot(owner: Owner, host: HTMLElement | null, view: ReactNode): Slot {
  return { key: ++owner.next, host, view: createStore<ReactNode>(() => view) };
}

function draw(owner: Owner) {
  // Something replaced the bar's contents: the page box mounts again.
  if (!owner.page.host.isConnected) owner.page.show({ view: "session", props: { store: owner.store } });
  flushSync(() => owner.store.setState({ frame: owner.frame, revision: owner.revision, slots: [...owner.children.values(), ...owner.portals.values()] }));
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

export function DraftHead({ project, openProject, machine, windowProps = null }: { windowProps?: ProjectWindowProps | null; project: string; openProject(): void; machine: StoreApi<string> }) {
  const here = useStore(machine);
  return <><span className="tt"><b><Button look="plain" className="agproj" id="agproj" {...windowProps?.clicks} {...windowProps?.menu} onClick={openProject}>{project}</Button></b><small id="aghere">{here}</small></span><span ref={editHost} id="agedit" className="agedit" /></>;
}

interface SessionHeadProps {
  agent: string; title: string; folder: string; branch: string; cwd: string; machine: string; usage: SessionUsage | null;
  /// The custom agent it runs as.
  persona: Persona | null;
  /// The session's machine id and its id, for plugins.
  machineId: string; session: string;
  /// The session's machine, harness and id, which the move views follow.
  moveKey: string;
  /// Opens the session's project from its folder name; null outside a project.
  openProject: (() => void) | null;
  windowProps?: ProjectWindowProps | null;
  /// Saves the title on the session's machine; null for a start, which no machine has yet.
  saveTitle: ((value: string) => Promise<void>) | null;
}

export function SessionHead({ agent, title, folder, branch, persona, openProject, windowProps = null, cwd, machine, usage, moveKey, saveTitle, machineId, session }: SessionHeadProps) {
  return <><HarnessIcon agent={agent} className="dapp dcoderav" /><span className="tt"><SessionTitle title={title} save={fixedTitle(title) ? null : saveTitle} /><small className="aglocation" title={cwd}><span className="agfolder">{openProject ? <Button look="plain" className="agproj" id="agproj" {...windowProps?.clicks} {...windowProps?.menu} onClick={openProject}>{folder}</Button> : <span id="agproj">{folder}</span>}<span id="agbranch">{branch}</span></span><span>@</span><MoveHere moveKey={moveKey} machine={machine} /><PersonaTag persona={persona} /></small></span><SessionHeaderSlot machine={machineId} session={session} /><small id="aguse" className="aguse" title="Context used / capacity · session tokens · session spend / remaining credits">{usage && <UsageBits usage={usage} />}</small><span id="agstate"><FieldControl slot="state" /></span><span ref={editHost} id="agedit" className="agedit" /></>;
}

/// A session's usage in its header: a meter of the context, then the tokens and the spend.
function UsageBits({ usage }: { usage: SessionUsage }) {
  const { context, tokens, spend } = usage;
  const fill: CSSProperties & { "--pct": string } = { "--pct": `${Math.min(100, context?.percent ?? 0)}%` };
  return <>{context && <span className="ctx" data-level={context.percent >= 90 ? "full" : context.percent >= 70 ? "high" : undefined}><i style={fill} /><b>{context.percent}%</b><em>of {context.size}</em></span>}{tokens && <span><b>{tokens}</b><em>tokens</em></span>}{spend && <span><b>{spend}</b></span>}</>;
}

/// Redraws the session head with `change`; false when the frame shows no session head.
export function updateSessionHead(change: Partial<Pick<SessionHeadProps, "title" | "folder" | "branch" | "usage" | "machine" | "persona">>) {
  const owner = current, head = owner?.frame.head;
  if (!owner || !isValidElement<SessionHeadProps>(head) || head.type !== SessionHead) return false;
  const unchanged = Object.entries(change).every(([key, value]) => key === "usage" || key === "persona"
    ? JSON.stringify(head.props[key]) === JSON.stringify(value)
    : head.props[key as keyof typeof change] === value);
  if (unchanged) return true;
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
  mode: "native" | "readonly" | "starting" | "writable"; name: string; banner: string; moveKey: string;
  /// The session this field belongs to, for plugins.
  machine: string; session: string;
  continueHere: ContinueHereProps | null;
  attach(files: File[]): unknown;
  /// The key the field's draft is kept under; another conversation's field starts afresh.
  draft: string;
  removeLast(): void;
  send(input: HTMLTextAreaElement): void;
  stop(): void;
  openParent(): void;
  /// Runs a start that failed again; null while it waits or starts.
  retry: (() => void) | null;
  /// Drops a start and leaves its view.
  cancel(): void;
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
  if (shown?.kind === "waiting") return <><Button id="agcontinue" disabled><Spinner />Waiting for the turn to end</Button><Button look="quiet" onClick={cancelContinue}>Cancel</Button></>;
  if (shown?.kind === "closing") return <Button id="agcontinue" disabled><Spinner />Closing it there</Button>;
  return <>{shown?.kind === "failed" && <span className="derr">{shown.message}</span>}<Button id="agcontinue" onClick={start}>Continue here</Button></>;
}

const FIELD_HEIGHT = "beep.fieldHeight";
function keptFieldHeight(key: string): number {
  try {
    const height = Number(readWindowPreference(key) ?? readWindowPreference(FIELD_HEIGHT));
    return Number.isFinite(height) ? Math.max(0, height) : 0;
  } catch { return 0; }
}

/// The top edge of the message field: dragging it makes the field taller, up to
/// half the conversation, and a double-click returns it to one line. The height
/// is kept for this window.
function FieldGrip({ preference }: { preference: string }) {
  const grip = useRef<HTMLSpanElement>(null), [height, setHeight] = useState(() => keptFieldHeight(preference));

  useLayoutEffect(() => { grip.current?.parentElement?.style.setProperty("--field-h", `${height}px`); }, [height]);

  const keep = (next: number) => {
    setHeight(next);
    try { writeWindowPreference(preference, String(next)); } catch { /* kept for this launch only */ }
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

export function SessionField({ mode, name, banner, moveKey, machine, session, continueHere, attach, draft, removeLast, send, stop, openParent, retry, cancel }: SessionFieldProps) {
  const fieldPreference = `${FIELD_HEIGHT}:${draft}`;
  const input = agentMessage, dropping = useFieldDrop(), box = useRef<HTMLDivElement>(null);
  if (mode === "native") return <div className="dinput small" id="agfield"><textarea id="agnativemsg" rows={1} aria-label="Messages are sent from the parent conversation" disabled placeholder="Open the parent conversation to send a message" /><span id="agmodel"><FieldControl slot="model" /></span><Button id="agnativeparent" onClick={openParent}>Open parent</Button></div>;
  if (mode === "starting") return <div className="dmuted dbanner dtakeover"><span>{banner}</span>{retry && <Button onClick={retry}>Retry</Button>}<Button look="quiet" onClick={cancel}>Cancel</Button></div>;
  if (mode === "readonly") return continueHere ? <div className="dmuted dbanner dtakeover"><span>{banner}</span><ContinueHere {...continueHere} /></div> : <div className="dmuted dblock dbanner">{banner}</div>;
  return <><MoveBox moveKey={moveKey} /><div id="agplan"><FieldControl slot="plan" /></div><div className="ag-work"><div className="ag-worklist" onMouseDown={keepPhoneField}><FieldControl slot="steps" /><div id="agtasks"><FieldControl slot="tasks" /></div><RunningPages /><div data-agent-queue-slot style={{ display: "contents" }}><FieldQueue /></div></div><div data-agent-work-line style={{ display: "contents" }}><WorkLine /></div></div><ComposerPresence machine={machine} session={session} /><PluginSlot name="composer" /><QuoteSelection /><div ref={box} className={dropping ? "dinput small dropping" : "dinput small"} id="agfield"><MoveBar moveKey={moveKey} /><FieldGrip key={fieldPreference} preference={fieldPreference} /><FilePicker attach={attach} /><div data-agent-attachment-slot style={{ display: "contents" }}><FieldAttachments /></div><ComposeField key={draft} ref={input} draft={draft} images={attach} removeLast={removeLast} grow={156} id="agmsg" rows={1} placeholder={`Message ${name}`} /><span id="agmode"><FieldControl slot="mode" /></span><span id="agmodel"><FieldControl slot="model" /></span><DictateButton field={input} host={box} /><StopButton stop={stop} /><Button id="agsend" aria-label="Send" onClick={() => { if (input.current) send(input.current); }}><Icon name="send" /></Button><div data-agent-menu-slot style={{ display: "contents" }}><FieldMenu /></div></div><div id="agpickm" ref={modelPickHost}><FieldControl slot="pick" /></div></>;
}

/// The range selected inside the open conversation when it has text and lies
/// wholly within a single agent reply.
function selectedAgentRange(selection: Selection | null): Range | null {
  if (!selection || selection.isCollapsed || !selection.rangeCount) return null;

  const range = selection.getRangeAt(0);
  if (!range.toString().trim()) return null;

  const body = sessionBody();
  const replyOf = (node: Node) => {
    const el = node instanceof Element ? node : node.parentElement;
    if (!el || el.closest("button, summary, [role='button']")) return null;
    return el.closest<HTMLElement>(".dcoderen.agent");
  };
  const start = replyOf(range.startContainer);
  if (!start || start !== replyOf(range.endContainer) || (body && !body.contains(start))) return null;
  return range;
}

function quoteSelectedReply(): boolean {
  const field = agentMessage.current;
  if (!field || !field.isConnected || field.readOnly) return false;

  const selection = getSelection(), range = selectedAgentRange(selection);
  const text = range?.toString() ?? "";
  if (!text.trim()) return false;

  // Clear the DOM selection before focusing the textarea: in WebKit,
  // removeAllRanges() after focus blurs the textarea's caret.
  selection?.removeAllRanges();
  return appendQuote(field, text);
}

/// Floating Quote button over a selection in an agent reply; Q while shown.
function QuoteSelection() {
  const [rect, setRect] = useState<DOMRect | null>(null);

  useEffect(() => {
    const update = () => {
      const field = agentMessage.current;
      if (!field || !field.isConnected || field.readOnly) { setRect(null); return; }

      const range = selectedAgentRange(getSelection());
      const box = range?.getBoundingClientRect() ?? null;
      const body = sessionBody()?.getBoundingClientRect();
      const visible = box && (box.width > 0 || box.height > 0)
        && (!body || (box.bottom > body.top && box.top < body.bottom));
      setRect(visible ? box : null);
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "q" || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return;
      const target = event.target instanceof HTMLElement ? event.target : null;
      if (target && target !== agentMessage.current && target.closest("input, textarea, select, [contenteditable='true']")) return;
      if (quoteSelectedReply()) event.preventDefault();
    };

    document.addEventListener("selectionchange", update);
    window.addEventListener("scroll", update, { capture: true, passive: true });
    window.addEventListener("resize", update);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("selectionchange", update);
      window.removeEventListener("scroll", update, { capture: true });
      window.removeEventListener("resize", update);
      document.removeEventListener("keydown", onKey);
    };
  }, []);

  if (!rect) return null;

  const left = Math.round(Math.min(Math.max(8, rect.left + rect.width / 2 - 38), window.innerWidth - 84));
  const below = rect.bottom + 6;
  const top = Math.round(below + 34 <= window.innerHeight ? below : Math.max(8, rect.top - 36));
  return createPortal(
    <div className="agquote-pop" role="toolbar" aria-label="Selection actions" style={{ left, top }} onMouseDown={e => e.preventDefault()}>
      <Button small look="quiet" onClick={() => { quoteSelectedReply(); }} title="Quote selection in message (Q)">Quote<kbd>Q</kbd></Button>
    </div>,
    document.body,
  );
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

const sameHints = (a: AgentHintsModel, b: AgentHintsModel) => a.project === b.project && a.value.window === b.value.window
  && a.value.items.length === b.value.items.length && a.value.items.every((item, i) => item.join("\n") === b.value.items[i].join("\n"));

export function updateSessionHints(hints: AgentHintsModel) {
  if (!current || sameHints(current.frame.hints, hints)) return;
  current.frame = { ...current.frame, hints };
  draw(current);
}
