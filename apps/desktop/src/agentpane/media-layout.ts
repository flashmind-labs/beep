// Where the pane's docks sit and how big they are: the split of tabs
// between them, zoom, ctrl-tab between tabs, and the dividers. The dock
// components (dock-view.tsx) draw from the model kept here.
import { createRef, type RefObject } from "react";
import { flushSync } from "react-dom";
import { createStore } from "zustand";
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { isMobile, onPhone } from "../ui/platform.ts";
import { type Box, type Dock, DOCKS, DEFAULT_LAYOUT, FILES, CHANGES, ARTS, RUNNING, SUBS, DEVICE, AGENTS, paneSize, type PaneLayout, SIDE_BY_SIDE_MIN, splitTabs, withFolded } from "./media-state.ts";
import type { PaneTabItem } from "./tabs-view.tsx";
import type { SubagentRow } from "./subagents-view.tsx";
import type { WorktreeMenuModel } from "./worktree-view.tsx";
import { PANE } from "../bar/core.ts";
import { P, type MediaTab } from "./pane-state.ts";
import { sessions } from "../agents/sessions.ts";
import { sessionVisible } from "../agents/closing.ts";
import { chatOf, chatRail } from "../agents/threads-state.ts";
import { shell as windowShell } from "../center/window-shell.tsx";

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
  const { counts, subCount, tabs } = paneModel.getState();
  return [FILES, ...(chat ? [] : [CHANGES]), ...(counts.running ? [RUNNING] : []), ...(counts.artifacts ? [ARTS] : []), ...(subCount && !chat ? [SUBS] : []), ...(chat && counts.agents ? [AGENTS] : []), ...(counts.device ? [DEVICE] : []), ...tabs.map(t => t.id)];
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
  if (!chat || P.rightRequested || paneModel.getState().entries || paneModel.getState().tabs.some(tab => tabs.includes(tab.id))) return true;

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
  return splitTabs(paneModel.getState().layout, tabIds(), dockWidth(), P.moved);
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

/// What the fixed tabs count: processes, artifacts and the agents of a chat, and
/// whether a device is there to show. Their owners give them to each draw.
export interface PaneCounts { running: number; artifacts: number; agents: number | null; device: boolean }

/// What a dock holds, worked out from the model: its tab strip, the views of
/// its tabs, and the worktree menu in its head while it shows Files or Changes.
export interface DockModel {
  tabs: PaneTabItem[];
  worktrees: WorktreeMenuModel | null;
  views: DockView[];
}

/// Where a dock sits as last laid out.
export interface DockPlace {
  /// The pane shows, or only its strip.
  shown: boolean;
  strip: boolean;
  /// Its size along the shell, once it was laid out.
  size: number | null;
}

/// A tab being dragged: its strip item, whether its dock shows it, and the
/// dock it would drop in, with where that dock is in the shell.
interface DragModel { item: PaneTabItem; selected: boolean; to: Dock | null; zone: Box | null }

export interface PaneModel {
  /// The tab on screen in each dock: a tab's id, `FILES`, `ARTS` or `SUBS`.
  on: Record<Dock, string>;
  layout: PaneLayout;
  /// The dock zoomed over the whole session by ⇧esc, remembered per session.
  zoom: Dock | null;
  /// The wide layout's fold of the right pane while a narrow window keeps it folded.
  wideFolded: boolean | null;
  /// The session drew on a phone, which shows the conversation first.
  phoneShown: boolean;
  /// The tabs the user opened, in the order they were opened. Replaced, never changed in place.
  tabs: MediaTab[];
  /// The subagents' rows and how many, as the log last drew them.
  subs: SubagentRow[];
  subCount: number;
  /// The tab ids in each dock as last laid out; null until the first layout.
  split: Record<Dock, string[]> | null;
  /// What the fixed tabs count, and whether Changes was made, as the last draw left them.
  counts: PaneCounts;
  changes: boolean;
  /// The working folder has something to show, as Files last read it.
  entries: boolean;
  /// The worktree menu of Files and Changes, as the last draw left it.
  worktrees: WorktreeMenuModel | null;
  places: Record<Dock, DockPlace>;
  narrow: boolean;
  /// The dock filling the session now: the zoomed one, or the right pane over a narrow window's conversation.
  zoomed: Dock | null;
  /// The chats list steps aside so the right pane fits beside the conversation.
  crowded: boolean;
  /// The divider being dragged.
  resizing: Dock | null;
  drag: DragModel | null;
  /// On a phone: its tabs, and whether the conversation is the one shown.
  phone: { tabs: { id: string; name: string }[]; selected: string } | null;
  log: boolean;
}

const HIDDEN: DockPlace = { shown: false, strip: false, size: null };

export const paneModel = createStore<PaneModel>(() => ({
  on: { right: FILES, bottom: FILES }, layout: { ...DEFAULT_LAYOUT }, zoom: null, wideFolded: null, phoneShown: false,
  tabs: [], subs: [], subCount: 0, split: null, counts: { running: 0, artifacts: 0, agents: null, device: false }, changes: false, entries: false, worktrees: null, places: { right: HIDDEN, bottom: HIDDEN }, narrow: false, zoomed: null, crowded: false, resizing: null, drag: null, phone: null, log: false,
}));

/// Sets part of the model; React draws it with the rest of the event.
export function setPaneModel(change: Partial<PaneModel>) {
  paneModel.setState(change);
}

/// Draws what the model changed now, for a caller that measures or focuses what it draws next.
export function drawPaneNow() {
  flushSync(() => {});
}

/// The tabs the user opened. They live in the pane's model, which draws them.
export const openTabs = () => paneModel.getState().tabs;

/// Shows tab `id` in dock `d` at the next draw.
export function setOn(d: Dock, id: string) {
  paneModel.setState(s => ({ on: { ...s.on, [d]: id } }));
}

/// Folds dock `d` to its strip, or shows it, at the next draw.
export function setHidden(d: Dock, folded: boolean) {
  paneModel.setState(s => ({ layout: withFolded(s.layout, d, folded) }));
}

/// Lays the docks out for the shell's size: which show, their size,
/// folded or not. A dock with no tabs goes.
let paneWasShown = false;

export function place(extra: Partial<PaneModel> = {}) {
  const el = shell(), box = main();
  if (!extra.split && !paneModel.getState().split) return;
  const split = extra.split ?? docked();
  if (!el || !box) { setPaneModel({ ...extra, split }); return; }
  const { width, height } = shellSize();
  const narrow = dockWidth() < SIDE_BY_SIDE_MIN;
  const over = foldNarrow();
  const { layout, zoom: asked } = paneModel.getState();
  const places = { ...paneModel.getState().places };
  const rightAvailable = rightPaneAvailable(split.right);

  for (const d of DOCKS) {
    const has = !!P.session && split[d].length > 0 && (d === "bottom" || rightAvailable);
    const shown = has && !layout.hidden[d];
    const total = d === "right" ? width : height;
    places[d] = { shown, strip: has && layout.hidden[d], size: shown ? paneSize(d, layout[d], total) : places[d].size };
  }

  const shown = DOCKS.some(d => places[d].shown);
  // A zoomed dock that folded, emptied or moved away ends the zoom.
  const zoom = asked && places[asked].shown ? asked : null;
  // On a narrow window the right pane, when open, covers the conversation. An assistant chat's
  // list steps aside instead, when that gives the pane room beside it.
  const crowded = chatsListCrowded(places.right.shown);
  const zoomed = over && places.right.shown && !crowded ? "right" : zoom;
  setPaneModel({ ...extra, split, places, zoom, narrow, zoomed, crowded });
  if (shown && !paneWasShown) queueMicrotask(() => window.dispatchEvent(new Event("agent-pane-shown")));
  paneWasShown = shown;
  refreshTopicSubscriptions();
}

/// A window too narrow for the conversation beside the right pane folds that pane to its
/// strip, keeping the wide layout's choice to put back once it is wide again. True while
/// the window is that narrow; a phone, which has no right pane, never is.
export function foldNarrow(): boolean {
  const width = dockWidth(), over = width > 0 && width < SIDE_BY_SIDE_MIN;
  const { layout, wideFolded } = paneModel.getState();
  if (over && wideFolded === null) paneModel.setState({ wideFolded: layout.hidden.right, layout: withFolded(layout, "right", true) });
  else if (!over && wideFolded !== null) paneModel.setState({ layout: withFolded(layout, "right", wideFolded), wideFolded: null });

  return over;
}

/// The docks on screen now, as last laid out.
export function shownDocks(): Dock[] {
  const { split, places } = paneModel.getState();
  return split ? DOCKS.filter(d => places[d].shown && !!paneOf(d)) : [];
}
