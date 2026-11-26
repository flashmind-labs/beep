// agentpane/: a coding agent session's side pane and its tabs (media,
// files, changes, artifacts, terminal). Entry: this file.
// The commands of the media pane of a coding agent session: starting it for a
// session, opening and closing its tabs, showing a tab. Its state is in
// pane-state.ts, its tabs' files and drawing in media-draw.ts and
// media-layout.ts.
import { viewKey } from "./view-memory.ts";
import { hasPreview } from "./html-preview-state.ts";
import { AGENTS, ARTS, CHANGES, DEVICE, FILES, RUNNING, SUBS, baseName, type Dock, DOCKS, moveTab, type PaneLayout, readLayout, SIDE_BY_SIDE_MIN, tabDock } from "./media-state.ts";
import { terminalName } from "./files-state.ts";
import { artifactCount, pickArtifact } from "./artifacts.ts";
import { changeListeners, PANE, S } from "../bar/core.ts";
import { revalidateChanges, writingComment } from "./changes.ts";
import { pointFilesAt } from "./files.ts";
import { paneRoot, startWorktrees } from "./worktree.ts";
import { unfollowFiles } from "./reload.ts";
import { type TermPlace } from "./terminal.ts";
import type { TerminalRow } from "../terminals/terminals.tsx";
import { newTermLike } from "../terminals/newkey-state.ts";

import { readingFrom } from "./read-file.ts";


import { frameResize } from "../ui/dom.ts";
import { draw, flipTab, pickPhoneTab, syncCounts } from "./media-draw.ts";
import { dockHolding, dockOf, docked, dockWidth, drawPaneNow, openTabs, paneModel, paneOf, place, setOn, shell, shellResized, shownDocks, watchShell, tabIds } from "./media-layout.ts";
import { loadLayout, memory, readTerminal, rememberPane, savedPane, saveLayout, startingTerminals } from "./pane-memory.ts";
import { P, type MediaTab, type PaneSession, type SplitSession } from "./pane-state.ts";
import { closeTab, dropTab, touchTab } from "./pane-tabs.ts";
import { addTab, focusTab, leaveLog, open, openIn, openPath, openPicture, previewTab, requestRightPane, setFolded } from "./pane-open.ts";
import { savedIn } from "../ui/platform.ts";

export { AGENTS, ARTS, CHANGES, DEVICE, FILES, RUNNING, SUBS };
/// Starts the pane for session `s`, closing what the last one had open.
export function startPane(s: PaneSession) {
  closePaneTabs();
  P.session = s;
  readingFrom.machine = s.machine_name;
  memory.released = true;
  const key = viewKey(s.machine_id, s.id);
  const saved = savedPane(key);
  const layout = saved && typeof saved === "object" && "layout" in saved ? readLayout(JSON.stringify(saved.layout)) : loadLayout();
  paneModel.setState({ on: { right: FILES, bottom: FILES }, layout: inBar(layout), zoom: null, wideFolded: null, phoneShown: false, subs: [], subCount: 0 });
  P.moved.clear();
  P.rightRequested = false;
  const el = shell();
  if (!el) return;
  P.watch?.disconnect();
  // Too narrow for the conversation beside it, the right pane folds to its strip, and back.
  P.watch = frameResize(() => {
    shellResized();
    if (paneModel.getState().narrow !== (dockWidth() < SIDE_BY_SIDE_MIN)) draw(); else place();
  });
  P.watch.observe(el);
  watchShell(el);
  readWorktrees(s);
  syncCounts();
  if (saved && typeof saved === "object" && "tabs" in saved && Array.isArray(saved.tabs)) {
    for (const tab of saved.tabs) {
      if (!tab || typeof tab !== "object") continue;
      if (typeof tab.root === "string" && typeof tab.rel === "string") {
        const opened = openIn(tab.root, tab.rel);
        if (opened && (tab.preview === true || (tab.preview == null && hasPreview(tab.rel)))) previewTab(opened);
      } else if (typeof tab.id === "string" && "terminal" in tab) {
        const row = readTerminal(tab.terminal);
        if (row) open({ id: tab.id, kind: "term", name: typeof tab.name === "string" ? tab.name : row.name, path: null, url: null, existing: row });
        else {
          const pending = startingTerminals.get(viewKey(s.machine_id, s.id))?.find(t => t.id === tab.id);
          if (pending) addTab(pending);
        }
      } else if (typeof tab.path === "string") openPath(tab.path);
      else if (typeof tab.url === "string" && tab.url.startsWith("data:image/") && typeof tab.name === "string") openPicture(tab.url, tab.name);
    }
    if ("on" in saved && saved.on && typeof saved.on === "object") {
      for (const dock of DOCKS) if (dock in saved.on) {
        const id: unknown = Reflect.get(saved.on, dock);
        if (typeof id === "string" && tabIds().includes(id)) setOn(dock, id);
      }
    }
    if ("layout" in saved) paneModel.setState({ layout: inBar(readLayout(JSON.stringify(saved.layout))) });
    if ("zoom" in saved && (saved.zoom === "right" || saved.zoom === "bottom")) paneModel.setState({ zoom: saved.zoom });
  }
  // Each tab reopened above took the phone's screen; a session still opens on its chat.
  paneModel.setState({ phoneShown: false });
  draw();
  memory.released = false;
  rememberPane();
}

/// The bar opens a session with its docks folded, the conversation first.
function inBar(layout: PaneLayout): PaneLayout {
  return PANE ? layout : { ...layout, hidden: { right: true, bottom: true } };
}

/// The pane's session changes with its frame as it is: the empty view's place
/// moves to another machine or checkout, or the session it starts takes over.
/// Terminal tabs stay as they are, running where they were opened; the tabs
/// that read files from the old machine go.
export function rebindPane(s: PaneSession) {
  const was = P.session;
  P.session = s;
  readingFrom.machine = s.machine_name;
  if (was && was.machine_id !== s.machine_id) for (const t of openTabs().filter(x => x.kind !== "term")) closeTab(t.id);
  readWorktrees(s);
  pointFilesAt();
  revalidateChanges();
  draw();
}

/// Reusing a conversation frame also restores views released when leaving it.
export function restorePane() {
  if (memory.released && P.session) startPane(P.session);
}

export function closePaneTabs() {
  rememberPane();
  memory.released = true;
  for (const t of openTabs()) {
    dropTab(t);
    t.term = undefined;
  }

  paneModel.setState({ tabs: [] });
}

export function openPreviewExport(path: string, blob: Blob, source: { tab: string; url: string }) {
  const name = baseName(path.replaceAll("\\", "/"));
  const id = `export:${path}`;
  if (openTabs().some(tab => tab.id === id)) return focusTab(id);

  const kind = blob.type === "application/pdf" ? "pdf" : "image";
  const url = kind === "image" ? URL.createObjectURL(blob) : null;
  P.moved.set(id, dockOf(source.tab));
  open({ id, kind, name, path: null, url, size: blob.size, note: `Saved to ${savedIn(path)}`, exported: { path, blob, source } });
}

/// ⌘2 to ⌘9: the pane's tab `n` (from 0), counting the right strip's tabs
/// then the bottom one's. False when there is no pane or no such tab.
export function showPaneTab(n: number) {
  const { right, bottom } = docked();
  const id = [...right, ...bottom][n];
  if (!id || !shell()) return false;
  const d = dockOf(id);
  setFolded(d, false);
  if (paneModel.getState().zoom) paneModel.setState({ zoom: d });
  saveLayout();
  showTab(d, id);
  return true;
}

/// Shows terminal `row`, one that runs on its machine, as a tab of the pane,
/// with the keyboard in it.
export function openTerminal(row: TerminalRow) {
  const have = openTabs().find(t => t.term?.row.id === row.id || t.existing?.id === row.id);
  if (have) return focusTab(have.id);
  open({ id: `term:${row.id}`, kind: "term", name: row.name, path: null, url: null, existing: row });
}

/// Opens a terminal tab on the pane's machine, in its folder (⌘T, ⌃`), in
/// dock `d`, else where the newest terminal sits. With no machine to run it
/// the tab says so. `zoom` fills the view with it, as ⇧esc does. `like`
/// gives another machine and folder, such as those of a terminal on screen.
export function newTerminal(d?: Dock, zoom = false, like?: Omit<TermPlace, "name">) {
  const s = P.session;
  if (!s) return;
  const terms = openTabs().filter(t => t.kind === "term");
  const root = like?.cwd ?? paneRoot();
  const name = terminalName(root, terms.map(t => t.name));
  const id = `term:${name}`;
  const newest = terms.at(-1);
  const { layout } = paneModel.getState();
  const to = d ? d : newest ? tabDock(layout, newest.id) : tabDock(layout, id);
  if (to !== tabDock(layout, id)) paneModel.setState({ layout: moveTab(layout, id, to) });
  const at = like ?? { machine_id: s.machine_id, machine_name: s.machine_name, cwd: root, project: s.project ?? "" };
  open({ id, kind: "term", name, path: null, url: null, at, error: at.machine_id ? "" : "No machine has a checkout of this project to open a terminal in." });
  if (zoom) { paneModel.setState({ zoom: dockOf(id) }); place(); }
}

/// ⌘N with a terminal tab on screen: a new one on its machine, in its folder,
/// in its dock. False when no terminal shows.
export function newTerminalLike() {
  if (!shell()) return false;
  const shown = shownDocks().flatMap(d => {
    const tab = openTabs().find(t => t.id === paneModel.getState().on[d] && t.kind === "term");
    const at = tab && (tab.term?.row ?? tab.existing ?? tab.at);
    return at ? [{ dock: d, machine_id: at.machine_id, machine_name: at.machine_name, cwd: at.cwd, project: at.project, focused: !!tab.term?.screen.focused() }] : [];
  });
  const like = newTermLike(shown);
  if (!like) return false;
  const { dock, focused: _, ...at } = like;
  newTerminal(dock, false, at);
  return true;
}

/// Closes the tab on screen in the focused pane (⌘W); false when no pane is
/// focused or that tab stays, as Files does. A terminal's tab ends the terminal.
export function closeFocusedTab() {
  const d = dockHolding(document.activeElement);
  const tab = d && openTabs().find(t => t.id === paneModel.getState().on[d]);
  if (!d || !tab) return false;
  closeTab(tab.id);
  paneOf(d)?.focus({ preventScroll: true });
  return true;
}

/// Opens coding agent session `s` as a live split tab in the right dock beside
/// the open conversation.
export function openSessionBeside(s: SplitSession, dock: Dock = "right") {
  if (!shell()) return;
  const id = `session:${s.machine_id}:${s.agent}:${s.id}`;
  const found = openTabs().find(t => t.id === id);
  if (found) {
    found.splitSession = s;
    found.name = s.title || s.agent || "Session";
    touchTab(found);
    const d = dockOf(id);
    setFolded(d, false);
    saveLayout();
    showTab(d, id);
    return;
  }

  const tab: MediaTab = {
    id, kind: "session", name: s.title || s.agent || "Session", path: null, url: null,
    size: 0, started: Date.now(), done: 0, error: "", rev: 0, viewHidden: false, note: null,
    player: null, playerAt: 0, splitSession: s,
  };
  addTab(tab);
  if (dock === "bottom") { paneModel.setState(s => ({ layout: moveTab(s.layout, id, "bottom") })); P.moved.set(id, "bottom"); }
  const d = dockOf(id);
  setFolded(d, false);
  setOn(d, id);
  saveLayout();
  draw();
}

/// Shows the Running tab, unfolding its dock; on a phone it fills the screen.
export const showRunning = () => showList(RUNNING);
/// Shows the Subagents tab the same way.
export const showSubagents = () => showList(SUBS);

function showList(id: typeof RUNNING | typeof SUBS) {
  syncCounts();
  if (!shell() || !tabIds().includes(id)) return;
  if (paneModel.getState().phone) return pickPhoneTab(id);

  const d = dockOf(id);
  setFolded(d, false);
  saveLayout();
  showTab(d, id);
}

/// Shows the Artifacts tab, unfolding its dock, on the artifact at `path`
/// when given. False when the tab does not list it.
export function showArtifact(path?: string) {
  if (path && !pickArtifact(path)) return false;
  if (!artifactCount()) return false;
  const d = dockOf(ARTS);
  leaveLog();
  requestRightPane(d);
  setOn(d, ARTS);
  setFolded(d, false);
  saveLayout();
  draw();
  return true;
}

export function showTab(d: Dock, id: string) {
  requestRightPane(d);
  setOn(d, id);
  rememberPane();
  if (!flipTab(d)) draw();
  drawPaneNow();
  paneOf(d)?.focus({ preventScroll: true });
}

/// Files and Changes read the folder of session `s`.
function readWorktrees(s: PaneSession) {
  startWorktrees(s, root => { if (root) { pointFilesAt(); revalidateChanges(); } draw(); }, writingComment);
}

// Leaving the session view lets go of what its tabs hold. Files and Changes
// keep their views of the last few folders, to show again as they were.
changeListeners.push(() => { if (S.view !== "agent") { closePaneTabs(); unfollowFiles(); P.watch?.disconnect(); P.watch = null; watchShell(null); } });
