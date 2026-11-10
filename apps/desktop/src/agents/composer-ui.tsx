import { createRef, isValidElement, useLayoutEffect, useRef, type ReactElement } from "react";
import { flushSync } from "react-dom";
import { createStore, useStore } from "zustand";
import { AgentQueue } from "./queue-view.tsx";
import { AgentStopFace } from "./mode-view.tsx";
import type { Waiting } from "./agentqueue.ts";
import type { Row } from "./mention-state.ts";

// What the session's composer shows around its field: the queued steers,
// attachments, the mention menu, the background work line, the pills and
// the head's state. Session code sets it here; the session frame draws it.

/// A control the session sets in its head or composer.
export type FieldSlot = "state" | "mode" | "model" | "plan" | "tasks" | "pick";

/// Background commands and queued steers fold into one line above the field on a phone;
/// the line opens their list over the log.
interface Work { running: number; queued: number; open: boolean }
interface Queue { items: readonly Waiting[]; edit(index: number, keep: boolean): unknown; signature: string }
interface Menu { rows: Row[]; index: number; pick(index: number): void }
/// Only a turn that runs can be stopped; a stop shows at once.
interface Stop { shown: boolean; stopping: boolean }
interface FieldUi {
  slots: Partial<Record<FieldSlot, ReactElement | null>>;
  /// What each slot was last drawn from, when its caller says.
  signatures: Partial<Record<FieldSlot, string>>;
  queue: Queue | null;
  attachments: ReactElement | null;
  menu: Menu | null;
  work: Work;
  stop: Stop;
  /// A path from the Files tab is dragged over the conversation.
  dropping: boolean;
}

const EMPTY: FieldUi = { slots: {}, signatures: {}, queue: null, attachments: null, menu: null, work: { running: 0, queued: 0, open: false }, stop: { shown: false, stopping: false }, dropping: false };
const fieldUi = createStore<FieldUi>(() => EMPTY);
/// Callers read the composer right after they change it, as when focusing
/// what they drew, so the views update at once.
const set = (patch: Partial<FieldUi> | ((state: FieldUi) => Partial<FieldUi>)) => flushSync(() => fieldUi.setState(patch));

/// The session's model picker host, which places itself by the model pill.
export const modelPickHost = createRef<HTMLDivElement>();

/// Forgets what the composer showed, as when its session leaves the page.
export function disposeComposerUi() { flushSync(() => fieldUi.setState(EMPTY, true)); }

export function showAgentControl(slot: FieldSlot, node: ReactElement | null) {
  const prior = fieldUi.getState().slots[slot] ?? null;
  if (sameControl(prior, node)) return;
  set(state => ({ slots: { ...state.slots, [slot]: node } }));
}

/// Draws `node` in `slot` unless it shows what `signature` says already.
export function showAgentControlFor(slot: FieldSlot, signature: string, node: () => ReactElement) {
  const state = fieldUi.getState();
  if (state.signatures[slot] === signature && state.slots[slot]) return;
  set({ slots: { ...state.slots, [slot]: node() }, signatures: { ...state.signatures, [slot]: signature } });
}

export function showAgentAttachments(view: ReactElement | null) {
  if (fieldUi.getState().attachments !== view) set({ attachments: view });
}

export function showAgentQueue(items: readonly Waiting[], edit: (index: number, keep: boolean) => unknown) {
  const state = fieldUi.getState();

  if (!items.length) {
    if (state.queue) set({ queue: null, work: workOf(state.work, { queued: 0 }) });
    return;
  }

  const signature = JSON.stringify(items.map(item => [item.typed, item.text, item.images.map(image => image.name), item.paths, item.state]));
  if (state.queue?.signature === signature) return;
  set({ queue: { items, edit, signature }, work: workOf(state.work, { queued: items.length }) });
}

/// The background commands, `running` of them.
export function showAgentTasks(node: ReactElement | null, running: number) {
  const work = fieldUi.getState().work;
  if (work.running !== running) set({ work: workOf(work, { running }) });
  showAgentControl("tasks", node);
}

export function showAgentMenu(rows: Row[], index: number, pick: (index: number) => void) {
  set({ menu: { rows, index, pick } });
}

export function showAgentStop(stop: Stop) {
  const prior = fieldUi.getState().stop;
  if (prior.shown !== stop.shown || prior.stopping !== stop.stopping) set({ stop });
}

export function showFieldDrop(dropping: boolean) {
  if (fieldUi.getState().dropping !== dropping) fieldUi.setState({ dropping });
}

/// Whether the field shows a path drop.
export const useFieldDrop = () => useStore(fieldUi, state => state.dropping);

function workOf(work: Work, counts: Partial<Pick<Work, "running" | "queued">>) {
  const next = { ...work, ...counts };
  if (!next.running && !next.queued) next.open = false;
  return next;
}

function sameControl(a: ReactElement | null, b: ReactElement | null) {
  if (a === b) return true;
  if (!isValidElement<Record<string, unknown>>(a) || !isValidElement<Record<string, unknown>>(b) || a.type !== b.type || a.key !== b.key) return false;
  const keys = Object.keys(a.props);
  return keys.length === Object.keys(b.props).length && keys.every(key => {
    const value = a.props[key];
    return Object.hasOwn(b.props, key) && (value === null || !["object", "function"].includes(typeof value)) && Object.is(value, b.props[key]);
  });
}

// ----- Views -----

export function FieldControl({ slot }: { slot: FieldSlot }) {
  return useStore(fieldUi, state => state.slots[slot] ?? null);
}

export function FieldQueue() {
  const queue = useStore(fieldUi, state => state.queue);
  return queue && <div className="ag-queue" id="agqueue"><AgentQueue items={queue.items} edit={queue.edit} /></div>;
}

export function FieldAttachments() {
  const view = useStore(fieldUi, state => state.attachments);
  return view && <div className="ag-att" id="agatt">{view}</div>;
}

export function WorkLine() {
  const { running, queued, open } = useStore(fieldUi, state => state.work);
  if (!running && !queued) return null;

  const toggle = () => fieldUi.setState(state => ({ work: { ...state.work, open: !state.work.open } }));
  const text = [running ? `${running} running` : "", queued ? `${queued} queued` : ""].filter(Boolean).join(", ");
  // Keeps the field focused, so the keyboard and the composer stay where they are.
  return <button type="button" className="ag-workline" aria-expanded={open} onMouseDown={event => event.preventDefault()} onClick={toggle}><i /><span>{text}</span><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d={open ? "M6 9l6 6 6-6" : "M6 15l6-6 6 6"} /></svg></button>;
}

export function FieldMenu() {
  const menu = useStore(fieldUi, state => state.menu);
  if (!menu) return null;
  return <div className="cm-menu" role="listbox" hidden={!menu.rows.length}>{menu.rows.length > 0 && <MenuRows {...menu} />}</div>;
}

function MenuRows({ rows, index, pick }: Menu) {
  const selected = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { selected.current?.scrollIntoView({ block: "nearest" }); }, [index]);
  return <>{rows.map((row, i) => <div key={`${row.insert}:${i}`} ref={i === index ? selected : null} className={`cm-row${i === index ? " on" : ""}`} role="option" aria-selected={i === index} data-i={i} onMouseDown={event => { event.preventDefault(); pick(i); }}><span className="n">{row.label}</span><span className="d">{row.detail}</span>{row.hint && <small>{row.hint}</small>}</div>)}</>;
}

export function StopButton({ stop }: { stop(): void }) {
  const { shown, stopping } = useStore(fieldUi, state => state.stop);
  return <button className="dbtn" id="agstop" aria-label="Stop the turn" title="Stop the turn (⌘.)" style={shown ? undefined : { display: "none" }} disabled={stopping} onClick={stop}><AgentStopFace stopping={stopping} /></button>;
}
