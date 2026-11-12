// agentpane/: a coding agent session's side pane and its tabs (media,
// files, changes, artifacts, terminal). Entry: this file.
// The media pane of a coding agent session: pictures, videos, 3D models and
// PDFs the agent made or looked at, its files, artifacts, terminals and
// subagents, each a tab. Drawing the tabs is in media-draw.ts, the docks'
// layout and dividers in media-layout.ts.
import { shownMessage } from "../agents/session-frame.tsx";
import { type SubagentRow } from "./subagents-view.tsx";
import { readView, writeView, viewKey } from "./view-memory.ts";
import { queuePreview } from "./html-preview-state.ts";
import { baseName, type Dock, type FileRef, DOCKS, type ImageView, type MediaKind, mediaKind, moveTab, type PaneLayout, readLayout, resolvePath, SIDE_BY_SIDE_MIN, tabDock } from "./media-state.ts";
import { terminalName } from "./files-state.ts";
import type { ModelView } from "./model3d.ts";
import { artifactCount, pickArtifact } from "./artifacts.ts";
import { changeListeners, PANE, S } from "../bar/core.ts";
import { focusChanges, revalidateChanges } from "./changes.ts";
import { pointFilesAt } from "./files.ts";
import { paneRoot, startWorktrees } from "./worktree.ts";
import { dropTerm, killTerm, type PaneTerm, type TermPlace } from "./terminal.ts";
import type { TerminalRow } from "../terminals/terminals.ts";
import { newTermLike } from "../terminals/newkey-state.ts";
import { readError, loadText, startTerminal, load } from "./read.ts";
import { togglePreview } from "./text-view.tsx";
import { frameResize } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";
import { draw, flipTab, noteTab, pickPhoneTab } from "./media-draw.ts";
import { dockHolding, dockOf, docked, paneModel, paneOf, place, setPaneModel, shell, shellResized, shellSize, shownDocks, watchShell, tabIds } from "./media-layout.ts";
import { savesName } from "../ui/platform.ts";

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
export type TabKind = MediaKind | "text" | "term";

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
  /// Counts changes to what its view shows, so the view draws again.
  rev: number;
  /// Its view was last drawn hidden behind another tab.
  viewHidden: boolean;
  /// What a save or copy said, shown in its footer.
  note: string | null;
  /// A video tab's element, and where it stood when its view went.
  video: HTMLVideoElement | null;
  videoAt: number;
  image?: { view: ImageView; w: number; h: number };
  model?: ModelView | null;
  wire?: boolean;
  bytes?: ArrayBuffer;
  /// A text tab's path in the worktree it was opened from, that worktree's
  /// folder, and what was read of it.
  rel?: string;
  root?: string;
  text?: TreeText;
  /// The line a link pointed at, marked and scrolled to in the text view.
  line?: number | null;
  /// The same file in the session's folder, read when it is not in the
  /// worktree it was looked for in.
  elsewhere?: string | null;
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

export const SUBS = "subagents";
export const FILES = "files";
export const CHANGES = "changes";
export const ARTS = "artifacts";
export const RUNNING = "running";
const KEY = `beep.mediaPane.${PANE ? "window" : "bar"}`;
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
  /// The session drew on a phone, which shows the conversation first.
  phoneShown: false,
};

/// Starts the pane for session `s`, closing what the last one had open.
export function startPane(s: PaneSession) {
  closePaneTabs();
  P.session = s;
  paneReleased = true;
  P.on = { right: FILES, bottom: FILES };
  const saved = readView(viewKey(s.machine_id, s.id));
  P.layout = loadLayout();
  if (saved && typeof saved === "object" && "layout" in saved) P.layout = readLayout(JSON.stringify(saved.layout));
  foldInBar();
  P.subs = [];
  P.subCount = 0;
  P.moved.clear();
  P.zoom = null;
  P.phoneShown = false;
  const el = shell();
  if (!el) return;
  P.watch?.disconnect();
  // Too narrow for a right dock, its tabs join the bottom one, and back.
  P.watch = frameResize(() => {
    shellResized();
    if (paneModel.getState().narrow !== (shellSize().width < SIDE_BY_SIDE_MIN)) draw(); else place();
  });
  P.watch.observe(el);
  watchShell(el);
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
    foldInBar();
    if ("zoom" in saved && (saved.zoom === "right" || saved.zoom === "bottom")) P.zoom = saved.zoom;
  }
  draw();
  paneReleased = false;
  rememberPane();
}

/// The bar opens a session with its docks folded, the conversation first.
function foldInBar() {
  if (!PANE) P.layout.hidden = { right: true, bottom: true };
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
  revalidateChanges();
  draw();
}

/// Reusing a conversation frame also restores views released when leaving it.
export function restorePane() {
  if (paneReleased && P.session) startPane(P.session);
}

export function closePaneTabs() {
  rememberPane();
  paneReleased = true;
  for (const t of P.tabs) {
    dropTab(t);
    t.term = undefined;
  }

  P.tabs = [];
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
    const tab: MediaTab = { size: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, video: null, videoAt: 0, ...t };
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
export function focusTab(id: string) {
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

export function previewTab(t: MediaTab) {
  if (t.preview?.active) return;
  if (!t.text) { queuePreview(t); return; }
  void togglePreview(t);
}

/// Opens `rel`, a file in the worktree at `root`, and gives its tab;
/// `elsewhere` is where else the file may be when it is not there.
function openIn(root: string, rel: string, elsewhere: string | null = null) {
  const full = rel.startsWith("/") ? rel : resolvePath(root, rel);
  const kind = mediaKind(full);
  // A file of the session's own folder keeps its tab through a switch.
  const id = kind ? full : root === P.session?.cwd.replace(/\/+$/, "") ? `text:${rel}` : `text:${root}\n${rel}`;

  if (kind) open({ id, kind, name: baseName(full), path: full, url: null, elsewhere });
  else open({ id, kind: "text", name: baseName(rel), path: null, url: null, rel, root, elsewhere });

  return P.tabs.find(t => t.id === id) ?? null;
}

/// Opens the file a link in a reply points at, at its line. A relative
/// path is read from the worktree the pane shows, else from the session's
/// folder. One outside the session's folder opens as a tab saying the node
/// would not read it.
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
  const elsewhere = !ref.path.startsWith("/") && shown !== cwd ? resolvePath(cwd, ref.path) : null;
  const root = [shown, cwd].find(root => full.startsWith(`${root}/`)) ?? cwd;
  const tab = openIn(root, full.startsWith(`${root}/`) ? full.slice(root.length + 1) : full, elsewhere);

  if (tab && tab.line !== ref.line) {
    tab.line = ref.line;
    tab.rev++;
    draw();
  }
}

/// Saves `rel`, a file in the worktree at `root` (the one shown when
/// none is given), to Downloads; `note` hears how it went.
export function saveWorkFile(rel: string, note: (text: string) => void, root = paneRoot()) {
  const s = P.session;
  if (!s) return;
  note(`Saving ${baseName(rel)}…`);
  invoke<string>("agent_tree_save", { machineId: s.machine_id, root, path: rel })
    .then(to => { note(`Saved to ${savesName()} as ${baseName(to)}`); invoke("reveal_file", { path: to }).catch(() => {}); })
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
  const narrow = !!shell() && shellSize().width < SIDE_BY_SIDE_MIN;
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
  const shown = shownDocks().flatMap(d => {
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

export function closeTab(id: string) {
  const i = P.tabs.findIndex(t => t.id === id);
  if (i < 0) return;
  // The tab goes once its terminal stopped, and says why when it could not.
  const tab = P.tabs[i];

  if (tab.term) {
    if (tab.closed) return;
    tab.closed = true;
    const term = tab.term;
    noteTab(tab, "Closing…");
    killTerm(term).then(() => {
      if (tab.term !== term) return;
      tab.term = undefined;
      closeTab(id);
    }, e => {
      tab.closed = false;
      noteTab(tab, `Could not stop it: ${e instanceof Error ? e.message : e}`);
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

/// Closes the tab on screen in the focused pane (⌘W); false when no pane is
/// focused or that tab stays, as Files does. A terminal's tab ends the terminal.
export function closeFocusedTab() {
  const d = dockHolding(document.activeElement);
  const tab = d && P.tabs.find(t => t.id === P.on[d]);
  if (!d || !tab) return false;
  closeTab(tab.id);
  paneOf(d)?.focus({ preventScroll: true });
  return true;
}

/// Shows the Files tab, unfolding its dock.
export function showFiles() {
  const d = dockOf(FILES);
  P.on[d] = FILES;
  P.layout.hidden[d] = false;
  draw();
}

/// Shows the Running tab, unfolding its dock; on a phone it fills the screen.
export const showRunning = () => showList(RUNNING);
/// Shows the Subagents tab the same way.
export const showSubagents = () => showList(SUBS);

function showList(id: typeof RUNNING | typeof SUBS) {
  if (!shell() || !tabIds().includes(id)) return;
  if (paneModel.getState().phone) return pickPhoneTab(id);

  const d = dockOf(id);
  P.layout.hidden[d] = false;
  saveLayout();
  showTab(d, id);
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

/// View diff: the Changes tab zoomed over the conversation, the keyboard in the diff.
/// A folded dock opens; the layout is otherwise as it was.
export function showChanges() {
  if (!shell()) return;
  const d = dockOf(CHANGES);
  P.on[d] = CHANGES;
  P.layout.hidden[d] = false;
  setPaneModel({ log: false });
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
  if (!P.zoom && !dockHolding(focus)) return false;
  P.zoom = null;
  place();
  rememberPane();
  const input = shownMessage();
  if (input) input.focus(); else if (focus instanceof HTMLElement) focus.blur();
  return true;
}

export function showTab(d: Dock, id: string) {
  P.on[d] = id;
  rememberPane();
  if (!flipTab(d)) draw();
  paneOf(d)?.focus({ preventScroll: true });
}

/// Files and Changes read the folder of session `s`.
function readWorktrees(s: PaneSession) {
  startWorktrees(s, root => { if (root) { pointFilesAt(); revalidateChanges(); } draw(); });
}

/// Remembers the pane before releasing its views. Terminals keep running.
let paneReleased = false;
const startingTerminals = new Map<string, MediaTab[]>();
export function rememberPane() {
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

function dropTab(t: MediaTab) {
  t.video?.pause();
  if (t.term) dropTerm(t.term);
  t.model?.dispose();
  if (t.url?.startsWith("blob:")) URL.revokeObjectURL(t.url);
}

// Leaving the session view lets go of what its tabs hold. Files and Changes
// keep their views of the last few folders, to show again as they were.
changeListeners.push(() => { if (S.view !== "agent") { closePaneTabs(); P.watch?.disconnect(); P.watch = null; watchShell(null); } });

function loadLayout(): PaneLayout {
  try { return readLayout(localStorage.getItem(KEY)); } catch { return readLayout(null); }
}

export function saveLayout() {
  try { localStorage.setItem(KEY, JSON.stringify(P.layout)); } catch { /* kept while the window is open */ }
  rememberPane();
}
