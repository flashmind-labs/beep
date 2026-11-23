// Where the pane's docks sit and how big they are: the split of tabs
// between them, zoom, ctrl-tab between tabs, and the dividers. The dock
// components (dock-view.tsx) draw from the model kept here.
import { createRef, type RefObject } from "react";
import { flushSync } from "react-dom";
import { createStore } from "zustand";
import { fileBase64 } from "../chat/attach.tsx";
import { showFailure } from "../ui/failbanner-view.tsx";
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { invoke } from "../ui/tauri.ts";
import { isMobile, onPhone } from "../ui/platform.ts";
import { type Box, type Dock, DOCKS, frameKey, onZoomKey, paneSize, SIDE_BY_SIDE_MIN, splitTabs } from "./media-state.ts";
import type { PaneTabItem } from "./tabs-view.tsx";
import type { WorktreeMenuModel } from "./worktree-view.tsx";
import { artifactCount } from "./artifacts.ts";
import { PANE, S } from "../bar/core.ts";
import { runningCount } from "./running.ts";
import { hasDeviceTab } from "./device.ts";
import { sessions } from "../agents/sessions.ts";
import { sessionVisible } from "../agents/closing.ts";
import { chatOf, chatRail } from "../agents/threads-state.ts";
import { shell as windowShell } from "../center/window-shell.tsx";
import { P, openPreviewExport, rememberPane, saveLayout, showTab, FILES, CHANGES, ARTS, RUNNING, SUBS, DEVICE, AGENTS, type MediaTab } from "./media.ts";
import { chatAgentsCount } from "./chat-agents.tsx";
import { hasFolderEntries } from "./files.ts";

/// How far an arrow key moves a divider.
const ARROW_STEP = 24;

// ----- The pane's place -----

/// The session's shell, the conversation with the bottom dock under it, and each dock.
export const shellRef = createRef<HTMLDivElement>();
export const mainRef = createRef<HTMLDivElement>();
export const paneRefs: Record<Dock, RefObject<HTMLElement | null>> = { right: createRef(), bottom: createRef() };
/// The strip a folded dock leaves.
export const stripRefs: Record<Dock, RefObject<HTMLElement | null>> = { right: createRef(), bottom: createRef() };

export const shell = () => shellRef.current?.isConnected ? shellRef.current : null;
export const main = () => mainRef.current?.isConnected ? mainRef.current : null;
export const paneOf = (d: Dock) => paneRefs[d].current?.isConnected ? paneRefs[d].current : null;

/// The shell's width and the conversation's height, as last read, and the shell
/// the pane's resize watcher follows.
let sized: { shell: HTMLElement; width: number; height: number } | null = null;
let watched: HTMLElement | null = null;

/// The pane's resize watcher follows `el` now, or nothing. The same shell
/// keeps its size through a session switch.
export function watchShell(el: HTMLElement | null) {
  if (watched !== el) sized = null;
  watched = el;
}

/// The watcher saw the shell resize: its size is read again.
export function shellResized() {
  sized = null;
}

/// The shell's width and the conversation's height. Kept until the watcher sees the
/// shell resize, so renders and visibility checks force no layout; read each time
/// while no watcher follows the shell, or while it is hidden, as it can show before
/// the watcher hears of it. Zero without a shell.
export function shellSize(): { width: number; height: number } {
  const el = shell();
  if (!el) return { width: 0, height: 0 };
  if (sized?.shell === el && watched === el) return sized;
  const size = { shell: el, width: el.clientWidth, height: main()?.clientHeight ?? 0 };
  sized = size.width > 0 ? size : null;
  return size;
}

/// The dock holding `node`, if any.
export const dockHolding = (node: Node | null) => DOCKS.find(d => !!node && !!paneOf(d)?.contains(node)) ?? null;

/// Every tab's id, in the order the tab strips show them.
export function tabIds() {
  const chat = chatPane();
  return [FILES, ...(chat ? [] : [CHANGES]), ...(runningCount() ? [RUNNING] : []), ...(artifactCount() ? [ARTS] : []), ...(P.subCount && !chat ? [SUBS] : []), ...(chat && chatAgentsCount(chat) ? [AGENTS] : []), ...(hasDeviceTab() ? [DEVICE] : []), ...P.tabs.map(t => t.id)];
}

/// The chat of the pane of an assistant chat or one of its threads: their folders have no git,
/// and the list beside them shows the threads. Null for any other session.
export function chatPane() {
  const s = P.session;
  if (!s) return null;
  const row = sessions.find(r => r.machine_id === s.machine_id && r.id === s.id);
  return (row && chatOf(sessions, row)) || chatOf(sessions, s);
}

/// The desktop Assistant keeps watching its working folder even without a pane.
export function desktopChatPane() {
  return PANE && !onPhone() && !isMobile() ? chatPane() : null;
}

function rightPaneAvailable(tabs: string[]) {
  const chat = desktopChatPane();
  if (!chat || P.rightRequested || hasFolderEntries() || P.tabs.some(tab => tabs.includes(tab.id))) return true;

  const rail = chatRail(sessions.filter(sessionVisible), chat);
  return !!rail?.running.some(row => row !== rail.chat);
}

/// The width the docks lay out in. A phone never gets a right dock: the phone layout shows one
/// pane at a time, and an unfolded phone keeps the conversation full width with the dock under it.
export function dockWidth(): number {
  if (!shell()) return SIDE_BY_SIDE_MIN;
  return onPhone() || document.body.classList.contains("mobile") ? 0 : shellSize().width;
}

/// Whether the chats list beside an assistant chat steps aside for the right pane: the pane
/// shows, and the conversation and the pane fit beside each other only without the list.
function chatsListCrowded(rightShown: boolean): boolean {
  const { assistant, side } = windowShell.getState();
  const box = document.getElementById("pane-box");
  if (!rightShown || !assistant || (side.rail?.kind === "chat" && !side.railHidden) || !box) return false;

  const list = parseFloat(getComputedStyle(box).getPropertyValue("--asst-w")) || 300;
  return box.clientWidth >= SIDE_BY_SIDE_MIN && box.clientWidth - list < SIDE_BY_SIDE_MIN;
}

/// The tabs in each dock, as the shell's width lets them sit.
export function docked(): Record<Dock, string[]> {
  return splitTabs(P.layout, tabIds(), dockWidth(), P.moved);
}

/// The dock tab `id` shows in now.
export function dockOf(id: string): Dock {
  return docked().bottom.includes(id) ? "bottom" : "right";
}

// ----- The model the docks draw -----

/// One view in a dock: a fixed tab's, or an opened tab's at its revision.
export type DockView =
  | { kind: "files" | "changes" | "artifacts" | "running" | "subagents" | "agents" | "device"; hidden: boolean }
  | { kind: "tab"; tab: MediaTab; rev: number; hidden: boolean };

export interface DockModel {
  tabs: PaneTabItem[];
  on: string;
  /// The pane shows, or only its strip.
  shown: boolean;
  strip: boolean;
  /// Its size along the shell, once it was laid out.
  size: number | null;
  zoomed: boolean;
  /// The worktree menu in its head, while it shows Files or Changes.
  worktrees: WorktreeMenuModel | null;
  views: DockView[];
}

/// A tab being dragged: its strip item, whether its dock shows it, and the
/// dock it would drop in, with where that dock is in the shell.
interface DragModel { item: PaneTabItem; selected: boolean; to: Dock | null; zone: Box | null }

interface PaneModel {
  docks: Record<Dock, DockModel> | null;
  narrow: boolean;
  zoomed: boolean;
  /// The chats list steps aside so the right pane fits beside the conversation.
  crowded: boolean;
  /// The divider being dragged.
  resizing: Dock | null;
  drag: DragModel | null;
  /// On a phone: its tabs, and whether the conversation is the one shown.
  phone: { tabs: { id: string; name: string }[]; selected: string } | null;
  log: boolean;
}

export const paneModel = createStore<PaneModel>(() => ({ docks: null, narrow: false, zoomed: false, crowded: false, resizing: null, drag: null, phone: null, log: false }));

/// Sets part of the model and renders it now: callers measure and focus what it draws.
export function setPaneModel(change: Partial<PaneModel>) {
  flushSync(() => paneModel.setState(change));
}

/// Lays the docks out for the shell's size: which show, their size,
/// folded or not. A dock with no tabs goes.
let paneWasShown = false;

export function place(fresh?: Record<Dock, DockModel>, extra: Partial<PaneModel> = {}) {
  const el = shell(), box = main();
  const docks = fresh ?? paneModel.getState().docks;
  if (!docks) return;
  if (!el || !box) { setPaneModel({ ...extra, docks }); return; }
  const { width, height } = shellSize();
  const narrow = dockWidth() < SIDE_BY_SIDE_MIN;
  const over = foldNarrow();
  const split = docked();
  const next = { ...docks };
  const rightAvailable = rightPaneAvailable(split.right);

  for (const d of DOCKS) {
    const has = !!P.session && split[d].length > 0 && (d === "bottom" || rightAvailable);
    const shown = has && !P.layout.hidden[d];
    const total = d === "right" ? width : height;
    next[d] = { ...docks[d], shown, strip: has && P.layout.hidden[d], size: shown ? paneSize(d, P.layout[d], total) : docks[d].size };
  }

  const shown = DOCKS.some(d => next[d].shown);
  // A zoomed dock that folded, emptied or moved away ends the zoom.
  if (P.zoom && !next[P.zoom].shown) P.zoom = null;
  // On a narrow window the right pane, when open, covers the conversation. An assistant chat's
  // list steps aside instead, when that gives the pane room beside it.
  const crowded = chatsListCrowded(next.right.shown);
  const zoom = over && next.right.shown && !crowded ? "right" : P.zoom;
  for (const d of DOCKS) next[d] = { ...next[d], zoomed: zoom === d };
  setPaneModel({ ...extra, docks: next, narrow, zoomed: !!zoom, crowded });
  if (shown && !paneWasShown) queueMicrotask(() => window.dispatchEvent(new Event("agent-pane-shown")));
  paneWasShown = shown;
  refreshTopicSubscriptions();
}

/// A window too narrow for the conversation beside the right pane folds that pane to its
/// strip, keeping the wide layout's choice to put back once it is wide again. True while
/// the window is that narrow; a phone, which has no right pane, never is.
export function foldNarrow(): boolean {
  const width = dockWidth(), over = width > 0 && width < SIDE_BY_SIDE_MIN;
  if (over && P.wideFolded === null) {
    P.wideFolded = P.layout.hidden.right;
    P.layout.hidden.right = true;
  } else if (!over && P.wideFolded !== null) {
    P.layout.hidden.right = P.wideFolded;
    P.wideFolded = null;
  }

  return over;
}

/// The docks on screen now, as last laid out.
export function shownDocks(): Dock[] {
  const docks = paneModel.getState().docks;
  return docks ? DOCKS.filter(d => docks[d].shown && !!paneOf(d)) : [];
}

/// ⇧esc: the dock with focus, else the right one, else the bottom one, fills the session
/// over the conversation; again puts it back. Only classes change, so a model, a video
/// or a terminal keeps its state and fits the new size on its own.
function toggleZoom() {
  // Narrow, the open right pane already covers the conversation: it folds instead.
  if (foldNarrow() && !P.layout.hidden.right) P.layout.hidden.right = true;
  else if (P.zoom) P.zoom = null;
  else {
    const shown = shownDocks();
    P.zoom = dockHolding(document.activeElement) ?? shown.find(d => d === "right") ?? shown[0] ?? null;
  }

  place();
  rememberPane();
  // From the input, the zoomed tab takes the keys: a terminal fills the view and is typed in.
  const zoom = P.zoom, pane = zoom && paneOf(zoom);

  if (zoom && pane && !pane.contains(document.activeElement)) {
    const term = P.tabs.find(t => t.id === P.on[zoom])?.term;
    if (term) term.screen.focus(); else pane.focus({ preventScroll: true });
  }
}

// S.view stays "agent" while the center shows another page; only a session on screen zooms.
onZoomKey(() => S.view === "agent" && !!P.session && shellSize().width > 0, toggleZoom);

// Sandboxed previews send links, exports and shortcuts to their owning pane.
window.addEventListener("message", (event: MessageEvent<unknown>) => {
  const data = event.data;
  if (!data || typeof data !== "object" || !("beep" in data)) return;

  if (data.beep === "link" || data.beep === "export") {
    const frame = document.activeElement;
    if (!(frame instanceof HTMLIFrameElement) || !frame.matches(".agartframe") || frame.contentWindow !== event.source) return;
    if (event.origin !== "null" || !navigator.userActivation.isActive) return;
    if (data.beep === "export") {
      const port = event.ports[0], dock = dockHolding(frame);
      if (port && dock) acceptPreviewExport(frame, port, dock);
    } else if ("url" in data && typeof data.url === "string" && /^https?:\/\//.test(data.url)) {
      invoke("open_url", { url: data.url }).catch(e => console.warn("opening a preview link:", e));
    }
    return;
  }

  const key = frameKey(data);
  const frame = key && [...document.querySelectorAll<HTMLIFrameElement>("iframe.agartframe")].find(f => f.contentWindow === event.source);
  if (frame) frame.dispatchEvent(new KeyboardEvent("keydown", { ...key, bubbles: true, cancelable: true }));
});

const pendingExports = new WeakSet<HTMLIFrameElement>();
const MAX_EXPORT_BYTES = 16 * 1024 * 1024;

function acceptPreviewExport(frame: HTMLIFrameElement, port: MessagePort, dock: Dock) {
  const session = P.session;
  if (!session || frame.closest("[hidden]") || pendingExports.has(frame)) {
    port.close();
    return;
  }

  const source = { tab: P.on[dock], url: frame.src };
  pendingExports.add(frame);
  const fail = (reason: unknown) => showFailure(`Could not export file.\n\n${reason instanceof Error ? reason.message : String(reason)}`);
  const close = () => { clearTimeout(timeout); port.close(); };
  const timeout = setTimeout(() => {
    close();
    pendingExports.delete(frame);
    fail("The preview stopped sending the export.");
  }, 35_000);
  port.onmessageerror = () => {
    close();
    pendingExports.delete(frame);
    fail("Could not read the export.");
  };
  port.onmessage = (event: MessageEvent<unknown>) => {
    close();
    void savePreviewExport(event.data).then(({ path, blob }) => {
      if (P.session === session && shell()) openPreviewExport(path, blob, source);
    }).catch(fail).finally(() => pendingExports.delete(frame));
  };
}

async function savePreviewExport(data: unknown) {
  if (!data || typeof data !== "object") throw new Error("Could not read the export.");
  if ("error" in data && data.error === "too-large") throw new Error("The export is larger than 16 MB.");
  if (!("name" in data) || typeof data.name !== "string" || !("blob" in data) || !(data.blob instanceof Blob)) {
    throw new Error("Could not read the export.");
  }
  if (data.blob.size > MAX_EXPORT_BYTES) throw new Error("The export is larger than 16 MB.");

  const mime = data.blob.type.split(";")[0];
  const kind = mime === "image/svg+xml" ? "svg" : mime === "application/pdf" ? "pdf" : /\.(svg|pdf)$/i.exec(data.name)?.[1].toLowerCase();
  if (kind !== "svg" && kind !== "pdf") throw new Error("Only SVG and PDF exports are supported.");

  const blob = new Blob([data.blob], { type: kind === "svg" ? "image/svg+xml" : "application/pdf" });
  const path = await invoke<string>("agent_preview_export", { name: data.name.slice(0, 1024), kind, data: await fileBase64(blob) });
  return { path, blob };
}

// ⌃tab with focus in a dock: its next tab, round to the first; ⌃⇧tab the one before.
window.addEventListener("keydown", e => {
  if (e.key !== "Tab" || !e.ctrlKey || e.metaKey || e.altKey) return;
  const d = dockHolding(e.target instanceof Node ? e.target : null);
  if (!d) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  const ids = docked()[d];
  const step = e.shiftKey ? ids.length - 1 : 1;
  showTab(d, ids[(ids.indexOf(P.on[d]) + step) % ids.length]);
}, true);

// ----- Dividers -----

/// Sets dock `dock`'s size, or the default for null, as the shell allows.
function resize(dock: Dock, size: number | null) {
  if (!shell() || !main()) return;
  const { width, height } = shellSize();
  P.layout[dock] = size == null ? null : paneSize(dock, size, dock === "right" ? width : height);
  place();
}

/// A drag of dock `dock`'s divider. While it moves only the dock's size, a
/// CSS variable on its element, changes; the layout takes it when the drag ends.
export function dividerDrag(dock: Dock) {
  const el = shell(), box = main(), pane = paneOf(dock);
  if (!el || !box || !pane) return;
  setPaneModel({ resizing: dock });
  const right = dock === "right";
  const r = (right ? el : box).getBoundingClientRect();
  const total = right ? el.clientWidth : box.clientHeight;
  let size: number | null = null;
  return {
    move: (ev: PointerEvent) => {
      size = paneSize(dock, right ? r.right - ev.clientX : r.bottom - ev.clientY, total);
      pane.style.setProperty("--drag-size", `${size}px`);
    },
    end: () => {
      setPaneModel({ resizing: null });
      if (size !== null) resize(dock, size);
      pane.style.removeProperty("--drag-size");
      saveLayout();
    },
  };
}

/// Double-click on a divider: the dock's default size.
export function resetDivider(dock: Dock) {
  resize(dock, null);
  saveLayout();
}

/// Arrow keys on a divider grow or shrink its dock; false for other keys.
export function dividerKey(dock: Dock, key: string) {
  const pane = paneOf(dock);
  const grow: Record<string, number> = dock === "right" ? { ArrowLeft: 1, ArrowRight: -1 } : { ArrowUp: 1, ArrowDown: -1 };
  const by = grow[key];
  if (!by || !pane) return false;
  const now = dock === "right" ? pane.offsetWidth : pane.offsetHeight;
  resize(dock, now + by * ARROW_STEP);
  saveLayout();
  return true;
}
