// agentpane/: a coding agent session's side pane and its tabs (media,
// files, changes, artifacts, terminal). Entry: this file.
// The commands of the media pane of a coding agent session: starting it for a
// session, opening and closing its tabs, showing a tab. Its state is in
// pane-state.ts, its tabs' files and drawing in media-draw.ts and
// media-layout.ts.
import { viewKey } from "./view-memory.ts";
import { hasPreview } from "./html-preview-state.ts";
import { AGENTS, ARTS, CHANGES, DEVICE, FILES, RUNNING, SUBS, baseName, type Dock, type FileRef, DOCKS, mediaKind, moveTab, type PaneLayout, readLayout, resolvePath, SIDE_BY_SIDE_MIN, tabDock } from "./media-state.ts";
import { terminalName } from "./files-state.ts";
import { artifactCount, pickArtifact } from "./artifacts.ts";
import { changeListeners, PANE, S } from "../bar/core.ts";
import { focusChanges, revalidateChanges, writingComment } from "./changes.ts";
import { pointFilesAt } from "./files.ts";
import { checkoutTop, paneRoot, startWorktrees } from "./worktree.ts";
import { unfollowFiles } from "./reload.ts";
import { type TermPlace } from "./terminal.ts";
import type { TerminalRow } from "../terminals/terminals.tsx";
import { newTermLike } from "../terminals/newkey-state.ts";
import { loadText, startTerminal, load } from "./read.ts";
import { readingFrom } from "./read-file.ts";
import { togglePreview } from "./text-view.tsx";
import type { LspLocation } from "./lsp-state.ts";
import { frameResize } from "../ui/dom.ts";
import { draw, flipTab, pickPhoneTab, syncCounts } from "./media-draw.ts";
import { desktopChatPane, dockHolding, dockOf, docked, dockWidth, drawPaneNow, openTabs, paneModel, paneOf, place, setHidden, setOn, shell, shellResized, shownDocks, watchShell, tabIds } from "./media-layout.ts";
import { loadLayout, memory, readTerminal, rememberPane, savedPane, saveLayout, startingTerminals } from "./pane-memory.ts";
import { P, newFileEdit, paneTarget, wantPane, type MediaTab, type PaneWant, type PaneSession, type SplitSession, type WorkFile } from "./pane-state.ts";
import { closeTab, dropTab, touchTab } from "./pane-tabs.ts";
import { onPhone, savedIn } from "../ui/platform.ts";

export { AGENTS, ARTS, CHANGES, DEVICE, FILES, RUNNING, SUBS };
function addTab(tab: MediaTab) {
  paneModel.setState(s => ({ tabs: [...s.tabs, tab] }));
}

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
  wantPane(null);
  rememberPane();
  memory.released = true;
  for (const t of openTabs()) {
    dropTab(t);
    t.term = undefined;
  }

  paneModel.setState({ tabs: [] });
}

/// Opens a picture the log already shows (a data URL) in its own tab.
export function openPicture(url: string, name: string) {
  open({ id: url, kind: "image", name: baseName(name), path: null, url });
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

/// Opens the file at `path` (relative to the session's folder, absolute, or
/// under `~/`) on the session's machine, when it is one the pane shows.
export function openPath(path: string) {
  const kind = mediaKind(path);
  if (!kind || !P.session) return false;
  const full = resolvePath(P.session.cwd, path);
  open({ id: full, kind, name: baseName(full), path: full, url: null });
  return true;
}

function open(t: Pick<MediaTab, "id" | "kind" | "name" | "path" | "url"> & Partial<MediaTab>) {
  if (!openTabs().some(x => x.id === t.id)) {
    const tab: MediaTab = { size: 0, started: Date.now(), done: 0, error: "", rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, ...t };
    addTab(tab);

    if (!tab.error) {
      if (tab.kind === "text") loadText(tab);
      else if (tab.kind === "term") startTerminal(tab);
      else if (!tab.url && !tab.exported) load(tab);
    }
  }

  focusTab(t.id);
}

/// Folds dock `d` or shows it, as the user asked. A right pane shown on a window too narrow for
/// it beside the conversation stays shown once the window is wide, as when an assistant chat's
/// list steps aside for it; one hidden there leaves the wide layout's choice as it was.
export function setFolded(d: Dock, folded: boolean) {
  setHidden(d, folded);
  if (d === "right" && folded) P.rightRequested = false;
  if (d === "right" && !folded && paneModel.getState().wideFolded !== null) paneModel.setState({ wideFolded: false });
}

/// Shows tab `id`, unfolding its dock, and gives the pane the keys.
export function focusTab(id: string) {
  const d = dockOf(id);
  leaveLog();
  setFolded(d, false);
  saveLayout();
  setOn(d, id);
  rememberPane();
  draw();
  paneOf(d)?.focus({ preventScroll: true });
}

/// On a phone, which shows one pane at a time, a tab being shown takes the screen from the log.
function leaveLog() {
  if (paneModel.getState().phone) paneModel.setState({ log: false });
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

/// Opens `rel`, a file in the session's folder, from the Files tab: a
/// picture, video, audio, 3D model or PDF in its viewer, anything else as text.
function openWorkFile(rel: string) {
  openIn(paneRoot(), rel);
}

/// Opens an image, HTML or Markdown file directly in its preview.
function previewWorkFile(rel: string) {
  const tab = openIn(paneRoot(), rel);
  if (tab) previewTab(tab);
}

export function previewTab(t: MediaTab) {
  if (t.kind !== "text" || t.preview?.active) return;
  if (!t.text) {
    t.previewQueued = true;
    rememberPane();
    return;
  }
  void togglePreview(t);
}

/// Opens `rel`, a file in the worktree at `root`, and gives its tab;
/// `elsewhere` is where else the file may be when it is not there.
function openIn(root: string, rel: string, elsewhere: WorkFile[] = []) {
  const full = rel.startsWith("/") ? rel : resolvePath(root, rel);
  const kind = mediaKind(full);
  // A file of the session's own folder keeps its tab through a switch.
  const id = kind ? full : root === P.session?.cwd.replace(/\/+$/, "") ? `text:${rel}` : `text:${root}\n${rel}`;

  if (kind) open({ id, kind, name: baseName(full), path: full, url: null, elsewhere });
  else open({ id, kind: "text", name: baseName(rel), path: null, url: null, rel, root, elsewhere });

  return openTabs().find(t => t.id === id) ?? null;
}

/// Opens the file a link in a reply points at, at its line. A relative
/// path is read from the worktree the pane shows, else from the session's
/// folder, else from the top of its checkout, where a session in a
/// subfolder finds the repository's worktrees. One outside the session's
/// folder opens as a tab saying the node would not read it.
export function openLinked(ref: FileRef) {
  const s = P.session;
  if (!s) return;

  if (ref.path.startsWith("~")) {
    openPath(ref.path);
    return;
  }

  const cwd = s.cwd.replace(/\/+$/, "");
  const shown = (paneRoot() || cwd).replace(/\/+$/, "");
  const full = resolvePath(shown, ref.path);
  const elsewhere = ref.path.startsWith("/") ? [] : [cwd, checkoutTop()]
    .filter((other): other is string => other !== null && other !== shown)
    .map(other => ({ root: other, full: resolvePath(other, ref.path) }))
    .filter(({ root, full }) => full.startsWith(`${root}/`))
    .map(({ root, full }) => ({ root, rel: full.slice(root.length + 1) }));
  const root = [shown, cwd].find(root => full.startsWith(`${root}/`)) ?? cwd;
  const rel = full.startsWith(`${root}/`) ? full.slice(root.length + 1) : full;
  // A tab that found the file at one of these places already, such as one
  // restored under the folder it was found in, is the one to show.
  const places = [{ root, rel }, ...elsewhere].map(place => resolvePath(place.root, place.rel));
  const held = openTabs().find(t => !t.closed && t.kind !== "term" && places.includes(t.rel ? resolvePath(t.root ?? cwd, t.rel) : t.path ?? ""));
  if (held) focusTab(held.id);
  const tab = held ?? openIn(root, rel, elsewhere);

  // An HTML page or Markdown file shows rendered; Source switches to its code.
  if (tab?.kind === "text" && tab.rel && hasPreview(tab.rel) && !tab.preview) previewTab(tab);

  if (tab && (tab.line !== ref.line || ref.line !== null)) {
    tab.line = ref.line;
    if (ref.line !== null) tab.lineJump = (tab.lineJump ?? 0) + 1;
    tab.rev++;
    draw();
  }
}

/// Opens a definition target returned by LSP at its 0-based line and column.
/// When `preferEdit` is set, opens or focuses the target in CodeMirror edit mode
/// so Cmd+click and Vim `gd` keep the editing flow intact.
function openLocation(root: string, location: LspLocation, preferEdit: boolean) {
  const s = P.session;
  if (!s) return;

  if (!location.inside_root) {
    openLinked({ path: location.path, line: location.line + 1 });
    return;
  }

  const cwd = s.cwd.replace(/\/+$/, "");
  const cleanRoot = root.replace(/\/+$/, "");
  const targetFull = resolvePath(cleanRoot, location.path);
  const held = openTabs().find(t => !t.closed && t.kind === "text" && t.rel && resolvePath(t.root ?? cwd, t.rel) === targetFull);
  const tab = held ?? openIn(cleanRoot, location.path);
  if (!tab || tab.kind !== "text") return;

  tab.findRequest = 0;
  if (tab.preview) tab.preview.active = false;
  tab.line = location.line + 1;
  tab.lineJump = (tab.lineJump ?? 0) + 1;

  const at = { line: location.line, column: location.column };
  if (tab.edit) {
    tab.edit.at = at;
  } else if (preferEdit && !onPhone()) {
    if (tab.text?.editable) tab.edit = newFileEdit(tab.text.text, at);
    else if (!tab.text) tab.editAt = at;
  }

  tab.rev++;
  if (held) focusTab(held.id); else draw();
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

/// Shows the Files tab, unfolding its dock.
function showFiles() {
  const d = dockOf(FILES);
  leaveLog();
  requestRightPane(d);
  setOn(d, FILES);
  setFolded(d, false);
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

/// Turns the Device tab's dock to it, unfolded, without taking the keys
/// from the conversation. A phone keeps its screen.
function showDevice() {
  syncCounts();
  if (!shell() || paneModel.getState().phone || !tabIds().includes(DEVICE)) return;
  const d = dockOf(DEVICE);
  requestRightPane(d);
  setFolded(d, false);
  saveLayout();
  setOn(d, DEVICE);
  rememberPane();
  if (!flipTab(d)) draw();
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

/// View diff: the Changes tab zoomed over the conversation, the keyboard in the diff.
/// Not `zoomed`, it shows beside the conversation. A folded dock opens; the
/// layout is otherwise as it was.
export function showChanges(zoomed = true) {
  if (!shell()) return;
  const d = dockOf(CHANGES);
  setOn(d, CHANGES);
  setFolded(d, false);
  paneModel.setState({ log: false });
  draw();

  if (zoomed) {
    paneModel.setState({ zoom: d });
    place();
    drawPaneNow();
    focusChanges();
  }

  rememberPane();
}

export function showTab(d: Dock, id: string) {
  requestRightPane(d);
  setOn(d, id);
  rememberPane();
  if (!flipTab(d)) draw();
  drawPaneNow();
  paneOf(d)?.focus({ preventScroll: true });
}

function requestRightPane(d: Dock) {
  const { split, places } = paneModel.getState();
  if (d === "right" && split && !places.right.shown && !places.right.strip && desktopChatPane()) P.rightRequested = true;
}

/// Files and Changes read the folder of session `s`.
function readWorktrees(s: PaneSession) {
  startWorktrees(s, root => { if (root) { pointFilesAt(); revalidateChanges(); } draw(); }, writingComment);
}

// Leaving the session view lets go of what its tabs hold. Files and Changes
// keep their views of the last few folders, to show again as they were.
changeListeners.push(() => { if (S.view !== "agent") { closePaneTabs(); unfollowFiles(); P.watch?.disconnect(); P.watch = null; watchShell(null); } });

// The tab that should show, and its file: the pane makes itself match it.
let shownTarget: PaneWant | null = null;
paneTarget.subscribe(({ target }) => {
  if (target === shownTarget) return;
  shownTarget = target;
  if (target) showTarget(target);
});

function showTarget(wanted: PaneWant) {
  if (wanted.kind === "show") {
    if (wanted.tab === "files") showFiles();
    else if (wanted.tab === "device") showDevice();
    else showChanges();
  } else if (wanted.kind === "open") {
    if (wanted.preview) previewWorkFile(wanted.rel); else openWorkFile(wanted.rel);
  } else if (wanted.kind === "openPath") {
    openPath(wanted.path);
  } else {
    openLocation(wanted.root, wanted.location, wanted.preferEdit);
  }
}
