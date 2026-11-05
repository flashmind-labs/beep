// agentpane/: a coding agent session's side pane and its tabs (media,
// files, changes, artifacts, terminal). Entry: this file.
// The media pane of a coding agent session: pictures, videos, 3D models and
// PDFs the agent made or looked at, its files, artifacts, terminals and
// subagents, each a tab.
import { baseName, type Dock, DOCKS, type ImageView, isZoomKey, type MediaKind, mediaKind, moveTab, type PaneLayout, paneSize, readLayout, resolvePath, SIDE_BY_SIDE_MIN, splitTabs, tabDock } from "./media-state.ts";
import { terminalName } from "./files-state.ts";
import type { Artifact } from "./artifacts-state.ts";
import type { ModelView } from "./model3d.ts";
import { artifactCount, artifactsView, pickArtifact, setArtifacts } from "./artifacts.ts";
import { changeListeners, PANE, S } from "../bar/core.ts";
import { changesView, dropChanges, focusChanges, madeChangesView, reloadChanges } from "./changes.ts";
import { closeFilesMenu, filesView, pointFilesAt } from "./files.ts";
import { drawWorktrees, paneRoot, startWorktrees } from "./worktree.ts";
import { dropTerm, type PaneTerm } from "./terminal.ts";
import { drag, tabDown } from "./drag.ts";
import { readError, loadText, startTerminal, load } from "./read.ts";
import { viewOf } from "./views.ts";
import { esc } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { ICON } from "../ui/icons.ts";
import { invoke } from "../ui/tauri.ts";

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
  /// A terminal tab's terminal, once it started.
  term?: PaneTerm;
}

/// The session the pane belongs to.
interface PaneSession {
  id: string;
  machine_id: string;
  machine_name: string;
  cwd: string;
}

const SUBS = "subagents";
const FILES = "files";
const CHANGES = "changes";
const ARTS = "artifacts";
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
  subs: "",
  subCount: 0,
  layout: loadLayout(),
  /// Tabs moved to a dock whose move is not kept (a picture by its data URL).
  moved: new Map<string, Dock>(),
  watch: null as ResizeObserver | null,
  /// A tab drag just ended: the click that follows it does nothing.
  dragged: false,
  /// The dock zoomed over the whole session by ⇧esc. Not kept: a new session or a
  /// restart starts unzoomed.
  zoom: null as Dock | null,
};

function loadLayout(): PaneLayout {
  try { return readLayout(localStorage.getItem(KEY)); } catch { return readLayout(null); }
}

export function saveLayout() {
  try { localStorage.setItem(KEY, JSON.stringify(P.layout)); } catch { /* kept while the window is open */ }
}

type Fixed = "subagents" | "files" | "changes" | "artifacts";

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

/// One dock's frame: its divider, its pane and the strip it folds to.
function dockFrame(d: Dock) {
  return `<div class="agdiv" id="agdiv-${d}" data-dock="${d}" role="separator" tabindex="0" title="Drag to resize, double-click to reset" hidden></div>`
    + `<aside class="agpane" id="agpane-${d}" data-dock="${d}" tabindex="-1" hidden><div class="agphead"><div class="agtabs" role="tablist"></div>`
    + `<span class="agwtw" hidden></span><button class="agpbtn agaddterm" data-pact="term" title="New terminal in this folder (⌃\`)">+ Terminal</button><button class="agpbtn" data-pact="dock"></button><button class="agpbtn" data-pact="fold" title="Hide the pane"></button></div><div class="agpviews"></div></aside>`
    + `<button class="agpstrip" id="agpstrip-${d}" data-dock="${d}" hidden title="Show the pane"></button>`;
}

/// The session's frame around `conv`, the conversation: the bottom dock
/// under it, the right dock beside both, and where a dragged tab drops.
export function paneFrame(conv: string) {
  return `<div class="agmain" id="agmain">${conv}${dockFrame("bottom")}</div>${dockFrame("right")}`
    + `<div class="agdropzone" id="agdropzone" hidden></div>`;
}

/// Starts the pane for session `s`, closing what the last one had open.
export function startPane(s: PaneSession) {
  closeAll();
  P.session = s;
  P.on = { right: FILES, bottom: FILES };
  dropChanges();
  startWorktrees(s, root => { if (root) { pointFilesAt(); reloadChanges(); } draw(); });
  P.subs = "";
  P.subCount = 0;
  P.moved.clear();
  P.zoom = null;
  const el = shell();
  if (!el) return;
  P.watch?.disconnect();
  // Too narrow for a right dock, its tabs join the bottom one, and back.
  P.watch = new ResizeObserver(() => el.classList.contains("narrow") !== (el.clientWidth < SIDE_BY_SIDE_MIN) ? draw() : place());
  P.watch.observe(el);
  for (const d of DOCKS) {
    wireDivider(d);
    const pane = paneOf(d)!;
    pane.addEventListener("click", e => paneClick(e, d));
    // A viewer may keep the pointer to itself; ⌘W still needs the pane focused.
    pane.addEventListener("pointerdown", () => { if (!pane.contains(document.activeElement)) pane.focus({ preventScroll: true }); }, true);
    pane.querySelector(".agtabs")!.addEventListener("pointerdown", e => tabDown(e as PointerEvent));
    document.getElementById(`agpstrip-${d}`)!.addEventListener("click", () => { P.layout.hidden[d] = false; saveLayout(); draw(); });
  }
}

/// Every open tab closed, its bytes let go.
function closeAll() {
  for (const t of P.tabs) dropTab(t);
  P.tabs = [];
}

function dropTab(t: MediaTab) {
  // A terminal keeps running on its machine; Home lists it.
  if (t.term) dropTerm(t.term);
  t.model?.dispose();
  t.el?.querySelector("video")?.pause();
  if (t.url?.startsWith("blob:")) URL.revokeObjectURL(t.url);
  t.el?.remove();
}

// Leaving the session view lets go of what its tabs hold.
changeListeners.push(() => { if (S.view !== "agent") { closeAll(); dropChanges(); P.watch?.disconnect(); P.watch = null; } });

/// Every tab's id, in the order the tab strips show them.
function tabIds() {
  return [FILES, CHANGES, ...(artifactCount() ? [ARTS] : []), ...(P.subCount ? [SUBS] : []), ...P.tabs.map(t => t.id)];
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
}

/// View diff: the Changes tab zoomed over the conversation, the keyboard in the diff.
/// A folded dock opens; the layout is otherwise as it was.
export function showChanges() {
  if (!shell()) return;
  const d = dockOf(CHANGES);
  P.on[d] = CHANGES;
  P.layout.hidden[d] = false;
  draw();
  P.zoom = d;
  place();
  focusChanges();
}

/// Esc in the window with a zoomed or focused pane: the pane steps back and the
/// conversation's input takes the keys. False when no pane has them.
export function leavePane() {
  const focus = document.activeElement;
  if (!P.zoom && !focus?.closest(".agpane")) return false;
  P.zoom = null;
  place();
  const input = document.getElementById("agmsg");
  if (input) input.focus(); else (focus as HTMLElement).blur();
  return true;
}

// In the capture phase so it comes before the terminal's, a canvas's and every Esc
// handler, none of which should see it.
window.addEventListener("keydown", e => {
  if (!isZoomKey(e)) return;
  // S.view stays "agent" while the center shows another page; only a session on screen zooms.
  if (S.view !== "agent" || !P.session || !shell()?.clientWidth) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  if (!e.repeat) toggleZoom();
}, true);

// ⇧tab with focus in a dock: its next tab, round to the first. A terminal keeps the
// key, and in the message field it is the mode's.
window.addEventListener("keydown", e => {
  if (e.key !== "Tab" || !e.shiftKey || e.metaKey || e.ctrlKey || e.altKey) return;
  const target = e.target as Element;
  const pane = target.closest<HTMLElement>(".agpane[data-dock]");
  if (!pane || target.closest(".tm-xterm")) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  const d = pane.dataset.dock as Dock, ids = docked()[d];
  showTab(d, ids[(ids.indexOf(P.on[d]) + 1) % ids.length]);
  pane.focus({ preventScroll: true });
}, true);

function wireDivider(dock: Dock) {
  const div = document.getElementById(`agdiv-${dock}`)!;
  const set = (size: number | null) => {
    const el = shell(), box = main();
    if (!el || !box) return;
    P.layout[dock] = size == null ? null : paneSize(dock, size, dock === "right" ? el.clientWidth : box.clientHeight);
    place();
  };
  div.addEventListener("pointerdown", e => {
    if (e.button !== 0) return;
    e.preventDefault();
    div.setPointerCapture(e.pointerId);
    const el = shell()!, box = main()!;
    el.classList.add("resizing", `resizing-${dock}`);
    div.classList.add("active");
    const move = (ev: PointerEvent) => {
      const r = (dock === "right" ? el : box).getBoundingClientRect();
      set(dock === "right" ? r.right - ev.clientX : r.bottom - ev.clientY);
    };
    const up = () => {
      div.removeEventListener("pointermove", move);
      div.removeEventListener("pointerup", up);
      div.removeEventListener("pointercancel", up);
      el.classList.remove("resizing", `resizing-${dock}`);
      div.classList.remove("active");
      saveLayout();
    };
    div.addEventListener("pointermove", move);
    div.addEventListener("pointerup", up);
    div.addEventListener("pointercancel", up);
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
export function drawPane(subs?: { html: string; count: number }, arts?: PaneArtifacts) {
  if (subs) { P.subs = subs.html; P.subCount = subs.count; }
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
  const tab = (id: string, kind: TabKind | Fixed, name: string, extra: string) =>
    `<button class="agtab${on === id ? " on" : ""}" role="tab" aria-selected="${on === id}" data-mtab="${esc(id)}" title="${esc(name)}">${ICONS[kind]}<span>${esc(name)}</span>${extra}</button>`;
  tabs.innerHTML = ids.map(id => {
    if (id === FILES) return tab(FILES, FILES, "Files", "");
    if (id === CHANGES) return tab(CHANGES, CHANGES, "Changes", "");
    if (id === SUBS) return tab(SUBS, SUBS, "Subagents", `<small>${P.subCount}</small>`);
    if (id === ARTS) return tab(ARTS, ARTS, "Artifacts", `<small>${artifactCount()}</small>`);
    const t = P.tabs.find(x => x.id === id)!;
    return tab(t.id, t.kind, t.name, `<em data-mclose="${esc(t.id)}" aria-label="Close ${esc(t.name)}" title="Close (⌘W)">${ICON.x}</em>`);
  }).join("");
  const bottom = d === "bottom";
  const dock = pane.querySelector<HTMLElement>('[data-pact="dock"]')!;
  dock.innerHTML = bottom ? DOCK_RIGHT : DOCK_BOTTOM;
  dock.title = bottom ? "Dock these tabs on the right" : "Dock these tabs at the bottom";
  pane.querySelector('[data-pact="fold"]')!.innerHTML = bottom ? FOLD_DOWN : FOLD_RIGHT;
  const strip = document.getElementById(`agpstrip-${d}`)!;
  const shown = P.tabs.find(t => t.id === on);
  const fixed = on === SUBS ? SUBS : on === ARTS ? ARTS : on === CHANGES ? CHANGES : FILES;
  const label = shown ? shown.name : { [SUBS]: "Subagents", [ARTS]: "Artifacts", [CHANGES]: "Changes", [FILES]: "Files" }[fixed];
  strip.innerHTML = `${ICONS[shown?.kind ?? fixed]}<span>${esc(label)}</span><small>${ids.length}</small>`;
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
  }
  // Subagents redraw each time; a file's view is drawn once and kept.
  let subView = document.querySelector<HTMLElement>(".agpviews > .agpview.subs");
  if (!P.subCount) subView?.remove();
  else if (ids.includes(SUBS)) {
    if (!subView) subView = h('<div class="agpview subs"></div>');
    if (subView.parentElement !== views) views.append(subView);
    if (subView.innerHTML !== P.subs) subView.innerHTML = P.subs;
    subView.hidden = on !== SUBS;
  }
  for (const t of P.tabs) {
    if (!ids.includes(t.id)) continue;
    if (!t.el) t.el = viewOf(t);
    if (t.el.parentElement !== views) views.append(t.el);
    const hide = t.id !== on || P.layout.hidden[d];
    if (hide && !t.el.hidden) {
      t.el.querySelector("video")?.pause();
      t.model?.anim?.play(false);
    }
    t.el.hidden = t.id !== on;
    if (!hide) t.model?.resize();
    if (!hide && t.term) requestAnimationFrame(() => { t.term?.screen.fit(); if (t.id === P.on[d]) t.term?.screen.focus(); });
  }
}

function showTab(d: Dock, id: string) {
  // Shown again, the changes are read again.
  if (id === CHANGES && P.on[d] !== CHANGES && madeChangesView()) reloadChanges();
  P.on[d] = id;
  draw();
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
  const d = dockOf(id);
  dropTab(P.tabs[i]);
  P.tabs.splice(i, 1);
  P.moved.delete(id);
  // The neighbour in the same dock shows next.
  if (P.on[d] === id) {
    const left = docked()[d];
    P.on[d] = P.tabs.slice(i).find(t => left.includes(t.id))?.id ?? left.at(-1) ?? FILES;
  }
  draw();
}

/// Closes the tab on screen in the focused pane (⌘W); false when no pane is
/// focused or that tab stays, as Files does. A terminal is not closed by a key.
export function closeFocusedTab() {
  const pane = document.activeElement?.closest<HTMLElement>(".agpane");
  const tab = pane && P.tabs.find(t => t.id === P.on[pane.dataset.dock as Dock]);
  if (!pane || !tab || tab.kind === "term") return false;
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
    const tab: MediaTab = { ...t, size: 0, done: 0, error: "", el: null };
    P.tabs.push(tab);
    if (tab.kind === "text") loadText(tab);
    else if (tab.kind === "term") startTerminal(tab);
    else if (!tab.url) load(tab);
  }
  const d = dockOf(t.id);
  P.layout.hidden[d] = false;
  saveLayout();
  P.on[d] = t.id;
  draw();
  paneOf(d)?.focus({ preventScroll: true });
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

/// Opens a terminal tab on the session's machine, in its folder (⌃`), in
/// dock `d`, else where the newest terminal sits.
export function newTerminal(d?: Dock) {
  const s = P.session;
  if (!s) return;
  const terms = P.tabs.filter(t => t.kind === "term");
  const name = terminalName(paneRoot(), terms.map(t => t.name));
  const id = `term:${name}`;
  // While the window is too narrow for a right dock, every tab shows at
  // the bottom and a new one keeps the dock it would have.
  const narrow = (shell()?.clientWidth ?? SIDE_BY_SIDE_MIN) < SIDE_BY_SIDE_MIN;
  const newest = terms.at(-1);
  const to = d && !narrow ? d : newest ? tabDock(P.layout, newest.id) : tabDock(P.layout, id);
  if (to !== tabDock(P.layout, id)) P.layout = moveTab(P.layout, id, to);
  open({ id, kind: "term", name, path: null, url: null });
}
