import { useDialogRef } from "../ui/parts.tsx";
import { createRef, isValidElement, useLayoutEffect, useRef, useState, useEffect, lazy, Suspense, type MouseEvent, type ReactElement, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { createStore, useStore } from "zustand";
import { AgentQueue } from "./queue-view.tsx";
import { AgentStopFace } from "./mode-view.tsx";
import type { OutboxRow, Waiting } from "./agentqueue.ts";
import type { Row } from "./mention-state.ts";
import { Button } from "../ui/kit.tsx";
import { Icon } from "../ui/icons.tsx";
import { plural } from "../ui/format.ts";
import { openViewer } from "../chat/media.tsx";

// What the session's composer shows around its field: the queued steers,
// attachments, the mention menu, the background work line, the pills and
// the head's state. Session code sets it here; the session frame draws it.

/// A control the session sets in its head or composer.
type FieldSlot = "state" | "mode" | "model" | "plan" | "tasks" | "pick";

/// Queued steers fold into one line above the field on a phone; the line opens their
/// list over the log.
interface Work { queued: number; open: boolean }
interface Queue {
  items: readonly Waiting[];
  edit(index: number, keep: boolean): unknown;
  /// Messages waiting on this device, after those the node holds.
  outbox: readonly OutboxRow[];
  editOutbox(id: number, keep: boolean): unknown;
  signature: string;
}
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
  uploading: boolean;
}

const EMPTY: FieldUi = { slots: {}, signatures: {}, queue: null, attachments: null, menu: null, work: { queued: 0, open: false }, stop: { shown: false, stopping: false }, dropping: false, uploading: false };
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

export function showAgentQueue(items: readonly Waiting[], edit: Queue["edit"], outbox: readonly OutboxRow[], editOutbox: Queue["editOutbox"]) {
  const state = fieldUi.getState(), queued = items.length + outbox.length;

  if (!queued) {
    if (state.queue) set({ queue: null, work: workOf(state.work, { queued: 0 }) });
    return;
  }

  const signature = JSON.stringify([items.map(item => [item.typed, item.text, item.images.map(image => image.name), item.paths, item.state]), outbox]);
  if (state.queue?.signature === signature) return;
  set({ queue: { items, edit, outbox, editOutbox, signature }, work: workOf(state.work, { queued }) });
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
export const showUploadPending = (uploading: boolean) => set({ uploading });
export const useUploadPending = () => useStore(fieldUi, state => state.uploading);

export const useFieldDrop = () => useStore(fieldUi, state => state.dropping);

function workOf(work: Work, counts: Pick<Work, "queued">) {
  const next = { ...work, ...counts };
  if (!next.queued) next.open = false;
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
  return queue && <div className="ag-queue" id="agqueue"><AgentQueue items={queue.items} edit={queue.edit} outbox={queue.outbox} editOutbox={queue.editOutbox} /></div>;
}

export function FieldAttachments() {
  const view = useStore(fieldUi, state => state.attachments);
  return view && <div className="ag-att" id="agatt">{view}</div>;
}

export function WorkLine() {
  const { queued, open } = useStore(fieldUi, state => state.work);
  if (!queued) return null;

  const toggle = () => fieldUi.setState(state => ({ work: { ...state.work, open: !state.work.open } }));
  // Keeps the field focused, so the keyboard and the composer stay where they are.
  return <button type="button" className="ag-workline" aria-expanded={open} onMouseDown={event => event.preventDefault()} onClick={toggle}><i /><span>{queued} queued</span><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d={open ? "M6 9l6 6 6-6" : "M6 15l6-6 6 6"} /></svg></button>;
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
  return <Button id="agstop" aria-label="Stop the turn" title="Stop main turn (subagents keep running)" style={shown ? undefined : { display: "none" }} disabled={stopping} onClick={stop}><AgentStopFace stopping={stopping} /></Button>;
}

const AttachmentSheet = lazy(async () => {
  const { ModalOverlay, Modal, Dialog } = await import("react-aria-components");

  return { default: function AttachmentDialog({ onClose, children }: { onClose(): void; children: ReactNode }) {
    return <ModalOverlay className="am-scrim" isOpen isDismissable onOpenChange={next => { if (!next) onClose(); }}>
      <Modal><Dialog className="attachment-window" aria-label="Attachments">{children}</Dialog></Modal>
    </ModalOverlay>;
  } };
});

/// The x on an attachment chip; `data` names the data attribute that holds `index`.
export function RemoveChip({ name, data, index, remove }: { name: string; data: string; index: number; remove(): void }) {
  return <button type="button" aria-label={`Remove ${name}`} {...{ [data]: index }} onClick={remove}><Icon name="x" /></button>;
}

/// The input bar keeps one attachment control; its window shows every file.
export function AttachmentBatch({ count, children }: { count: number; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const dialog = useDialogRef<HTMLDivElement>();
  const trigger = useRef<HTMLButtonElement>(null), close = useRef<HTMLButtonElement>(null);
  const dismiss = () => { setOpen(false); requestAnimationFrame(() => trigger.current?.focus()); };
  useEffect(() => { if (!count) setOpen(false); }, [count]);

  if (!count) return null;

  return <><Button ref={trigger} className="attachment-count" onMouseDown={event => event.preventDefault()} aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen(true)}>{plural(count, "file", "files")}</Button>
    {open && <Suspense fallback={null}><AttachmentSheet onClose={dismiss}>
      <header><b>{plural(count, "attachment", "attachments")}</b><Button ref={close} look="quiet" aria-label="Close attachments" onClick={dismiss}>Close</Button></header>
      <div ref={dialog} className="ag-att attachment-gallery" onClick={event => {
        if (!event.currentTarget.closest(".attachment-window")?.contains(document.activeElement)) close.current?.focus();
      }}>{children}</div>
    </AttachmentSheet></Suspense>}</>;
}

/// The chip of an image: its name and a preview that opens `images` at `index`.
/// `listed` marks one of an indexed batch (expandable, drawn at once); a lone
/// file's preview is drawn lazily.
export function ImagePreview({ images, index, listed }: { images: { url: string; name: string }[]; index: number; listed: boolean }) {
  const { url, name } = images[index];
  const open = (event: MouseEvent<HTMLButtonElement>) => openViewer(images.map(a => ({ url: a.url, name: a.name, saves: true })), index, event.currentTarget.closest<HTMLElement>(".attachment-window") ?? undefined);

  return listed
    ? <button type="button" className="preview" data-att-preview={index} aria-label={`Preview ${name}`} title="Expand image" onClick={open}><img src={url} alt="" /><span className="nm">{name}</span></button>
    : <button type="button" className="preview" aria-label={`Preview ${name}`} onClick={open}><img src={url} alt="" loading="lazy" /><span className="nm">{name}</span></button>;
}
