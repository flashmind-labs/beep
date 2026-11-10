import { createElement, Fragment, isValidElement, useLayoutEffect, useRef, type ReactElement } from "react";
import { createPortal, flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { AgentQueue } from "./queue-view.tsx";
import type { Waiting } from "./agentqueue.ts";
import type { Row } from "./mention-state.ts";

interface Owner { field: HTMLElement; root: DocumentView; queue: HTMLElement | null; queueSignature: string | null; attachments: HTMLElement | null; menu: HTMLElement | null; queueView: ReactElement | null; attachmentView: ReactElement | null; menuView: ReactElement | null; work: Work; controls: Map<HTMLElement, { key: number; node: ReactElement | null }>; next: number; observer: MutationObserver }
/// Background commands and queued steers fold into one line above the field on a phone;
/// the line opens their list over the log.
interface Work { host: HTMLElement | null; running: number; queued: number; open: boolean }
// One owner keeps all session composer portals alive until the field leaves.
// Queue, attachment and menu hosts can come and go without replacing controls.
let current: Owner | null = null;
function draw(owner: Owner) {
  flushSync(() => owner.root.render(createElement(Fragment, null, owner.queue && createPortal(owner.queueView, owner.queue, "queue"), owner.attachments && createPortal(owner.attachmentView, owner.attachments, "attachments"), owner.menu && createPortal(owner.menuView, owner.menu, "menu"), owner.work.host && createPortal(workLine(owner), owner.work.host, "work"), ...[...owner.controls].map(([host, control]) => createPortal(control.node, host, control.key)))));
}
function ownerOf(field: HTMLElement): Owner {
  if (current?.field === field) return current;
  disposeComposerUi();
  const root = createDocumentView(document.createElement("div"));
  const observer = new MutationObserver(() => { if (current?.field === field && !field.isConnected) disposeComposerUi(); });
  observer.observe(document.body, { childList: true, subtree: true });
  current = { field, root, queue: null, queueSignature: null, attachments: null, menu: null, queueView: null, attachmentView: null, menuView: null, work: { host: null, running: 0, queued: 0, open: false }, controls: new Map(), next: 0, observer };
  return current;
}
export function disposeComposerUi() {
  const owner = current; current = null;
  if (!owner) return;
  owner.observer.disconnect(); owner.root.unmount();
}
export function showAgentAttachments(field: HTMLElement, view: ReactElement | null) {
  if (!view && (!current || current.field !== field || !current.attachments)) return;
  const owner = ownerOf(field);
  if (!view) {
    owner.attachments = null; owner.attachmentView = null; draw(owner);
    return;
  }
  owner.attachments ??= field.parentElement?.querySelector<HTMLElement>("[data-agent-attachment-slot]") ?? null;
  if (!owner.attachments) return;
  owner.attachmentView = createElement("div", { className: "ag-att", id: "agatt" }, view); draw(owner);
}
export function showAgentQueue(field: HTMLElement, items: readonly Waiting[]) {
  if (!items.length) {
    if (!current || current.field !== field || !current.queue) return;
    const owner = current; owner.queue = null; owner.queueSignature = null; owner.queueView = null;
    countWork(owner, { queued: 0 }); draw(owner);
    return;
  }
  const owner = ownerOf(field);
  const signature = JSON.stringify(items.map(item => [item.typed, item.text, item.images.map(image => image.name), item.paths, item.state]));
  if (owner.queue && owner.queueSignature === signature) return;
  owner.queue ??= field.parentElement?.querySelector<HTMLElement>("[data-agent-queue-slot]") ?? null;
  if (!owner.queue) return;
  owner.queueSignature = signature;
  owner.queueView = createElement("div", { className: "ag-queue", id: "agqueue" }, createElement(AgentQueue, { items }));
  countWork(owner, { queued: items.length }); draw(owner);
}

/// The background commands in `host`, `running` of them.
export function showAgentTasks(field: HTMLElement, host: HTMLElement, node: ReactElement | null, running: number) {
  const owner = ownerOf(field);
  if (owner.work.running !== running) { countWork(owner, { running }); draw(owner); }
  showAgentControl(field, host, node);
}

function countWork(owner: Owner, counts: Partial<Pick<Work, "running" | "queued">>) {
  const work = { ...owner.work, ...counts };
  work.host ??= owner.field.parentElement?.querySelector<HTMLElement>("[data-agent-work-line]") ?? null;
  if (!work.running && !work.queued) work.open = false;
  owner.work = work;
}

function workLine(owner: Owner) {
  const { running, queued, open } = owner.work;
  if (!running && !queued) return null;

  const toggle = () => { owner.work = { ...owner.work, open: !owner.work.open }; draw(owner); };
  const text = [running ? `${running} running` : "", queued ? `${queued} queued` : ""].filter(Boolean).join(", ");
  // Keeps the field focused, so the keyboard and the composer stay where they are.
  return <button type="button" className="ag-workline" aria-expanded={open} onMouseDown={event => event.preventDefault()} onClick={toggle}><i /><span>{text}</span><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d={open ? "M6 9l6 6 6-6" : "M6 15l6-6 6 6"} /></svg></button>;
}

function Menu({ rows, index, pick }: { rows: Row[]; index: number; pick(index: number): void }) {
  const selected = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { selected.current?.scrollIntoView({ block: "nearest" }); }, [index]);
  return <>{rows.map((row, i) => <div key={`${row.insert}:${i}`} ref={i === index ? selected : null} className={`cm-row${i === index ? " on" : ""}`} role="option" aria-selected={i === index} data-i={i} onMouseDown={event => { event.preventDefault(); pick(i); }}><span className="n">{row.label}</span><span className="d">{row.detail}</span>{row.hint && <small>{row.hint}</small>}</div>)}</>;
}
export function showAgentMenu(field: HTMLElement, rows: Row[], index: number, pick: (index: number) => void) {
  const owner = ownerOf(field);
  owner.menu ??= field.querySelector<HTMLElement>("[data-agent-menu-slot]");
  if (!owner.menu) throw new Error("Agent menu slot is missing");
  owner.menuView = createElement("div", { className: "cm-menu", role: "listbox", hidden: !rows.length }, rows.length ? createElement(Menu, { rows, index, pick }) : null);
  draw(owner);
  const menu = owner.menu.querySelector<HTMLElement>(".cm-menu");
  if (!menu) throw new Error("Agent menu did not mount");
  return menu;
}

export function showAgentControl(field: HTMLElement, host: HTMLElement, node: ReactElement | null) {
  const owner = ownerOf(field), prior = owner.controls.get(host);
  if (prior && sameControl(prior.node, node)) return;
  if (!prior) host.replaceChildren();
  owner.controls.set(host, { key: prior?.key ?? ++owner.next, node });
  draw(owner);
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
