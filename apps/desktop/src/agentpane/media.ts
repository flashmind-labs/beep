import { createElement, Fragment } from "react";
import { createPortal } from "react-dom";
import { removeSessionPortal, setSessionChild, setSessionPortal } from "../agents/session-frame.tsx";
import { SubagentsView, type SubagentRow } from "./subagents-view.tsx";
import { PaneTabs, PhonePaneTabs, type PaneTabItem } from "./tabs-view.tsx";
import { readView, writeView, viewKey, rememberScroll } from "./view-memory.ts";
import { isHtmlFile, queuePreview } from "./html-preview-state.ts";
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
// agentpane/: a coding agent session's side pane and its tabs (media,
// files, changes, artifacts, terminal). Entry: this file.
// The media pane of a coding agent session: pictures, videos, 3D models and
// PDFs the agent made or looked at, its files, artifacts, terminals and
// subagents, each a tab.
import { baseName, type Dock, DOCKS, type ImageView, type MediaKind, mediaKind, moveTab, onZoomKey, type PaneLayout, paneSize, readLayout, resolvePath, SIDE_BY_SIDE_MIN, splitTabs, tabDock } from "./media-state.ts";
import { terminalName } from "./files-state.ts";
import type { Artifact } from "./artifacts-state.ts";
import type { ModelView } from "./model3d.ts";
import { artifactCount, artifactsView, checkArtifacts, pickArtifact, setArtifacts, showFrames } from "./artifacts.ts";
import { changeListeners, PANE, S } from "../bar/core.ts";
import { changesView, dropChanges, focusChanges, madeChangesView, reloadChanges } from "./changes.ts";
import { closeFilesMenu, filesView, pointFilesAt } from "./files.ts";
import { drawWorktrees, paneRoot, startWorktrees } from "./worktree.ts";
import { dropTerm, killTerm, type PaneTerm, type TermPlace } from "./terminal.ts";
import type { TerminalRow } from "../terminals/terminals.ts";
import { newTermLike } from "../terminals/newkey-state.ts";
import { drag, tabDown } from "./drag.ts";
import { drawTermFooter } from "./term-footer.tsx";
import { readError, loadText, startTerminal, load } from "./read.ts";
import { viewOf } from "./views.ts";
import { frameResize, h } from "../ui/dom.ts";
import { ICON } from "../ui/icons.ts";
import { invoke } from "../ui/tauri.ts";
import { runningCount, runningView } from "./running.ts";
import { onShownChange } from "../ui/active.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";
import { writeClipboard } from "../ui/parts.ts";
import { dragHandle } from "../ui/drag.ts";

/// What the node gives of a file, a part at a time (`agent_file`).
export interface FilePart {
  size: number;
  mime: string;
  /// Base64.
  data: string;
}

/// The start of a file of the Files tab, as `agent_tree_text` reads it.
export interface TreeText {
  size: number;
  text: string;
  cut: boolean;
  binary: boolean;
}

/// What a tab shows: a media file, a file of the Files tab as text, or a
/// terminal.
type TabKind = MediaKind | "text" | "term";

/// One tab: a file of the session's machine, a picture already here, or a
/// terminal on the machine.
export interface MediaTab {
  /// The path on the machine, the picture's data URL, `text:` and the
  /// path in the session's folder, or `term:` and its name.
  id: string;
  kind: TabKind;
  name: string;
  /// Where the node reads it, for a file of the machine.
  path: string | null;
  /// Its bytes as a URL once here, and how many there were.
  url: string | null;
  size: number;
  /// Bytes read so far while it loads, and why it could not be.
  done: number;
  error: string;
  /// The tab's view, drawn once and kept while it is open.
  el: HTMLElement | null;
  image?: { view: ImageView; w: number; h: number };
  model?: ModelView | null;
  wire?: boolean;
  bytes?: ArrayBuffer;
  /// A text tab's path in the worktree it was opened from, that worktree's
  /// folder, and what was read of it.
  rel?: string;
  root?: string;
  text?: TreeText;
  preview?: { active: boolean; loading: boolean; src: string; error: string };
  /// A terminal tab's terminal, once it started.
  term?: PaneTerm;
  /// Where a new terminal tab starts its shell (its machine and folder stay
  /// those when the pane's session changes), or the terminal already
  /// running that the tab follows.
  at?: Omit<TermPlace, "name">;
  existing?: TerminalRow;
  /// Its tab was closed: the terminal is being stopped, or is stopped once
  /// it started.
  closed?: boolean;
}

/// The session the pane belongs to.
interface PaneSession {
  id: string;
  machine_id: string;
  machine_name: string;
  cwd: string;
  agent: string;
  external?: boolean;
  /// The project it belongs to, which new terminals are listed under.
  project?: string;
}

const SUBS = "subagents";
const FILES = "files";
const CHANGES = "changes";
const ARTS = "artifacts";
const RUNNING = "running";
const KEY = `beep.mediaPane.${PANE ? "window" : "bar"}`;
const ARROW_STEP = 24;
/// How far a tab moves under the pointer before it drags.
export const DRAG_START = 5;

export const P = {
  session: null as PaneSession | null,
  tabs: [] as MediaTab[],
  /// The tab on screen in each dock: a tab's id, `FILES`, `ARTS` or `SUBS`.
  on: { right: FILES, bottom: FILES } as Record<Dock, string>,
  /// The subagents' rows and how many, as the log last drew them.
  subs: [] as SubagentRow[],
  subCount: 0,
  layout: loadLayout(),
  /// Tabs moved to a dock whose move is not kept (a picture by its data URL).
  moved: new Map<string, Dock>(),
  watch: null as ResizeObserver | null,
  /// A tab drag just ended: the click that follows it does nothing.
  dragged: false,
  /// The dock zoomed over the whole session by ⇧esc, remembered per session.
  zoom: null as Dock | null,
};

function loadLayout(): PaneLayout {
  try { return readLayout(localStorage.getItem(KEY)); } catch { return readLayout(null); }
}

export function saveLayout() {
  try { localStorage.setItem(KEY, JSON.stringify(P.layout)); } catch { /* kept while the window is open */ }
  rememberPane();
}

type Fixed = "subagents" | "files" | "changes" | "artifacts" | "running";

const ICONS: Record<TabKind | Fixed, string> = {
  // files.ts imports this module, so its folder icon is repeated here
  // rather than read before that module has run.
  files: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
  changes: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 4v8M8 8h8M8 18h8"/></svg>',
  text: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 7l-5 5 5 5M16 7l5 5-5 5"/></svg>',
  term: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>',
  image: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 17l-5-5-8 8"/></svg>',
  model: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 2l9 5v10l-9 5-9-5V7z"/><path d="M12 22V12M3 7l9 5 9-5"/></svg>',
  video: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="5" width="13" height="14" rx="2"/><path d="M16 10l5-3v10l-5-3"/></svg>',
  pdf: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/></svg>',
  artifacts: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18M9 21V9"/></svg>',
  running: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 17l6-6-6-6M12 19h8"/></svg>',
  subagents: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V13a5 5 0 0 0 5 5h4.5"/></svg>',
};
export const DOCK_RIGHT = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/></svg>';
export const DOCK_BOTTOM = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 14h18"/></svg>';
const FOLD_RIGHT = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>';
const FOLD_DOWN = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>';

// ----- The pane's place -----

export const shell = () => document.getElementById("agshell");
/// The conversation and the bottom dock under it.
export const main = () => document.getElementById("agmain");
export const paneOf = (d: Dock) => document.getElementById(`agpane-${d}`);

/// Starts the pane for session `s`, closing what the last one had open.
export function startPane(s: PaneSession) {
  closePaneTabs();
  P.session = s;
  paneReleased = true;
  P.on = { right: FILES, bottom: FILES };
  const saved = readView(viewKey(s.machine_id, s.id));
  P.layout = loadLayout();
  if (saved && typeof saved === "object" && "layout" in saved) P.layout = readLayout(JSON.stringify(saved.layout));
  P.subs = [];
  P.subCount = 0;
  P.moved.clear();
  P.zoom = null;
  const el = shell();
  if (!el) return;
  P.watch?.disconnect();
  // Too narrow for a right dock, its tabs join the bottom one, and back.
  P.watch = frameResize(() => el.classList.contains("narrow") !== (el.clientWidth < SIDE_BY_SIDE_MIN) ? draw() : place());
  P.watch.observe(el);

  for (const d of DOCKS) {
    wireDivider(d);
    const pane = paneOf(d)!;
    pane.addEventListener("click", e => paneClick(e, d));
    pane.addEventListener("contextmenu", tabMenu);
    // A viewer may keep the pointer to itself; ⌘W still needs the pane focused.
    pane.addEventListener("pointerdown", () => { if (!pane.contains(document.activeElement)) pane.focus({ preventScroll: true }); }, true);
    pane.querySelector(".agtabs")!.addEventListener("pointerdown", e => tabDown(e as PointerEvent));
    document.getElementById(`agpstrip-${d}`)!.addEventListener("click", () => { P.layout.hidden[d] = false; saveLayout(); draw(); });
  }
  readWorktrees(s);
  if (saved && typeof saved === "object" && "tabs" in saved && Array.isArray(saved.tabs)) {
    for (const tab of saved.tabs) {
      if (!tab || typeof tab !== "object") continue;
      if (typeof tab.root === "string" && typeof tab.rel === "string") {
        openIn(tab.root, tab.rel);
        if (tab.preview === true) {
          const opened = P.tabs.at(-1);
          if (opened) previewTab(opened);
        }
      } else if (typeof tab.id === "string" && "terminal" in tab) {
        const row = readTerminal(tab.terminal);
        if (row) open({ id: tab.id, kind: "term", name: typeof tab.name === "string" ? tab.name : row.name, path: null, url: null, existing: row });
        else {
          const pending = startingTerminals.get(viewKey(s.machine_id, s.id))?.find(t => t.id === tab.id);
          if (pending) P.tabs.push(pending);
        }
      } else if (typeof tab.path === "string") openPath(tab.path);
      else if (typeof tab.url === "string" && tab.url.startsWith("data:image/") && typeof tab.name === "string") openPicture(tab.url, tab.name);
    }
    if ("on" in saved && saved.on && typeof saved.on === "object") {
      for (const dock of DOCKS) if (dock in saved.on) {
        const id: unknown = Reflect.get(saved.on, dock);
        if (typeof id === "string" && tabIds().includes(id)) P.on[dock] = id;
      }
    }
    if ("layout" in saved) P.layout = readLayout(JSON.stringify(saved.layout));
    if ("zoom" in saved && (saved.zoom === "right" || saved.zoom === "bottom")) P.zoom = saved.zoom;
  }
  draw();
  paneReleased = false;
  rememberPane();
}


/// Files and Changes read the folder of session `s`.
function readWorktrees(s: PaneSession) {
  dropChanges();
  startWorktrees(s, root => { if (root) { pointFilesAt(); reloadChanges(); } draw(); });
}

/// The pane's session changes with its frame as it is: the empty view's place
/// moves to another machine or checkout, or the session it starts takes over.
/// Terminal tabs stay as they are, running where they were opened; the tabs
/// that read files from the old machine go.
export function rebindPane(s: PaneSession) {
  const was = P.session;
  P.session = s;
  if (was && was.machine_id !== s.machine_id) for (const t of P.tabs.filter(x => x.kind !== "term")) closeTab(t.id);
  readWorktrees(s);
  pointFilesAt();
  reloadChanges();
  draw();
}

/// Remembers the pane before releasing its views. Terminals keep running.
let paneReleased = false;
const startingTerminals = new Map<string, MediaTab[]>();
function rememberPane() {
  if (paneReleased || !P.session) return;
  const key = viewKey(P.session.machine_id, P.session.id);
  const pending = P.tabs.filter(t => t.kind === "term" && !t.term && !t.existing && !t.closed);
  if (pending.length) startingTerminals.set(key, pending); else startingTerminals.delete(key);
  writeView(key, { on: P.on, layout: P.layout, zoom: P.zoom, tabs: P.tabs.filter(t => !t.closed).map(savedTab) });
}

function savedTab(t: MediaTab) {
  if (t.kind === "term") return { id: t.id, name: t.name, terminal: t.term?.row ?? t.existing ?? null };
  if (t.rel && t.root) return { root: t.root, rel: t.rel, preview: t.preview?.active === true };
  return { path: t.path, url: t.path ? null : t.url, name: t.name };
}

function readTerminal(value: unknown): TerminalRow | null {
  if (!value || typeof value !== "object") return null;
  if (!("id" in value && typeof value.id === "string"
    && "project" in value && typeof value.project === "string"
    && "machine_id" in value && typeof value.machine_id === "string"
    && "machine_name" in value && typeof value.machine_name === "string"
    && "name" in value && typeof value.name === "string"
    && "command" in value && typeof value.command === "string"
    && "cwd" in value && typeof value.cwd === "string"
    && "state" in value && typeof value.state === "string"
    && "code" in value && (value.code === null || typeof value.code === "number")
    && "started_at" in value && typeof value.started_at === "number")) return null;
  return { id: value.id, project: value.project, machine_id: value.machine_id, machine_name: value.machine_name,
    name: value.name, command: value.command, cwd: value.cwd, state: value.state, code: value.code, started_at: value.started_at };
}

/// A shell can finish starting after navigation released its tab.
export function rememberTerminal(key: string, tab: MediaTab, row: TerminalRow) {
  const pending = startingTerminals.get(key)?.filter(t => t !== tab) ?? [];
  if (pending.length) startingTerminals.set(key, pending); else startingTerminals.delete(key);
  tab.existing = row;
  if (P.tabs.includes(tab)) { rememberPane(); return; }
  if (tab.closed) return;
  const saved = readView(key);
  if (!saved || typeof saved !== "object" || !("tabs" in saved) || !Array.isArray(saved.tabs)) return;
  saved.tabs = saved.tabs.map(item => item && typeof item === "object" && item.id === tab.id ? savedTab(tab) : item);
  writeView(key, saved);
}

window.addEventListener("pagehide", rememberPane);

export function closePaneTabs() {
  rememberPane();
  paneReleased = true;
  for (const t of P.tabs) {
    dropTab(t);
    t.term = undefined;
  }

  P.tabs = [];
}

function dropTab(t: MediaTab) {
  t.el?.querySelector("video")?.pause();
  if (t.el) removeSessionPortal(t.el);
  if (t.term) dropTerm(t.term);
  const stage = t.el?.querySelector<HTMLElement>(".agpstage.model");
  if (stage) removeSessionPortal(stage);
  const controls = t.el?.querySelector<HTMLElement>(".anim");
  if (controls) removeSessionPortal(controls);
  t.model?.dispose();
  if (t.url?.startsWith("blob:")) URL.revokeObjectURL(t.url);
  t.el?.remove();
}

// Leaving the session view lets go of what its tabs hold.
changeListeners.push(() => { if (S.view !== "agent") { closePaneTabs(); dropChanges(); P.watch?.disconnect(); P.watch = null; } });

/// Every tab's id, in the order the tab strips show them.
function tabIds() {
  return [FILES, CHANGES, ...(runningCount() ? [RUNNING] : []), ...(artifactCount() ? [ARTS] : []), ...(P.subCount ? [SUBS] : []), ...P.tabs.map(t => t.id)];
}

/// The tabs in each dock, as the shell's width lets them sit.
function docked(): Record<Dock, string[]> {
  return splitTabs(P.layout, tabIds(), shell()?.clientWidth ?? SIDE_BY_SIDE_MIN, P.moved);
}

/// The dock tab `id` shows in now.
export function dockOf(id: string): Dock {
  return docked().bottom.includes(id) ? "bottom" : "right";
}

/// Lays the docks out for the shell's size: which show, their size,
/// folded or not. A dock with no tabs goes.
let paneWasShown = false;

function place() {
  const el = shell(), box = main();
  if (!el || !box) return;
  const narrow = el.clientWidth < SIDE_BY_SIDE_MIN;
  el.classList.toggle("narrow", narrow);
  const split = docked();

  for (const d of DOCKS) {
    const pane = paneOf(d), div = document.getElementById(`agdiv-${d}`), strip = document.getElementById(`agpstrip-${d}`);
    if (!pane || !div || !strip) continue;
    // The Files tab is always there while a session shows, so one dock is.
    const has = !!P.session && split[d].length > 0;
    pane.hidden = div.hidden = !has || P.layout.hidden[d];
    strip.hidden = !has || !P.layout.hidden[d];
    if (pane.hidden) continue;
    const total = d === "right" ? el.clientWidth : box.clientHeight;
    pane.style.flexBasis = `${paneSize(d, P.layout[d], total)}px`;
    div.setAttribute("aria-orientation", d === "right" ? "vertical" : "horizontal");
  }

  const shown = DOCKS.some(d => paneOf(d)?.hidden === false);
  if (shown && !paneWasShown) queueMicrotask(() => window.dispatchEvent(new Event("agent-pane-shown")));
  paneWasShown = shown;
  refreshTopicSubscriptions();
  // A zoomed dock that folded, emptied or moved away ends the zoom.
  if (P.zoom && paneOf(P.zoom)?.hidden !== false) P.zoom = null;
  el.classList.toggle("zoomed", !!P.zoom);
  for (const d of DOCKS) paneOf(d)?.classList.toggle("zoomed", P.zoom === d);
}

/// ⇧esc: the dock with focus, else the right one, else the bottom one, fills the session
/// over the conversation; again puts it back. Only classes change, so a model, a video
/// or a terminal keeps its state and fits the new size on its own.
function toggleZoom() {
  if (P.zoom) P.zoom = null;
  else {
    const shown = DOCKS.filter(d => paneOf(d)?.hidden === false);
    P.zoom = shown.find(d => paneOf(d)?.contains(document.activeElement)) ?? shown.find(d => d === "right") ?? shown[0] ?? null;
  }

  place();
  rememberPane();
  // From the input, the zoomed tab takes the keys: a terminal fills the view and is typed in.
  const pane = P.zoom && paneOf(P.zoom);

  if (pane && !pane.contains(document.activeElement)) {
    const term = P.tabs.find(t => t.id === P.on[P.zoom!])?.term;
    if (term) term.screen.focus(); else pane.focus({ preventScroll: true });
  }
}

/// View diff: the Changes tab zoomed over the conversation, the keyboard in the diff.
/// A folded dock opens; the layout is otherwise as it was.
export function showChanges() {
  if (!shell()) return;
  const d = dockOf(CHANGES);
  P.on[d] = CHANGES;
  P.layout.hidden[d] = false;
  shell()?.classList.remove("mb-log");
  draw();
  P.zoom = d;
  place();
  focusChanges();
  rememberPane();
}

/// Esc in the window with a zoomed or focused pane: the pane steps back and the
/// conversation's input takes the keys. False when no pane has them.
export function leavePane() {
  const focus = document.activeElement;
  if (!P.zoom && !focus?.closest(".agpane")) return false;
  P.zoom = null;
  place();
  rememberPane();
  const input = document.querySelector<HTMLElement>("#agmsg, #agdraft textarea");
  if (input) input.focus(); else (focus as HTMLElement).blur();
  return true;
}

// S.view stays "agent" while the center shows another page; only a session on screen zooms.
onZoomKey(() => S.view === "agent" && !!P.session && !!shell()?.clientWidth, toggleZoom);

// ⌃tab with focus in a dock: its next tab, round to the first; ⌃⇧tab the one before.
window.addEventListener("keydown", e => {
  if (e.key !== "Tab" || !e.ctrlKey || e.metaKey || e.altKey) return;
  const target = e.target as Element;
  const pane = target.closest<HTMLElement>(".agpane[data-dock]");
  if (!pane) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  const d = pane.dataset.dock as Dock, ids = docked()[d];
  const step = e.shiftKey ? ids.length - 1 : 1;
  showTab(d, ids[(ids.indexOf(P.on[d]) + step) % ids.length]);
}, true);

function wireDivider(dock: Dock) {
  const div = document.getElementById(`agdiv-${dock}`)!;
  const set = (size: number | null) => {
    const el = shell(), box = main();
    if (!el || !box) return;
    P.layout[dock] = size == null ? null : paneSize(dock, size, dock === "right" ? el.clientWidth : box.clientHeight);
    place();
  };
  dragHandle(div, () => {
    const el = shell()!, box = main()!;
    el.classList.add("resizing", `resizing-${dock}`);
    div.classList.add("active");
    const right = dock === "right";
    const r = (right ? el : box).getBoundingClientRect();
    return {
      move: ev => set(right ? r.right - ev.clientX : r.bottom - ev.clientY),
      end: () => {
        el.classList.remove("resizing", `resizing-${dock}`);
        div.classList.remove("active");
        saveLayout();
      },
    };
  });
  div.addEventListener("dblclick", () => { set(null); saveLayout(); });
  div.addEventListener("keydown", e => {
    const pane = paneOf(dock)!;
    const now = dock === "right" ? pane.offsetWidth : pane.offsetHeight;
    const grow: Record<string, number> = dock === "right" ? { ArrowLeft: 1, ArrowRight: -1 } : { ArrowUp: 1, ArrowDown: -1 };
    const by = grow[e.key];
    if (!by) return;
    e.preventDefault();
    e.stopPropagation();
    set(now + by * ARROW_STEP);
    saveLayout();
  });
}

// ----- Tabs -----

/// What the log has now for the Artifacts tab: its artifacts, and when
/// each turn started, by its message's place.
export interface PaneArtifacts {
  list: Artifact[];
  turnStart: (turn: number) => number | undefined;
}

/// Draws the pane again: its tabs and the one on screen.
export function drawPane(subs?: { rows: SubagentRow[]; count: number }, arts?: PaneArtifacts) {
  if (subs) { P.subs = subs.rows; P.subCount = subs.count; }
  if (arts) setArtifacts(arts.list, arts.turnStart);
  draw();
}

export function draw() {
  // A tab being dragged is not drawn over until it drops.
  if (drag?.ghost) return;
  const split = docked();
  for (const d of DOCKS) drawDock(d, split[d]);

  for (const d of DOCKS) {
    const host = paneOf(d)?.querySelector<HTMLElement>(".agwtw");
    if (host) drawWorktrees(host, P.on[d] === FILES || P.on[d] === CHANGES);
  }

  place();
  phoneTabs();
}

/// On a phone the session shows one pane at a time: Log, the conversation, or one of the
/// pane's tabs filling the screen. The tabs are the bottom dock's, whose own strip is
/// hidden by mobile.css; `mb-log` on the shell says the conversation is the one shown.
function phoneTabs() {
  const el = shell(), box = main();
  if (!el || !box || !document.body.classList.contains("mobile")) return;
  let nav = box.querySelector<HTMLElement>(":scope > #agmnav");

  if (!nav) {
    nav = document.createElement("nav");
    nav.id = "agmnav";
    nav.className = "agmnav";
    box.prepend(nav);
    el.classList.add("mb-log");
  }

  const on = el.classList.contains("mb-log") ? "log" : P.on.bottom;
  const tabs = [...(paneOf("bottom")?.querySelectorAll<HTMLElement>(".agtabs > [data-mtab]") ?? [])]
    .map(t => ({ id: t.dataset.mtab!, name: t.querySelector("span")?.textContent ?? "" }));
  setSessionPortal(nav, createElement(PhonePaneTabs, { tabs: [{ id: "log", name: "Log" }, ...tabs], selected: on, select: (id: string) => {
    el.classList.toggle("mb-log", id === "log");
    if (id !== "log") P.on.bottom = id;
    draw();
  } }));
}

/// Draws dock `d` with tabs `ids`, moving into it the views of tabs that
/// came from the other.
function drawDock(d: Dock, ids: string[]) {
  const pane = paneOf(d);
  const tabs = pane?.querySelector<HTMLElement>(".agtabs"), views = pane?.querySelector<HTMLElement>(".agpviews");
  if (!pane || !tabs || !views) return;
  // The tab on screen went: the newest one here.
  if (!ids.includes(P.on[d])) P.on[d] = ids.at(-1) ?? FILES;
  const on = P.on[d];
  const items: PaneTabItem[] = ids.map(id => {
    if (id === FILES) return { id, name: "Files", icon: ICONS.files, count: null, closeIcon: null };
    if (id === CHANGES) return { id, name: "Changes", icon: ICONS.changes, count: null, closeIcon: null };
    if (id === SUBS) return { id, name: "Subagents", icon: ICONS.subagents, count: P.subCount, closeIcon: null };
    if (id === RUNNING) return { id, name: "Running", icon: ICONS.running, count: runningCount(), closeIcon: null };
    if (id === ARTS) return { id, name: "Artifacts", icon: ICONS.artifacts, count: artifactCount(), closeIcon: null };
    const item = P.tabs.find(tab => tab.id === id)!;
    return { id, name: item.name, icon: ICONS[item.kind], count: null, closeIcon: ICON.x };
  });
  setSessionPortal(tabs, createElement(PaneTabs, { tabs: items, selected: on }));
  const bottom = d === "bottom";
  const dock = pane.querySelector<HTMLElement>('[data-pact="dock"]')!;
  dock.title = bottom ? "Dock these tabs on the right" : "Dock these tabs at the bottom";
  const fold = pane.querySelector<HTMLElement>('[data-pact="fold"]')!;
  const strip = document.getElementById(`agpstrip-${d}`)!;
  const shown = P.tabs.find(t => t.id === on);
  const fixed = on === RUNNING ? RUNNING : on === SUBS ? SUBS : on === ARTS ? ARTS : on === CHANGES ? CHANGES : FILES;
  const label = shown ? shown.name : { [RUNNING]: "Running", [SUBS]: "Subagents", [ARTS]: "Artifacts", [CHANGES]: "Changes", [FILES]: "Files" }[fixed];
  const icon = (html: string) => createElement("span", { style: { display: "contents" }, dangerouslySetInnerHTML: { __html: html } });
  setSessionChild(`dock-controls:${d}`, createElement(Fragment, null,
    createPortal(icon(bottom ? DOCK_RIGHT : DOCK_BOTTOM), dock, "dock"),
    createPortal(icon(bottom ? FOLD_DOWN : FOLD_RIGHT), fold, "fold"),
    createPortal(createElement(Fragment, null, icon(ICONS[shown?.kind ?? fixed]), createElement("span", null, label), createElement("small", null, ids.length)), strip, "strip")));
  // The Files view is made once per session, wherever it sits.
  const made = filesView();
  let filesEl = document.querySelector<HTMLElement>(".agpviews > .agpview.files");
  if (filesEl !== made) { filesEl?.remove(); filesEl = made; }

  if (ids.includes(FILES)) {
    if (filesEl.parentElement !== views) views.prepend(filesEl);
    filesEl.hidden = on !== FILES;
    if (on !== FILES) closeFilesMenu();
  }

  // The Changes view is made the first time it shows, as it runs git.
  const changesEl = on === CHANGES ? changesView() : madeChangesView();

  if (changesEl && ids.includes(CHANGES)) {
    if (changesEl.parentElement !== views) views.append(changesEl);
    changesEl.hidden = on !== CHANGES;
  }

  // So is the Artifacts view, while there are any.
  const artsEl = artifactsView();

  if (!artifactCount()) artsEl.remove();
  else if (ids.includes(ARTS)) {
    if (artsEl.parentElement !== views) views.append(artsEl);
    artsEl.hidden = on !== ARTS;
    showFrames(on === ARTS && !P.layout.hidden[d]);
    if (on === ARTS) checkArtifacts();
  }

  const runningEl = runningView();

  if (runningEl && ids.includes(RUNNING)) {
    if (runningEl.parentElement !== views) views.append(runningEl);
    runningEl.hidden = on !== RUNNING;
  }

  // Subagents redraw each time; a file's view is drawn once and kept.
  let subView = document.querySelector<HTMLElement>(".agpviews > .agpview.subs");

  if (!P.subCount && subView) { removeSessionPortal(subView); subView.remove(); }
  else if (ids.includes(SUBS)) {
    if (!subView) subView = h('<div class="agpview subs"></div>');
    if (subView.parentElement !== views) views.append(subView);
    setSessionPortal(subView, createElement(SubagentsView, { rows: P.subs }));
    subView.hidden = on !== SUBS;
  }

  for (const t of P.tabs) {
    if (!ids.includes(t.id)) continue;
    if (!t.el) {
      t.el = viewOf(t);
      if (P.session && t.text) rememberScroll(t.el, `${viewKey(P.session.machine_id, P.session.id)}:file:${t.id}`);
    }
    if (t.el.parentElement !== views) views.append(t.el);
    const hide = t.id !== on || P.layout.hidden[d];
    if (hide && !t.el.hidden) stopPlaying(t);
    if (!hide && t.el.hidden) showFrame(t);
    t.el.hidden = t.id !== on;
    if (!hide) t.model?.resize();
    if (!hide && t.term) requestAnimationFrame(() => {
      t.term?.screen.fit();
      // A refresh must not take the keys back from the conversation.
      // Check now, since focus may have changed while this frame waited.
      if (t.id === P.on[d] && !P.layout.hidden[d] && pane.isConnected && (document.activeElement === pane || t.term?.screen.focused())) t.term?.screen.focus();
    });
  }
}

/// Pauses a tab's video and its model's animation, whose frames would
/// otherwise keep drawing out of sight.
function stopPlaying(t: MediaTab) {
  t.el?.querySelector("video")?.pause();
  t.model?.anim?.play(false);
}

/// Loads a tab's video again where it stood: WebKit can lose its picture
/// while it is out of sight and show it dark, sound and controls still there.
function showFrame(t: MediaTab) {
  const video = t.el?.querySelector("video");
  if (!video || video.readyState === 0) return;
  const at = video.currentTime;
  video.addEventListener("loadedmetadata", () => { video.currentTime = at; }, { once: true });
  video.load();
}

// The window hid or the pane went: nothing plays on. Shown again, the
// video in view gets its picture back.
onShownChange(shown => {
  for (const t of P.tabs) {
    if (!shown) stopPlaying(t);
    else if (t.el && !t.el.hidden) showFrame(t);
  }
}, true);

function showTab(d: Dock, id: string) {
  // Shown again, the changes are read again.
  if (id === CHANGES && P.on[d] !== CHANGES && madeChangesView()) reloadChanges();
  P.on[d] = id;
  rememberPane();
  draw();
  paneOf(d)?.focus({ preventScroll: true });
}

/// The menu of a tab the user opened: close it, save its file, copy its path. The
/// fixed tabs (Files, Changes, Subagents, Artifacts) have none.
function tabMenu(e: MouseEvent) {
  const id = (e.target as HTMLElement).closest<HTMLElement>("[data-mtab]")?.dataset.mtab;
  const t = P.tabs.find(x => x.id === id);
  if (!t) return;
  const items: MenuItem[] = [];
  const note = (text: string) => { const m = t.el?.querySelector(".agpacts .meta"); if (m) m.textContent = text; };
  const path = t.kind === "text" ? t.rel : t.path;
  if (t.kind === "text" && t.rel && isHtmlFile(t.rel) && !t.text?.binary) items.push({ label: t.preview?.active ? "Source" : "Preview", on: () => { focusTab(t.id); if (t.preview?.active) t.el?.querySelector<HTMLButtonElement>("[data-preview]")?.click(); else previewTab(t); } });
  if (path) items.push({ label: "Copy path", on: () => void writeClipboard(path) });

  if (t.kind === "text" && t.rel) {
    const rel = t.rel;
    items.push({ label: "Save", on: () => saveWorkFile(rel, note, t.root) });
  } else if (t.path && P.session) {
    const { machine_id } = P.session, file = t.path;
    items.push({ label: "Save", on: () => void invoke<string>("agent_file_save", { machineId: machine_id, path: file })
      .then(to => { note(`Saved to Downloads as ${baseName(to)}`); return invoke("reveal_file", { path: to }); })
      .catch(err => note(readError(err))) });
  }

  items.push({ label: "Close", on: () => closeTab(t.id) });
  openMenu(e, t.name, items);
}

function paneClick(e: MouseEvent, d: Dock) {
  const target = e.target as HTMLElement;
  const close = target.closest<HTMLElement>("[data-mclose]");
  if (close) { e.stopPropagation(); return closeTab(close.dataset.mclose!); }
  const tab = target.closest<HTMLElement>("[data-mtab]");

  if (tab) {
    if (P.dragged) return;
    return showTab(d, tab.dataset.mtab!);
  }

  const act = target.closest<HTMLElement>("[data-pact]")?.dataset.pact;

  if (act === "dock") {
    // This dock's tabs go to the other, which shows the one on screen here.
    const to: Dock = d === "right" ? "bottom" : "right", on = P.on[d];
    for (const id of docked()[d]) P.layout = moveTab(P.layout, id, to);
    for (const id of P.moved.keys()) if (docked()[d].includes(id)) P.moved.set(id, to);
    P.layout.dock = to;
    P.on[to] = on;
    saveLayout();
    return draw();
  }

  if (act === "fold") { P.layout.hidden[d] = true; saveLayout(); return draw(); }
  if (act === "term") return newTerminal(d);
}

export function closeTab(id: string) {
  const i = P.tabs.findIndex(t => t.id === id);
  if (i < 0) return;
  // The tab goes once its terminal stopped, and says why when it could not.
  const tab = P.tabs[i];

  if (tab.term) {
    if (tab.closed) return;
    tab.closed = true;
    const term = tab.term;
    setTermNote(tab, "Closing…");
    killTerm(term).then(() => {
      if (tab.term !== term) return;
      tab.term = undefined;
      closeTab(id);
    }, e => {
      tab.closed = false;
      setTermNote(tab, `Could not stop it: ${e instanceof Error ? e.message : e}`);
    });
    return;
  }

  // A shell still starting is stopped once it is there.
  if (tab.kind === "term") tab.closed = true;
  const d = dockOf(id);
  dropTab(P.tabs[i]);
  P.tabs.splice(i, 1);
  P.moved.delete(id);

  // The neighbour in the same dock shows next.
  if (P.on[d] === id) {
    const left = docked()[d];
    P.on[d] = P.tabs.slice(i).find(t => left.includes(t.id))?.id ?? left.at(-1) ?? FILES;
  }

  rememberPane();
  draw();
}

function setTermNote(t: MediaTab, text: string) {
  drawTermFooter(t, text);
}

/// Closes the tab on screen in the focused pane (⌘W); false when no pane is
/// focused or that tab stays, as Files does. A terminal's tab ends the terminal.
export function closeFocusedTab() {
  const pane = document.activeElement?.closest<HTMLElement>(".agpane");
  const tab = pane && P.tabs.find(t => t.id === P.on[pane.dataset.dock as Dock]);
  if (!pane || !tab) return false;
  closeTab(tab.id);
  pane.focus({ preventScroll: true });
  return true;
}

/// Shows the Files tab, unfolding its dock.
export function showFiles() {
  const d = dockOf(FILES);
  P.on[d] = FILES;
  P.layout.hidden[d] = false;
  draw();
}

/// Shows the Artifacts tab, unfolding its dock, on the artifact at `path`
/// when given. False when the tab does not list it.
export function showArtifact(path?: string) {
  if (path && !pickArtifact(path)) return false;
  if (!artifactCount()) return false;
  const d = dockOf(ARTS);
  P.on[d] = ARTS;
  P.layout.hidden[d] = false;
  saveLayout();
  draw();
  return true;
}

/// Opens a picture the log already shows (a data URL) in its own tab.
export function openPicture(url: string, name: string) {
  open({ id: url, kind: "image", name: baseName(name), path: null, url });
}

/// Opens the file at `path` (relative to the session's folder, absolute, or
/// under `~/`) on the session's machine, when it is one the pane shows.
export function openPath(path: string) {
  const kind = mediaKind(path);
  if (!kind || !P.session) return false;
  const full = resolvePath(P.session.cwd, path);
  open({ id: full, kind, name: baseName(full), path: full, url: null });
  return true;
}

export function open(t: Pick<MediaTab, "id" | "kind" | "name" | "path" | "url"> & Partial<MediaTab>) {
  if (!P.tabs.some(x => x.id === t.id)) {
    const tab: MediaTab = { size: 0, done: 0, error: "", el: null, ...t };
    P.tabs.push(tab);

    if (!tab.error) {
      if (tab.kind === "text") loadText(tab);
      else if (tab.kind === "term") startTerminal(tab);
      else if (!tab.url) load(tab);
    }
  }

  focusTab(t.id);
}

/// Shows tab `id`, unfolding its dock, and gives the pane the keys.
function focusTab(id: string) {
  const d = dockOf(id);
  P.layout.hidden[d] = false;
  saveLayout();
  P.on[d] = id;
  rememberPane();
  draw();
  paneOf(d)?.focus({ preventScroll: true });
}

/// ⌘2, ⌘3, ⌘4: the pane's Files, Changes or Artifacts tab (`n` from 0). False
/// when there is no pane, or no artifact to list.
export function showPaneTab(n: number) {
  const id = [FILES, CHANGES, ARTS][n];
  if (!id || !shell() || (id === ARTS && !artifactCount())) return false;
  const d = dockOf(id);
  P.layout.hidden[d] = false;
  if (P.zoom) P.zoom = d;
  saveLayout();
  showTab(d, id);
  return true;
}

/// Shows terminal `row`, one that runs on its machine, as a tab of the pane,
/// with the keyboard in it.
export function openTerminal(row: TerminalRow) {
  const have = P.tabs.find(t => t.term?.row.id === row.id || t.existing?.id === row.id);
  if (have) return focusTab(have.id);
  open({ id: `term:${row.id}`, kind: "term", name: row.name, path: null, url: null, existing: row });
}

/// The session the pane shows, while one does.
export function paneSession(): Readonly<PaneSession> | null {
  return P.session;
}

/// Opens `rel`, a file in the session's folder, from the Files tab: a
/// picture, video, 3D model or PDF in its viewer, anything else as text.
export function openWorkFile(rel: string) {
  openIn(paneRoot(), rel);
}

/// Opens an HTML file directly in its preview.
export function previewWorkFile(rel: string) {
  const root = paneRoot();
  openIn(root, rel);
  const id = root === P.session?.cwd ? `text:${rel}` : `text:${root}\n${rel}`;
  const tab = P.tabs.find(t => t.id === id);
  if (tab) previewTab(tab);
}

function previewTab(t: MediaTab) {
  if (t.preview?.active) return;
  if (!t.text) { queuePreview(t); return; }
  t.el?.querySelector<HTMLButtonElement>("[data-preview]")?.click();
}

/// Opens `rel`, a file in the worktree at `root`.
function openIn(root: string, rel: string) {
  if (openPath(rel.startsWith("/") ? rel : resolvePath(root, rel))) return;
  // A file of the session's own folder keeps its tab through a switch.
  const id = root === P.session?.cwd ? `text:${rel}` : `text:${root}\n${rel}`;
  open({ id, kind: "text", name: baseName(rel), path: null, url: null, rel, root });
}

/// Opens the file a link in a reply points at. One outside the session's
/// folder opens as a tab saying the node would not read it.
export function openLinked(path: string) {
  if (openPath(path) || !P.session) return;
  const cwd = P.session.cwd.replace(/\/+$/, "");
  const full = resolvePath(cwd, path);
  openIn(P.session.cwd, full.startsWith(`${cwd}/`) ? full.slice(cwd.length + 1) : full);
}

/// Saves `rel`, a file in the worktree at `root` (the one shown when
/// none is given), to Downloads; `note` hears how it went.
export function saveWorkFile(rel: string, note: (text: string) => void, root = paneRoot()) {
  const s = P.session;
  if (!s) return;
  note(`Saving ${baseName(rel)}…`);
  invoke<string>("agent_tree_save", { machineId: s.machine_id, root, path: rel })
    .then(to => { note(`Saved to Downloads as ${baseName(to)}`); invoke("reveal_file", { path: to }).catch(() => {}); })
    .catch(e => note(readError(e)));
}

/// Opens a terminal tab on the pane's machine, in its folder (⌘T, ⌃`), in
/// dock `d`, else where the newest terminal sits. With no machine to run it
/// the tab says so. `zoom` fills the view with it, as ⇧esc does. `like`
/// gives another machine and folder, such as those of a terminal on screen.
export function newTerminal(d?: Dock, zoom = false, like?: Omit<TermPlace, "name">) {
  const s = P.session;
  if (!s) return;
  const terms = P.tabs.filter(t => t.kind === "term");
  const root = like?.cwd ?? paneRoot();
  const name = terminalName(root, terms.map(t => t.name));
  const id = `term:${name}`;
  // While the window is too narrow for a right dock, every tab shows at
  // the bottom and a new one keeps the dock it would have.
  const narrow = (shell()?.clientWidth ?? SIDE_BY_SIDE_MIN) < SIDE_BY_SIDE_MIN;
  const newest = terms.at(-1);
  const to = d && !narrow ? d : newest ? tabDock(P.layout, newest.id) : tabDock(P.layout, id);
  if (to !== tabDock(P.layout, id)) P.layout = moveTab(P.layout, id, to);
  const at = like ?? { machine_id: s.machine_id, machine_name: s.machine_name, cwd: root, project: s.project ?? "" };
  open({ id, kind: "term", name, path: null, url: null, at, error: at.machine_id ? "" : "No machine has a checkout of this project to open a terminal in." });
  if (zoom) { P.zoom = dockOf(id); place(); }
}

/// ⌘N with a terminal tab on screen: a new one on its machine, in its folder,
/// in its dock. False when no terminal shows.
export function newTerminalLike() {
  if (!shell()) return false;
  const shown = DOCKS.filter(d => paneOf(d)?.hidden === false).flatMap(d => {
    const tab = P.tabs.find(t => t.id === P.on[d] && t.kind === "term");
    const at = tab && (tab.term?.row ?? tab.existing ?? tab.at);
    return at ? [{ dock: d, machine_id: at.machine_id, machine_name: at.machine_name, cwd: at.cwd, project: at.project, focused: !!tab.term?.screen.focused() }] : [];
  });
  const like = newTermLike(shown);
  if (!like) return false;
  const { dock, focused: _, ...at } = like;
  newTerminal(dock, false, at);
  return true;
}
