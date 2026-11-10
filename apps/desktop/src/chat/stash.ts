import { createElement, createRef } from "react";
import { flushSync } from "react-dom";
import { createDocumentScope } from "../ui/document-root.tsx";
import { StashPanel } from "./stash-view.tsx";
import { invoke, listen } from "../ui/tauri.ts";
import { hasStashMessage, type StashEntry, type StashMessage } from "./stash-state.ts";

export interface StashBinding {
  project(): string | null;
  hasAttachments(): boolean;
  capture(): Promise<StashMessage>;
  clear(): void;
  /** Prepares attachments before removing the entry from disk. */
  prepare(message: StashMessage): Promise<() => void>;
  lock?(busy: boolean): void;
}

const bindings = new WeakMap<HTMLTextAreaElement, StashBinding>();
const entries = new Map<string, StashEntry[]>();
const reads = new Map<string, number>();
let busy = false;
const panelScope = createDocumentScope<"panel">();
const panelRef = createRef<HTMLDivElement>();
let panelProject: string | null = null;
let panelOpened = 0;

function active(project?: string) {
  return [...document.querySelectorAll<HTMLTextAreaElement>("textarea[data-prompt-stash]")].find(field => {
    const binding = bindings.get(field);
    return binding && (!project || binding.project() === project) && field.getClientRects().length;
  });
}

export function bindPromptStash(field: HTMLTextAreaElement, binding: StashBinding) {
  field.dataset.promptStash = "";
  bindings.set(field, binding);
  field.addEventListener("keydown", e => {
    if (!e.ctrlKey || e.metaKey || e.altKey || e.shiftKey || e.isComposing || !["s", "p"].includes(e.key.toLowerCase())) return;
    e.preventDefault(); e.stopPropagation();
    void act(e.key.toLowerCase() === "s" ? "save" : "pop", field);
  });
  field.addEventListener("input", () => { if (panelProject) drawPanel(); });
}

const subscribers = new Set<() => void>();
let revision = 0;
export function subscribeStash(listener: () => void) { subscribers.add(listener); return () => { subscribers.delete(listener); }; }
export function stashRevision() { return revision; }
export function stashCount(project: string) { return entries.get(project)?.length ?? 0; }
export function stashIsOpen(project: string) { return panelProject === project; }
export function ensureStash(project: string) { if (!reads.has(project)) void reload(project); }
function changedIndicators() { revision++; for (const listener of subscribers) listener(); }
function drawIndicators() {
  flushSync(changedIndicators);
  if (panelProject) drawPanel();
}

async function reload(project: string) {
  const read = (reads.get(project) ?? 0) + 1;
  reads.set(project, read);

  try {
    const list = await invoke<StashEntry[]>("prompt_stash_list", { project });
    if (reads.get(project) !== read) return;
    entries.set(project, list);
    drawIndicators();
  } catch (error) {
    reads.delete(project);
    const field = active(project);
    if (field) note(field, `Could not read the stash: ${String(error)}`);
  }
}

const noticeScope = createDocumentScope<string>();
const notices = new WeakMap<HTMLElement, { key: string; timer: ReturnType<typeof setTimeout> }>();
let nextNotice = 0;
function note(field: HTMLTextAreaElement, text: string) {
  const root = field.closest<HTMLElement>(".agconv, form, .dinput") ?? field.parentElement;
  if (!root) return;
  const prior = notices.get(root), key = prior?.key ?? String(++nextNotice);
  if (prior) clearTimeout(prior.timer);
  noticeScope.set([{ slot: key, host: root, content: createElement("div", { className: "prompt-stash-note", role: "status" }, text) }]);
  const timer = setTimeout(() => { noticeScope.remove([key]); notices.delete(root); }, 6000);
  notices.set(root, { key, timer });
}

function anchorOf(project: string) {
  return [...document.querySelectorAll<HTMLElement>("[data-stash-open]")].find(el => el.dataset.stashOpen === project) ?? null;
}

function close() {
  panelProject = null;
  panelScope.remove(["panel"]);
  flushSync(changedIndicators);
}

function drawPanel() {
  if (!panelProject) return;
  const project = panelProject, list = entries.get(project) ?? [];
  if (!anchorOf(project) || !list.length) { close(); return; }
  const field = active(project), binding = field && bindings.get(field);
  const swap = !!field?.value.trim() || !!binding?.hasAttachments();
  const content = createElement(StashPanel, { project, entries: list, busy, canPop: !!field, swap, panelRef, onClose: close, onRemove: id => remove(project, id), onPop: id => pop(id), onOutside: outside });
  panelScope.set([{ slot: "panel", host: document.body, content, identity: String(panelOpened) }]);

  positionPanel();
}

function positionPanel() {
  const panel = panelRef.current;
  if (!panel || !panelProject) return;
  const anchor = anchorOf(panelProject);
  if (!anchor) { close(); return; }
  const rect = anchor.getBoundingClientRect();
  panel.style.width = `${Math.min(400, innerWidth - 32)}px`;
  panel.style.left = `${Math.max(16, Math.min(rect.right - panel.offsetWidth, innerWidth - panel.offsetWidth - 16))}px`;
  panel.style.bottom = `${Math.max(16, innerHeight - rect.top + 8)}px`;
  panel.style.maxHeight = `${Math.max(100, rect.top - 24)}px`;
}

function show(project: string) {
  close();
  if (!entries.get(project)?.length) return;
  panelProject = project;
  panelOpened++;
  flushSync(changedIndicators);
  drawPanel();
}

/// Opens or closes a project's stash from its indicator.
export function toggleStash(project: string) {
  if (panelProject === project) close();
  else show(project);
}

/// Runs a stash shortcut from the hints on the visible composer.
export function stashAction(action: "save" | "pop") {
  const field = active(panelProject ?? undefined);
  if (field) void act(action, field);
}

function pop(id: string) {
  const field = active(panelProject ?? undefined);
  if (field) void act("pop", field, id);
}

function remove(project: string, id: string) {
  if (busy) return;
  void invoke("prompt_stash_remove", { project, id }).then(() => reload(project)).catch(error => { const field = active(project); if (field) note(field, String(error)); });
}

/// The indicators and hint actions toggle or act on the panel themselves.
function outside(target: EventTarget | null) {
  if (target instanceof Element && target.closest("[data-stash-open], [data-stash-action]")) return;
  close();
}

async function act(action: "save" | "pop", field: HTMLTextAreaElement, id?: string) {
  const binding = bindings.get(field), project = binding?.project();
  if (!binding || busy) return;
  if (!project) { note(field, "Choose a project before using the stash"); return; }
  const live = () => field.isConnected && binding.project() === project && bindings.get(field) === binding;
  const opened = panelProject ? panelOpened : null;
  busy = true;
  const readOnly = field.readOnly;
  const root = field.closest<HTMLElement>(".agconv, form");
  const inert = root?.inert ?? false;
  if (root) root.inert = true;
  field.readOnly = true;
  binding.lock?.(true);
  drawPanel();

  try {
    if (action === "save") {
      const message = await binding.capture();
      if (!hasStashMessage(message) || !live()) return;
      await invoke("prompt_stash_put", { project, message });
      if (live()) { binding.clear(); note(field, "Message stashed"); }
    } else {
      if (!entries.has(project)) await reload(project);
      id ??= entries.get(project)?.[0]?.id;
      if (!id) { note(field, "Stash is empty"); return; }
      const message = await invoke<StashMessage>("prompt_stash_read", { project, id });
      const current = await binding.capture();
      const apply = await binding.prepare(message);
      if (!live()) return;
      await invoke("prompt_stash_take", { project, id, swap: hasStashMessage(current) ? current : null });
      if (live()) apply();
      else await invoke("prompt_stash_put", { project, message });
    }

    if (panelProject && panelOpened === opened) close();
    await reload(project);
  } catch (error) {
    note(field, String(error));
    await reload(project);
  } finally {
    busy = false; field.readOnly = readOnly; binding.lock?.(false);
    if (root) root.inert = inert;
    drawPanel();
    if (live()) field.focus();
  }
}

document.addEventListener("keydown", e => { if (e.key === "Escape" && panelProject) { e.preventDefault(); e.stopImmediatePropagation(); close(); active()?.focus(); } }, true);
addEventListener("resize", positionPanel);
document.addEventListener("scroll", e => { if (panelProject && !panelRef.current?.contains(e.target as Node)) positionPanel(); }, true);
void listen<string>("prompt-stash-changed", e => { void reload(e.payload); });
