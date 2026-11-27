import { flushSync } from "react-dom";
import { createStore } from "zustand";
import { createDocumentScope } from "../../ui/document-root.tsx";
import { StashPanel } from "./stash-view.tsx";
import { invoke, listen } from "../../ui/tauri.ts";
import { hasStashMessage, type StashEntry, type StashMessage } from "../../stores/chat/stash-state.ts";

interface StashBinding {
  canUse?(): boolean;
  project(): string | null;
  hasAttachments(): boolean;
  capture(): Promise<StashMessage>;
  clear(): void;
  /** Prepares attachments before removing the entry from disk. */
  prepare(message: StashMessage): Promise<() => void>;
  lock?(busy: boolean): void;
}

/// Each project's stash entries, and the project whose panel is open.
export const stash = createStore(() => ({ entries: new Map<string, StashEntry[]>(), open: null as string | null }));

const bindings = new WeakMap<HTMLTextAreaElement, StashBinding>();
/// The bound fields, held weakly: a composer that is gone drops out.
const fields = new Set<WeakRef<HTMLTextAreaElement>>();
/// The Stash buttons the indicators draw, with their projects.
const anchors = new Map<HTMLElement, string>();
const reads = new Map<string, number>();
let busy = false;
const panelScope = createDocumentScope<"panel">();
let panelOpened = 0;

/// The shown field bound to `project`, or to any project.
function active(project?: string) {
  for (const ref of fields) {
    const field = ref.deref();
    if (!field) { fields.delete(ref); continue; }

    const binding = bindings.get(field);
    if (binding && (!project || binding.project() === project) && field.getClientRects().length) return field;
  }

  return undefined;
}

/// Keeps a Stash button `el` for project `project` while it is drawn; a ref callback.
export function stashAnchor(project: string) {
  return (el: HTMLElement | null) => {
    if (!el) return;
    anchors.set(el, project);
    return () => { anchors.delete(el); };
  };
}

export function bindPromptStash(field: HTMLTextAreaElement, binding: StashBinding) {
  field.dataset.promptStash = "";
  bindings.set(field, binding);
  fields.add(new WeakRef(field));
  field.addEventListener("keydown", e => {
    if (!e.ctrlKey || e.metaKey || e.altKey || e.shiftKey || e.isComposing || !["s", "p"].includes(e.key.toLowerCase())) return;
    e.preventDefault(); e.stopPropagation();
    void act(e.key.toLowerCase() === "s" ? "save" : "pop", field);
  });
  field.addEventListener("input", () => { if (opened()) drawPanel(); });
}

export function ensureStash(project: string) { if (!reads.has(project)) void reload(project); }
const opened = () => stash.getState().open;
const listOf = (project: string) => stash.getState().entries.get(project);
function setOpen(open: string | null) { flushSync(() => stash.setState({ open })); }

async function reload(project: string) {
  const read = (reads.get(project) ?? 0) + 1;
  reads.set(project, read);

  try {
    const list = await invoke<StashEntry[]>("prompt_stash_list", { project });
    if (reads.get(project) !== read) return;
    flushSync(() => stash.setState(state => ({ entries: new Map(state.entries).set(project, list) })));
    if (opened()) drawPanel();
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
  noticeScope.set([{ slot: key, host: root, content: <div className="prompt-stash-note" role="status">{text}</div> }]);
  const timer = setTimeout(() => { noticeScope.remove([key]); notices.delete(root); }, 6000);
  notices.set(root, { key, timer });
}

function anchorOf(project: string) {
  const drawn = [...anchors].filter(([el, of]) => of === project && el.isConnected).map(([el]) => el);
  return drawn.find(el => el.getClientRects().length) ?? drawn[0] ?? null;
}

function close() {
  panelScope.remove(["panel"]);
  setOpen(null);
}

function drawPanel() {
  const project = opened();
  if (!project) return;
  const list = listOf(project) ?? [];
  const anchor = anchorOf(project);
  if (!anchor || !list.length) { close(); return; }
  const field = active(project), binding = field && bindings.get(field);
  const swap = !!field?.value.trim() || !!binding?.hasAttachments();
  const content = <StashPanel project={project} entries={list} busy={busy} canPop={!!field} swap={swap} anchor={anchor} onClose={close} onRemove={id => remove(project, id)} onPop={id => pop(id)} onOutside={outside} />;
  panelScope.set([{ slot: "panel", host: document.body, content, identity: String(panelOpened) }]);
}

function show(project: string) {
  close();
  if (!listOf(project)?.length) return;
  panelOpened++;
  setOpen(project);
  drawPanel();
}

/// Opens or closes a project's stash from its indicator.
export function toggleStash(project: string) {
  if (opened() === project) close();
  else show(project);
}

/// Runs a stash shortcut from the hints on the visible composer.
export function stashAction(action: "save" | "pop") {
  const field = active(opened() ?? undefined);
  if (field) void act(action, field);
}

function pop(id: string) {
  const field = active(opened() ?? undefined);
  if (field) void act("pop", field, id);
}

function remove(project: string, id: string) {
  if (busy) return;
  void invoke("prompt_stash_remove", { project, id }).catch(error => { const field = active(project); if (field) note(field, String(error)); });
}

/// The indicators and hint actions toggle or act on the panel themselves.
function outside(target: EventTarget | null) {
  if (target instanceof Element && target.closest("[data-stash-open], [data-stash-action]")) return;
  close();
}

async function act(action: "save" | "pop", field: HTMLTextAreaElement, id?: string) {
  const binding = bindings.get(field), project = binding?.project();
  if (!binding || busy || binding.canUse?.() === false) return;
  if (!project) { note(field, "Choose a project before using the stash"); return; }
  const live = () => field.isConnected && binding.project() === project && bindings.get(field) === binding;
  const shown = opened() ? panelOpened : null;
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
      if (!listOf(project)) await reload(project);
      id ??= listOf(project)?.[0]?.id;
      if (!id) { note(field, "Stash is empty"); return; }
      const message = await invoke<StashMessage>("prompt_stash_read", { project, id });
      const current = await binding.capture();
      const apply = await binding.prepare(message);
      if (!live()) return;
      await invoke("prompt_stash_take", { project, id, swap: hasStashMessage(current) ? current : null });
      if (live()) apply();
      else await invoke("prompt_stash_put", { project, message });
    }

    // A write that went through says so in `prompt-stash-changed`, which reads the list.
    if (opened() && panelOpened === shown) close();
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

document.addEventListener("keydown", e => { if (e.key === "Escape" && opened()) { e.preventDefault(); e.stopImmediatePropagation(); close(); active()?.focus(); } }, true);
void listen<string>("prompt-stash-changed", e => { void reload(e.payload); });
