import { createElement } from "react";
import { flushSync } from "react-dom";
import { createDocumentScope, createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { StashPanel } from "./stash-view.tsx";
import { invoke, listen } from "../ui/tauri.ts";
import { h } from "../ui/dom.ts";
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
let panel: HTMLElement | null = null;
let panelRoot: DocumentView | null = null;
let panelProject: string | null = null;

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
  field.addEventListener("input", () => { if (panel) drawPanel(); });
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
  if (panel) drawPanel();
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

function close() {
  panelRoot?.unmount(); panelRoot = null;
  panel?.remove(); panel = null; panelProject = null;
  flushSync(changedIndicators);
}

function drawPanel() {
  if (!panel || !panelProject) return;
  const project = panelProject, list = entries.get(project) ?? [];
  const anchor = [...document.querySelectorAll<HTMLElement>("[data-stash-open]")].find(el => el.dataset.stashOpen === project);
  if (!anchor || !list.length) { close(); return; }
  const field = active(project), binding = field && bindings.get(field);
  const swap = !!field?.value.trim() || !!binding?.hasAttachments();
  const current = panelRoot;
  if (!current) return;
  flushSync(() => current.render(createElement(StashPanel, { project, entries: list, busy, canPop: !!field, swap })));

  positionPanel();
}

function positionPanel() {
  if (!panel || !panelProject) return;
  const anchor = [...document.querySelectorAll<HTMLElement>("[data-stash-open]")].find(el => el.dataset.stashOpen === panelProject);
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
  panel = h('<div class="prompt-stash-panel" role="dialog" aria-label="Project stash"></div>');
  document.body.append(panel);
  panelRoot = createDocumentView(panel);
  flushSync(changedIndicators);
  drawPanel();
  panel?.querySelector<HTMLButtonElement>("[data-stash-close]")?.focus();
}

async function act(action: "save" | "pop", field: HTMLTextAreaElement, id?: string) {
  const binding = bindings.get(field), project = binding?.project();
  if (!binding || busy) return;
  if (!project) { note(field, "Choose a project before using the stash"); return; }
  const live = () => field.isConnected && binding.project() === project && bindings.get(field) === binding;
  const openedPanel = panel;
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

    if (panel === openedPanel) close();
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

document.addEventListener("click", e => {
  const button = (e.target as Element).closest<HTMLElement>("[data-stash-open], [data-stash-action], [data-stash-pop], [data-stash-remove], [data-stash-close]");
  if (!button) { if (panel && !panel.contains(e.target as Node)) close(); return; }
  if (button instanceof HTMLButtonElement && button.disabled) return;
  e.preventDefault();

  if (button.dataset.stashOpen) { if (panelProject === button.dataset.stashOpen) close(); else show(button.dataset.stashOpen); }
  else if (button.hasAttribute("data-stash-close")) close();
  else if (button.dataset.stashRemove && panelProject && !busy) {
    const project = panelProject;
    void invoke("prompt_stash_remove", { project, id: button.dataset.stashRemove }).then(() => reload(project)).catch(error => { const field = active(project); if (field) note(field, String(error)); });
  } else {
    const field = active(panelProject ?? undefined);
    if (field) void act(button.dataset.stashAction === "save" ? "save" : "pop", field, button.dataset.stashPop);
  }
});
document.addEventListener("keydown", e => { if (e.key === "Escape" && panel) { e.preventDefault(); e.stopImmediatePropagation(); close(); active()?.focus(); } }, true);
addEventListener("resize", positionPanel);
document.addEventListener("scroll", e => { if (panel && !panel.contains(e.target as Node)) positionPanel(); }, true);
void listen<string>("prompt-stash-changed", e => { void reload(e.payload); });
