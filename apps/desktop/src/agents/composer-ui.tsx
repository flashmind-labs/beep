import { createElement, Fragment, useLayoutEffect, useRef, type ReactElement } from "react";
import { createPortal, flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { AgentQueue } from "./queue-view.tsx";
import type { Waiting } from "./agentqueue.ts";
import type { Row } from "./mention-state.ts";

interface Owner { field: HTMLElement; root: DocumentView; queue: HTMLElement | null; attachments: HTMLElement | null; menu: HTMLElement | null; queueView: ReactElement | null; attachmentView: ReactElement | null; menuView: ReactElement | null; controls: Map<HTMLElement, { key: number; node: ReactElement | null }>; next: number; observer: MutationObserver }
// One owner keeps all session composer portals alive until the field leaves.
// Queue, attachment and menu hosts can come and go without replacing controls.
let current: Owner | null = null;
function draw(owner: Owner) {
  flushSync(() => owner.root.render(createElement(Fragment, null, owner.queue && createPortal(owner.queueView, owner.queue, "queue"), owner.attachments && createPortal(owner.attachmentView, owner.attachments, "attachments"), owner.menu && createPortal(owner.menuView, owner.menu, "menu"), ...[...owner.controls].map(([host, control]) => createPortal(control.node, host, control.key)))));
}
function ownerOf(field: HTMLElement): Owner {
  if (current?.field === field) return current;
  disposeComposerUi();
  const root = createDocumentView(document.createElement("div"));
  const observer = new MutationObserver(() => { if (current?.field === field && !field.isConnected) disposeComposerUi(); });
  observer.observe(document.body, { childList: true, subtree: true });
  current = { field, root, queue: null, attachments: null, menu: null, queueView: null, attachmentView: null, menuView: null, controls: new Map(), next: 0, observer };
  return current;
}
export function disposeComposerUi() {
  const owner = current; current = null;
  if (!owner) return;
  owner.observer.disconnect(); owner.root.unmount(); owner.queue?.remove(); owner.attachments?.remove(); owner.menu?.remove();
}
export function showAgentAttachments(field: HTMLElement, view: ReactElement | null) {
  if (!view && (!current || current.field !== field || !current.attachments)) return;
  const owner = ownerOf(field);
  if (!view) {
    const old = owner.attachments; owner.attachments = null; owner.attachmentView = null; draw(owner); old?.remove();
    return;
  }
  if (!owner.attachments) { owner.attachments = document.createElement("div"); owner.attachments.className = "ag-att"; owner.attachments.id = "agatt"; field.before(owner.attachments); }
  owner.attachmentView = view; draw(owner);
}
export function showAgentQueue(field: HTMLElement, items: readonly Waiting[]) {
  if (!items.length) {
    if (!current || current.field !== field || !current.queue) return;
    const owner = current, old = owner.queue; owner.queue = null; owner.queueView = null; draw(owner); old?.remove();
    return;
  }
  const owner = ownerOf(field);
  if (!owner.queue) { owner.queue = document.createElement("div"); owner.queue.className = "ag-queue"; owner.queue.id = "agqueue"; (document.getElementById("agatt") ?? field).before(owner.queue); }
  owner.queueView = createElement(AgentQueue, { items }); draw(owner);
}
function Menu({ rows, index, pick }: { rows: Row[]; index: number; pick(index: number): void }) {
  const selected = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { selected.current?.scrollIntoView({ block: "nearest" }); }, [index]);
  return <>{rows.map((row, i) => <div key={`${row.insert}:${i}`} ref={i === index ? selected : null} className={`cm-row${i === index ? " on" : ""}`} role="option" aria-selected={i === index} data-i={i} onMouseDown={event => { event.preventDefault(); pick(i); }}><span className="n">{row.label}</span><span className="d">{row.detail}</span>{row.hint && <small>{row.hint}</small>}</div>)}</>;
}
export function showAgentMenu(field: HTMLElement, rows: Row[], index: number, pick: (index: number) => void) {
  const owner = ownerOf(field);
  if (!owner.menu) { owner.menu = document.createElement("div"); owner.menu.className = "cm-menu"; owner.menu.setAttribute("role", "listbox"); field.append(owner.menu); }
  owner.menu.hidden = !rows.length;
  owner.menuView = rows.length ? createElement(Menu, { rows, index, pick }) : null;
  draw(owner);
  return owner.menu;
}

export function showAgentControl(field: HTMLElement, host: HTMLElement, node: ReactElement | null) {
  const owner = ownerOf(field), prior = owner.controls.get(host);
  if (!prior) host.replaceChildren();
  owner.controls.set(host, { key: prior?.key ?? ++owner.next, node });
  draw(owner);
}
