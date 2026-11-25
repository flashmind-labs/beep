// The Files tab of a coding agent session's pane: the session's working folder
// on its machine as a tree, read three levels at a time, what git ignores left
// out unless asked, git's marks beside what changed. The trees of the last few
// folders are kept: showing one again draws it as it was left and reads its
// folders again only when the folder's `files` topic said something changed.
// The view is `FilesView` in files-tree.tsx, drawn from `filesModel`.
import { createRef, type DragEvent, type KeyboardEvent, type MouseEvent } from "react";
import { createStore } from "zustand";
import type { FilesMenuProps } from "./files-menu.tsx";
import { writeClipboard } from "../ui/clipboard.ts";
import { hasPreview } from "./html-preview-state.ts";
import { reloadChatAgents } from "./chat-agents.tsx";
import { type Listing, type TreeRow, isTree, atEdge, unreadFolders, failedListing, mergeTree, parentPath, joinPath, treeRows } from "./files-state.ts";
import { pushedEach } from "../ui/snapshot-state.ts";
import { relativeTo } from "./artifacts-state.ts";
import { baseName, mediaKind, resolvePath } from "./media-state.ts";
import { paneRoot, watchRoot, worktreesReady } from "./worktree.ts";
import { mentioned } from "./worktree-state.ts";
import { sessions } from "../agents/sessions.ts";
import { threadFolders } from "../agents/threads-state.ts";
import { openWorkFile, previewWorkFile, paneSession, saveWorkFile, showFiles } from "./media.ts";
import { desktopChatPane, place } from "./media-layout.ts";
import { localFileItems } from "./localfile.ts";
import { addMention, askUpload } from "../agents/workdrop.ts";
import { hideDropOverlay } from "../chat/attach.tsx";
import { selectFile, selectionRoots, fileNameError, canPasteFiles, type FileClipboard } from "./file-actions-state.ts";
import { confirmSheet, renameSheet } from "../ui/menu.tsx";
import { invoke } from "../ui/tauri.ts";
import { AMBIENT_MS, ambient, paneShown } from "../ui/active.ts";
import { savedIn } from "../ui/platform.ts";

/// The type a row of the tree drags as, its path relative to the session's folder.
export const PATH_TYPE = "application/x-beep-path";

/// How many folders keep their tree.
const KEPT = 4;

/// The tree of one folder, owned directly while shown and kept while another shows.
interface FolderTree {
  key: string;
  listings: Map<string, Listing>;
  roots: Set<string>;
  flat: boolean;
  open: Set<string>;
  filter: string;
  /// Whether its listings include what git ignores.
  ignored: boolean;
  picked: string | null;
  anchor: string | null;
  selected: Set<string>;
  scroll: number;
  /// Folders whose reads stopped as another folder showed.
  unread: Set<string>;
  /// The node said something in the folder changed since the last refresh.
  dirty: boolean;
  unwatch: () => void;
}

const emptyTree = (key = "", ignored = false): FolderTree => ({
  key, listings: new Map(), roots: new Set(), flat: false, open: new Set(), filter: "", ignored,
  picked: null, anchor: null, selected: new Set(), scroll: 0, unread: new Set(), dirty: false, unwatch: () => {},
});

/// Most recently shown last, plus the currently active tree.
const kept = new Map<string, FolderTree>();
let current: FolderTree = emptyTree();

const view = {
  reading: new Set<string>(),
  again: new Set<string>(),
  walking: false,
  capped: false,
  clipboard: null as FileClipboard | null,
  fresh: false,
  generation: 0,
  made: false,
  reveal: null as string | null,
};

const isCurrent = (tree: FolderTree, generation: number) => current === tree && view.generation === generation;

/// What the tree shows: rows, or a message in their place.
export type TreeView =
  | { kind: "rows"; rows: TreeRow[]; more: number; walking: boolean; capped: boolean }
  | { kind: "message"; text: string; error: boolean; loading: boolean };

/// What `FilesView` draws.
interface FilesModel {
  root: string;
  filter: string;
  ignored: boolean;
  label: string;
  busy: boolean;
  /// The id of the picked row, and the paths selected.
  active: string | null;
  selected: ReadonlySet<string>;
  status: string;
  /// Null for a session with no folder yet.
  tree: TreeView | null;
  menu: FilesMenuProps | null;
  /// The folder a drag of files from this computer would drop into; "" for
  /// the top, over the tree's empty space.
  drop: string | null;
  reveal: string | null;
}

export const filesModel = createStore<FilesModel>(() => ({
  root: "", filter: "", ignored: false, label: "", busy: false, active: null, selected: new Set(), status: "", tree: null, menu: null, drop: null, reveal: null,
}));

/// The Files view and its tree, while they are drawn.
export const filesRef = createRef<HTMLDivElement>();
export const treeRef = createRef<HTMLDivElement>();

/// The rows the tree last drew: a refresh that has not read the top
/// folder yet leaves them on screen.
let showingRows = false;

/// The tree shown is not yet back where it was left: one drawn while its
/// page is hidden, as a session comes back, can scroll only once it has a
/// size, and one whose rows are not drawn yet only once they are. Its
/// scroll is not kept meanwhile.
let restoring = true;

const setModel = (change: Partial<FilesModel>) => filesModel.setState(change);
const ignoredNow = () => filesModel.getState().ignored;
const busyNow = () => filesModel.getState().busy;

/// Folder reads run at once, and the entries a filter stops at.
const WALK_AT_ONCE = 8;
export const WALK_ENTRIES = 50_000;

/// The pane drew the Files tab: `on` when it is the dock's tab on screen,
/// `visible` when it can be seen. It shows the tree of the folder the pane reads.
export function filesTab(on: boolean, visible: boolean) {
  view.made = true;
  const key = treeKey();
  if (key !== current.key) showTree(key);
  if (!on) closeFilesMenu();
  revalidateFiles(visible);
}

/// The menu a right-click opened, while it shows.
export const filesMenuRef = createRef<HTMLDivElement>();

/// Closes the menu a right-click opened; true when one was open.
export function closeFilesMenu() {
  const open = !!filesModel.getState().menu;
  const focused = !!filesMenuRef.current?.contains(document.activeElement);
  if (open) setModel({ menu: null });
  if (focused) treeRef.current?.focus({ preventScroll: true });
  return open;
}

/// Shows the tree of the worktree the pane shows now: as it was left when it
/// is kept, read otherwise. What git ignores stays as it is, the open file
/// tabs are not touched.
export function pointFilesAt() {
  const key = treeKey();
  if (view.made && key !== current.key) showTree(key);
}

const treeKey = () => {
  const s = paneSession();
  return s?.cwd && paneRoot() ? `${s.machine_id}\n${paneRoot()}` : "";
};

/// Whether the working folder has anything to show before applying the file filter.
export function hasFolderEntries() {
  const top = current.listings.get("");
  if (!top?.entries || current.key !== treeKey()) return false;
  const threads = threadsHere();
  return top.more > 0 || top.entries.some(entry => !entry.folder || !threads.has(entry.name));
}

/// Saves scroll and unfinished reads on the outgoing tree and switches to `key`.
function showTree(key: string) {
  closeFilesMenu();
  if (current.key) {
    for (const dir of view.reading) current.unread.add(dir);
    keepScroll();
  }

  const s = paneSession();
  let entry = kept.get(key) ?? null;

  if (!entry && key && s) {
    const made = emptyTree(key, ignoredNow());
    const machine = s.machine_id, root = paneRoot();
    made.unwatch = watchRoot(machine, root, data => {
      // A node that pushes the folders it listed for this tree has nothing to be read again.
      if (takeListings(made, machine, root, data)) return;
      made.dirty = true;
      if (current === made) revalidate();
    });
    entry = made;
  }

  if (entry) { kept.delete(key); kept.set(key, entry); }
  current = entry ?? emptyTree("", ignoredNow());
  Object.assign(view, { reading: new Set<string>(), again: new Set<string>(), walking: false, capped: false, fresh: false, generation: view.generation + 1, reveal: null });
  restoring = true;
  setModel({ label: "", active: null, status: "", busy: false });
  watchingFiles = false;
  trim();
  draw();
  if (!entry) return;

  // Read what was never read or stopped, and all of it when it was read with ignored files shown otherwise.
  if (entry.ignored !== ignoredNow()) { entry.ignored = ignoredNow(); entry.dirty = true; }
  if (!entry.listings.get("")?.entries) load("");
  for (const dir of entry.unread) load(dir);
  entry.unread.clear();
  if (entry.filter.trim()) readAll();
}

/// What the tree scrolled to is kept, once it is back where it was left.
export function keepScroll() {
  const tree = treeRef.current;
  if (current.key && !restoring && tree?.isConnected && tree.offsetParent) current.scroll = tree.scrollTop;
}

/// The reader scrolls, or a row asked for comes into view: that is where the tree is now.
export function scrolledHere() {
  restoring = false;
}

/// The tree may be drawn again from nothing, as its view goes and comes back.
export function scrollAgain() {
  restoring = true;
}

/// Where the tree shown was left, for its first draw.
export const keptScroll = () => (current.key ? current.scroll : 0);

/// Puts the tree back where it was left, once it can scroll there. True
/// when this moved it.
export function restoreScroll(tree: HTMLElement) {
  // Hidden, it has no scroll: it goes back once it shows.
  if (!tree.clientHeight) { restoring = true; return false; }
  if (!restoring) return false;
  const scroll = keptScroll(), was = tree.scrollTop;
  if (was !== scroll) tree.scrollTop = scroll;
  restoring = Math.abs(tree.scrollTop - scroll) >= 1;
  return tree.scrollTop !== was;
}

/// Lets go of the least recent trees past `KEPT`.
function trim() {
  for (const entry of kept.values()) {
    if (kept.size <= KEPT) return;
    if (entry === current) continue;
    entry.unwatch();
    kept.delete(entry.key);
  }
}

/// Takes the folder listings an update of `root` pushes into `tree`, as the tree read them: each
/// view carries the folder as `agent_tree` answers it. True when every view the update carries for
/// the tree was one it took; false when it carries none (a node that does not push them), or one
/// that failed, so the tree reads again.
function takeListings(tree: FolderTree, machine: string, root: string, data: unknown) {
  let taken = false, redraw = false;

  for (const { args, pushed } of pushedEach<unknown>(data, "agent_tree")) {
    if (args.machineId !== machine || args.root !== root || args.deep === true || args.ignored !== tree.ignored || typeof args.path !== "string") continue;
    if ("error" in pushed || !isTree(pushed.value)) return false;
    taken = true;
    redraw = mergeTree(tree.listings, pushed.value) || redraw;
    tree.roots.add(args.path);
  }

  if (redraw && current === tree) draw();
  return taken;
}

/// Whether the tree was being watched when last asked.
let watchingFiles = false;
/// Recheck on changes while the tree shows or Assistant needs to decide whether to show it.
const revalidate = ambient(() => (current.key && current.dirty ? refresh() : undefined), AMBIENT_MS, () =>
  filesShown() || (paneShown() && !!desktopChatPane()));

/// Pick up pending changes immediately when this tree starts being watched again.
function revalidateFiles(visible: boolean) {
  const now = paneShown() && (visible || !!desktopChatPane()), appeared = now && !watchingFiles;
  watchingFiles = now;
  if (now && current.key && current.dirty) revalidate(appeared);
}

/// Shows `rel`, a file in the session's folder, in the tree: its folders
/// open, the file picked and in view, the Files tab on screen.
export function revealPath(rel: string) {
  const s = paneSession();
  // `rel` is in the session's folder; the tree may be of another worktree.
  const inRoot = s ? relativeTo(paneRoot(), resolvePath(s.cwd, rel)) : null;
  if (inRoot == null) return;
  const parts = inRoot.split("/").filter(Boolean);
  if (!parts.length) return;
  showFiles();
  current.filter = "";

  for (let i = 1; i < parts.length; i++) {
    const dir = parts.slice(0, i).join("/");
    current.open.add(dir);
    if (!current.listings.get(dir)?.entries) load(dir);
  }

  const picked = parts.join("/");
  current.picked = current.anchor = view.reveal = picked;
  current.selected = new Set([picked]);
  draw();
}

/// Reads `dir` again from the node when the tree has it, as after an upload into it.
export function reloadFolder(dir: string) {
  for (let d = dir.replace(/\/+$/, ""); ; d = parentPath(d)) {
    if (current.listings.has(d)) { load(d, false, true); return; }
    if (!d) return;
  }
}

/// Reads every folder the tree was read from again, in place, past the
/// native cache: only for a change, a refresh or an edit.
async function refresh() {
  const tree = current, generation = view.generation;
  if (tree.key) tree.dirty = false;
  view.fresh = true;
  const roots = [...new Set(["", ...tree.roots, ...view.reading])];
  let read = false;

  try {
    for (let offset = 0; offset < roots.length; offset += WALK_AT_ONCE) {
      if (!isCurrent(tree, generation)) return;
      await Promise.all(roots.slice(offset, offset + WALK_AT_ONCE).map(dir => load(dir)));
    }
    read = isCurrent(tree, generation);
  } finally {
    if (isCurrent(tree, generation)) view.fresh = false;
    // A read that stopped short leaves the tree as stale as before, so the next show reads again.
    if (!read && tree.key) tree.dirty = true;
  }

  if (!isCurrent(tree, generation)) return;
  const error = [...tree.listings.values()].find(listing => listing.error)?.error;
  if (error) {
    if (tree.key) tree.dirty = true;
    throw new Error(error);
  }

  // The chat's agent may have written agents of its own since.
  reloadChatAgents();
}

/// Whether the tree can be seen: its tab is picked (a view behind another
/// is `hidden` but keeps its layout) in a dock not folded (`offsetParent` is
/// null under one), and the pane shows.
const filesShown = () => !!filesRef.current?.offsetParent && !filesRef.current.hidden && paneShown();

/// Reads the rest of the tree while a filter is typed, so it finds what no
/// folder opened yet shows.
async function readAll() {
  if (view.walking) return;
  const tree = current, generation = view.generation;
  view.walking = true;
  draw();

  while (isCurrent(tree, generation) && tree.filter.trim()) {
    let entries = 0;
    for (const listing of tree.listings.values()) entries += listing.entries?.length ?? 0;
    view.capped = entries >= WALK_ENTRIES;
    const next = unreadFolders(tree.listings).slice(0, WALK_AT_ONCE);
    if (view.capped || !next.length) break;
    await Promise.all(next.map(dir => load(dir, true)));
  }

  if (!isCurrent(tree, generation)) return;
  view.walking = false;
  draw();
}

/// Reads the tree from `dir` down and merges it in. `deep` is a filter's
/// read: as far as one read goes, and not read again on refresh. `fresh`
/// asks the node past the copy of the folder this computer may have.
const reads = new Map<string, Promise<void>>();
function load(dir: string, deep = false, fresh = false): Promise<void> {
  const tree = current, generation = view.generation;
  const token = JSON.stringify([tree.key, generation, dir]);
  const pending = reads.get(token);
  if (pending) { if (!deep) view.again.add(dir); return pending; }
  const read = (async () => {
    do {
      view.again.delete(dir);
      await readFolder(tree, generation, dir, deep, fresh);
      deep = false;
    } while (isCurrent(tree, generation) && view.again.has(dir));
  })().finally(() => reads.delete(token));
  reads.set(token, read);
  return read;
}

async function readFolder(tree: FolderTree, generation: number, dir: string, deep: boolean, fresh: boolean) {
  await worktreesReady();
  if (!isCurrent(tree, generation)) return;
  const s = paneSession();
  if (!s?.cwd) return;
  const had = tree.listings.get(dir);
  const layoutDependsOnFiles = dir === "" && !!desktopChatPane();
  const hadEntries = layoutDependsOnFiles && hasFolderEntries();
  tree.listings.set(dir, { entries: had?.entries ?? null, more: had?.more ?? 0, error: "" });
  view.reading.add(dir);
  // A folder shown stays as it is while it is read again.
  if (!had?.entries) draw();
  let changed = true;

  try {
    const got = await invoke<unknown>("agent_tree", { machineId: s.machine_id, root: paneRoot(), path: dir, ignored: ignoredNow(), deep, fresh: fresh || view.fresh });
    if (!isCurrent(tree, generation)) return;
    if (!isTree(got)) throw new Error("The machine sent no file listing.");
    changed = mergeTree(tree.listings, got);
    if (!deep) tree.roots.add(dir);
    const top = got.folders[0];
    tree.flat ||= got.folders.length === 1 && !got.truncated && !!top?.entries.some(e => e.folder && !e.ignored);
  } catch (e) {
    if (!isCurrent(tree, generation)) return;
    tree.listings.set(dir, failedListing(String(e instanceof Error ? e.message : e), had));
  } finally {
    if (isCurrent(tree, generation)) view.reading.delete(dir);
  }

  if (changed || !had?.entries) draw();
  if (layoutDependsOnFiles && hadEntries !== hasFolderEntries()) place();
  if (isCurrent(tree, generation) && tree.filter.trim()) readAll();
}

function draw() {
  if (!paneSession()?.cwd) { showingRows = false; setModel({ tree: null }); return; }
  const top = current.listings.get("");
  const rows = rowsNow();
  const visible = new Set(rows.map(row => row.path));
  current.selected = new Set([...current.selected].filter(path => visible.has(path)));
  if (current.picked && !visible.has(current.picked)) current.picked = null;
  const label = busyNow() ? filesModel.getState().label : current.selected.size ? `${current.selected.size} selected` : "";
  const shell = { root: paneRoot(), filter: current.filter, label, selected: current.selected, active: current.picked ? rowId(current.picked) : null };

  let message: string | null = null, error = false, loading = false;
  if (top?.error && !top.entries) { message = top.error; error = true; }
  else if (!top?.entries && view.reading.has("") && showingRows) { setModel(shell); return; }
  else if (!top?.entries) { message = "Reading the folder…"; loading = true; }
  else if (!rows.length && view.walking && current.filter.trim()) { message = "Reading folders…"; loading = true; }
  else if (!rows.length && !view.capped) message = current.filter ? "No files match." : "This folder is empty.";
  const searching = !!current.filter.trim() && !!top?.entries && !top.error;
  showingRows = message === null;
  const tree: TreeView = message !== null ? { kind: "message", text: message, error, loading }
    : { kind: "rows", rows, more: top?.more ?? 0, walking: searching && view.walking && !!rows.length, capped: searching && !view.walking && view.capped };
  setModel({ ...shell, tree, reveal: view.reveal });
}

/// The element id of the row of `path`.
export const rowId = (path: string) => `agfile-${encodeURIComponent(path)}`;

/// The tree drew the row `view.reveal` asked for.
export function revealed() {
  restoring = false;
  view.reveal = null;
  setModel({ reveal: null });
}

function toggle(path: string) {
  if (current.open.has(path)) current.open.delete(path);
  else {
    current.open.add(path);
    // Read on from a folder at the edge: shown at once when it was read.
    if (!current.listings.get(path)?.entries || (!current.flat && atEdge(current.listings, path))) load(path);
  }

  draw();
}

/// The row under an event in the tree, if any.
const rowAt = (target: EventTarget) => target instanceof Element ? target.closest<HTMLElement>("[data-fpath]") : null;

/// The folder a drop on `row` goes into: the folder itself, or a file's.
function dropFolder(row: HTMLElement | null) {
  if (!row?.dataset.fpath) return "";
  return row.dataset.ffolder ? row.dataset.fpath : parentPath(row.dataset.fpath);
}

/// The filter's field changed.
export function setFilter(value: string) {
  current.filter = value;
  draw();
  if (current.filter.trim()) readAll();
}

/// Show ignored was ticked or not.
export function setIgnored(on: boolean) {
  current.ignored = on;
  setModel({ ignored: on });
  void refresh().catch(e => console.warn("files:", e));
}

export function refreshFiles() {
  void refresh().catch(e => console.warn("files:", e));
}

function clearSelection() {
  current.selected.clear();
  current.picked = current.anchor = null;
}

/// What the tree does with the pointer, the keys and drags; a row's path is
/// on its `data-fpath`.
export const treeActions = {
  click(e: MouseEvent<HTMLElement>) {
    closeFilesMenu();
    e.currentTarget.focus();
    const row = rowAt(e.target);
    const path = row?.dataset.fpath;
    if (!row || !path) { clearSelection(); draw(); return; }
    Object.assign(current, selectFile(current, visiblePaths(), path, e.metaKey || e.ctrlKey, e.shiftKey));
    if (e.metaKey || e.ctrlKey || e.shiftKey) { draw(); return; }

    if (row.dataset.ffolder) toggle(path);
    else { draw(); openFile(path); }
  },
  contextMenu(e: MouseEvent<HTMLElement>) {
    e.preventDefault();
    e.currentTarget.focus();
    const row = rowAt(e.target);
    const path = row?.dataset.fpath ?? "";

    if (path && !current.selected.has(path)) Object.assign(current, selectFile(current, visiblePaths(), path));
    else if (!path) clearSelection();

    draw();
    menu(path, !!row?.dataset.ffolder, e.clientX, e.clientY);
  },
  keyDown(e: KeyboardEvent<HTMLElement>) {
    if (e.nativeEvent.isComposing || (e.target instanceof Element && e.target.closest("input,textarea,[contenteditable=true]"))) return;
    const cmd = e.metaKey || e.ctrlKey, key = e.key.toLowerCase();
    let handled = true;

    if (cmd && key === "a") { current.selected = new Set(visiblePaths()); draw(); }
    else if (cmd && (key === "c" || key === "x")) clipboard(key === "x");
    else if (cmd && key === "v") void paste(destination());
    else if ((cmd && (key === "backspace" || key === "delete")) || (!cmd && key === "delete")) deleteItems();
    else if (key === "escape") { closeFilesMenu(); clearSelection(); draw(); }
    else if (key === "enter" || key === "f2") renameItem();
    else if (key === " " && current.picked) openPicked();
    else if (key === "arrowup" || key === "arrowdown" || key === "home" || key === "end") {
      const paths = visiblePaths(), at = current.picked ? paths.indexOf(current.picked) : -1;
      const next = key === "home" ? 0 : key === "end" ? paths.length - 1 : Math.max(0, Math.min(paths.length - 1, at + (key === "arrowup" ? -1 : 1)));
      if (paths[next]) { Object.assign(current, selectFile(current, paths, paths[next], false, e.shiftKey)); view.reveal = paths[next]; draw(); }
    } else if ((key === "arrowright" || key === "arrowleft") && current.picked) {
      const row = rowsNow().find(r => r.path === current.picked);
      if (row?.folder && row.open === (key === "arrowleft")) toggle(row.path);
    } else if (key === "contextmenu" || (e.shiftKey && key === "f10")) {
      // At the first selected row, or the tree's corner while that row is scrolled away.
      const first = rowsNow().find(row => current.selected.has(row.path)) ?? null;
      const box = ((first && document.getElementById(rowId(first.path))) ?? e.currentTarget).getBoundingClientRect();
      menu(current.picked ?? "", !!first?.folder, box.left, box.bottom);
    } else handled = false;

    if (handled) { e.preventDefault(); e.stopPropagation(); }
  },
  dragStart(e: DragEvent<HTMLElement>) {
    const path = rowAt(e.target)?.dataset.fpath;
    if (!path) return;
    const named = mentioned(paneRoot(), paneSession()?.cwd ?? "", path);
    e.dataTransfer.setData(PATH_TYPE, named);
    e.dataTransfer.setData("text/plain", named);
    e.dataTransfer.effectAllowed = "copy";
  },
  // Files from Finder dropped on a folder (or a file's folder, or the
  // tree's empty space for the top) upload there, once the user says so.
  dragOver(e: DragEvent<HTMLElement>) {
    if (!e.dataTransfer.types.includes("Files")) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = "copy";
    const dir = dropFolder(rowAt(e.target));
    if (filesModel.getState().drop !== dir) setModel({ drop: dir });
  },
  dragLeave(e: DragEvent<HTMLElement>) {
    if (!(e.relatedTarget instanceof Node && e.currentTarget.contains(e.relatedTarget))) setModel({ drop: null });
  },
  drop(e: DragEvent<HTMLElement>) {
    if (!e.dataTransfer.files.length) return;
    e.preventDefault();
    e.stopPropagation();
    const dir = dropFolder(rowAt(e.target));
    setModel({ drop: null });
    hideDropOverlay();
    askUpload([...e.dataTransfer.files], dir, paneRoot());
  },
};

/// The right-click menu of `path`.
function menu(path: string, folder: boolean, x: number, y: number) {
  closeFilesMenu();
  const s = paneSession();
  if (!s) return;
  const paths = selectedPaths(), n = paths.length, single = n === 1 && current.selected.size === 1;
  const dir = path && folder ? path : parentPath(path);
  const clip = view.clipboard;
  const items: [string, string][] = [
    ...(single ? [["open", folder ? "Open folder" : "Open in viewer"]] as [string, string][] : []),
    ...(single && !folder && (hasPreview(path) || mediaKind(path) === "image") ? [["preview", "Preview"]] as [string, string][] : []),
    ...(n ? [["mention", `Mention ${n > 1 ? `${n} items` : "in message"}`], ["-", ""], ["abs", "Copy path"], ["rel", "Copy relative path"], ["copy", `Copy${n > 1 ? ` ${n} items` : ""}`], ["cut", `Cut${n > 1 ? ` ${n} items` : ""}`]] as [string, string][] : []),
    ...(clip && canPasteFiles(clip, current.key, dir) ? [["paste", `Paste ${clip.paths.length} items into ${dir || "working folder"}`]] as [string, string][] : []),
    ...(single ? [["rename", "Rename..."]] as [string, string][] : []),
    ["-", ""], ["newfile", "New file..."], ["newfolder", "New folder..."], ["refresh", "Refresh"],
    ...(n ? [["-", ""], ["save", `Save ${n > 1 ? `${n} items ` : ""}to this computer`], ["delete", `Delete ${n > 1 ? `${n} items ` : ""}permanently...`]] as [string, string][] : []),
  ];
  const selected: FilesMenuProps = { items, local: [], x, y, close: closeFilesMenu, action: act => {
    closeFilesMenu();

    if (act === "open") openPicked();
    else if (act === "preview") previewWorkFile(path);
    else if (act === "mention") paths.forEach(p => addMention(mentioned(paneRoot(), s.cwd, p)));
    else if (act === "abs" || act === "rel") {
      const text = paths.map(p => act === "abs" ? resolvePath(paneRoot(), p) : p).join("\n");
      writeClipboard(text).then(() => note(`Copied ${paths.length} paths`), err => note(`Could not copy: ${err}`));
    } else if (act === "copy" || act === "cut") clipboard(act === "cut");
    else if (act === "paste") void paste(dir);
    else if (act === "rename") renameItem();
    else if (act === "newfile" || act === "newfolder") createItem(act === "newfolder", dir);
    else if (act === "refresh") void refresh().catch(e => console.warn("files:", e));
    else if (act === "delete") deleteItems();
    else if (act === "save") void saveItems();
  } };
  setModel({ menu: selected });

  // A file this Mac has can be opened here, in another app, or shown in Finder.
  if (single && !folder) {
    void localFileItems({ machineId: s.machine_id, root: paneRoot(), path }, { x, y }).then(more => {
      if (!more.length || filesModel.getState().menu !== selected) return;
      setModel({ menu: { ...selected, local: more } });
    });
  }
}

const rowsNow = () => treeRows(current.listings, current.open, current.filter, threadsHere());

/// The folders the session's threads work in, which show with each thread and not here. The
/// tree shows the session's own folder: a worktree has none of them.
function threadsHere() {
  const s = paneSession();
  return s && paneRoot() === s.cwd ? threadFolders(sessions, s) : new Set<string>();
}
const visiblePaths = () => rowsNow().map(r => r.path);
const selectedPaths = () => selectionRoots(visiblePaths().filter(p => current.selected.has(p)));
function destination() {
  const row = rowsNow().find(r => r.path === current.picked);
  return row?.folder ? row.path : parentPath(row?.path ?? "");
}
const target = () => ({ machineId: paneSession()?.machine_id ?? "", root: paneRoot(), key: current.key });
const note = (text: string) => setModel({ status: text });
function openPicked() {
  if (!current.picked || current.selected.size !== 1) return;
  const row = rowsNow().find(r => r.path === current.picked);
  if (row?.folder) toggle(row.path); else openFile(current.picked);
}
const openFile = (path: string) => (hasPreview(path) ? previewWorkFile : openWorkFile)(path);

type FileAction = { CreateFile: { path: string } } | { CreateFolder: { path: string } }
  | { Rename: { path: string; to: string } } | { Copy: { paths: string[]; folder: string } }
  | { Move: { paths: string[]; folder: string } } | { Delete: { paths: string[] } };

async function edit(action: FileAction, at = target()) {
  if (busyNow()) throw new Error("Wait for the current file operation to finish.");
  if (current.key !== at.key) throw new Error("The working folder changed. Open the action again.");
  setModel({ busy: true });
  draw();
  note("Updating files...");

  try {
    await invoke("agent_tree_edit", { machineId: at.machineId, root: at.root, action });
    if (current.key === at.key) note("Files updated.");
  } finally {
    if (current.key === at.key) {
      setModel({ busy: false });
      Object.assign(view, { fresh: true, generation: view.generation + 1, walking: false });
      view.reading.clear();
      current.listings.clear();
      current.roots = new Set(["", ...current.open]);
      void refresh().catch(e => console.warn("files:", e));
    }
  }
}
function createItem(folder: boolean, dir = destination()) {
  if (busyNow()) return;
  const at = target();
  renameSheet({ title: folder ? "New folder" : "New file", value: "", run: async name => {
    const why = fileNameError(name);
    if (why) throw new Error(why);
    const path = joinPath(dir, name);
    await edit(folder ? { CreateFolder: { path } } : { CreateFile: { path } }, at);
  } });
}
function renameItem() {
  const paths = selectedPaths();
  if (busyNow() || current.selected.size !== 1 || paths.length !== 1) return;
  const path = paths[0], at = target();
  renameSheet({ title: "Rename", value: baseName(path), run: async name => {
    const why = fileNameError(name);
    if (why) throw new Error(why);
    await edit({ Rename: { path, to: joinPath(parentPath(path), name) } }, at);
  } });
}
function clipboard(cut: boolean) {
  const paths = selectedPaths();
  if (!paths.length || busyNow()) return;
  view.clipboard = { key: current.key, paths, cut };
  note(`${cut ? "Cut" : "Copied"} ${paths.length} items. Select a destination folder and paste.`);
}
async function paste(folder: string) {
  const clip = view.clipboard;
  if (busyNow() || !canPasteFiles(clip, current.key, folder) || !clip) return;

  try {
    await edit(clip.cut ? { Move: { paths: clip.paths, folder } } : { Copy: { paths: clip.paths, folder } });
    if (clip.cut && view.clipboard === clip) view.clipboard = null;
  } catch (e) { note(`Could not paste: ${e}`); }
}
function deleteItems() {
  const paths = selectedPaths(), at = target();
  if (!paths.length || busyNow()) return;
  const label = paths.length === 1 ? paths[0] : `${paths.length} items`;
  confirmSheet({ title: `Delete ${label} permanently?`, text: paths.join("\n"),
    note: "Folders include everything inside. This cannot be undone.", action: `Delete ${paths.length} ${paths.length === 1 ? "item" : "items"}`,
    run: async () => { await edit({ Delete: { paths } }, at); },
  });
}
async function saveItems() {
  const paths = selectedPaths(), at = target();
  if (!paths.length || busyNow()) return;
  if (paths.length === 1 && !rowsNow().find(r => r.path === paths[0])?.folder) { saveWorkFile(paths[0], note, at.root); return; }
  setModel({ busy: true });
  draw();
  note(`Saving ${paths.length} items...`);

  try {
    const to = await invoke<string>("agent_tree_save_many", { machineId: at.machineId, root: at.root, paths });
    if (current.key === at.key) note(`Saved to ${savedIn(to)}.`);
    await invoke("reveal_file", { path: to });
  } catch (e) { if (current.key === at.key) note(`Download stopped: ${e}. The save folder may contain incomplete files.`); }
  finally { if (current.key === at.key) { setModel({ busy: false }); draw(); } }
}
