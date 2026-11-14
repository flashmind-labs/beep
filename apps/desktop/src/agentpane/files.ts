// The Files tab of a coding agent session's pane: the session's working folder
// on its machine as a tree, read three levels at a time, what git ignores left
// out unless asked, git's marks beside what changed. The trees of the last few
// folders are kept: showing one again draws it as it was left and reads its
// folders again only when the folder's `files` topic said something changed.
// The view is `FilesView` in files-tree.tsx, drawn from `filesModel`.
import { createRef, type DragEvent, type KeyboardEvent, type MouseEvent } from "react";
import { createStore } from "zustand";
import type { FilesMenuProps } from "./files-menu.tsx";
import { writeClipboard } from "../ui/parts.tsx";
import { isHtmlFile } from "./html-preview-state.ts";
import { type Listing, type TreeRow, isTree, atEdge, unreadFolders, failedListing, mergeTree, parentPath, joinPath, treeRows } from "./files-state.ts";
import { relativeTo } from "./artifacts-state.ts";
import { resolvePath } from "./media-state.ts";
import { paneRoot, watchRoot, worktreesReady } from "./worktree.ts";
import { mentioned } from "./worktree-state.ts";
import { openWorkFile, previewWorkFile, paneSession, saveWorkFile, showFiles } from "./media.ts";
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

/// The tree of one folder, kept while another shows.
interface Kept {
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

/// Most recently shown last.
const kept = new Map<string, Kept>();

const F = {
  /// The session's machine and folder the tree is of, and what is kept of it.
  key: "",
  kept: null as Kept | null,
  listings: new Map<string, Listing>(),
  /// The folders the tree was read from, read again on refresh.
  roots: new Set<string>(),
  /// Folders being read now.
  reading: new Set<string>(),
  again: new Set<string>(),
  /// The node reads one folder at a time (from before trees).
  flat: false,
  /// A filter is reading the rest of the tree.
  walking: false,
  /// That read stopped at `WALK_ENTRIES`.
  capped: false,
  open: new Set<string>(),
  filter: "",
  ignored: false,
  picked: null as string | null,
  anchor: null as string | null,
  selected: new Set<string>(),
  clipboard: null as FileClipboard | null,
  busy: false,
  fresh: false,
  generation: 0,
  /// The tab drew once, so the tree has a key.
  made: false,
  /// A path to scroll into view once its row is drawn.
  reveal: null as string | null,
};

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

/// Folder reads run at once, and the entries a filter stops at.
const WALK_AT_ONCE = 8;
export const WALK_ENTRIES = 50_000;

/// The pane drew the Files tab: `on` when it is the dock's tab on screen,
/// `visible` when it can be seen. It shows the tree of the folder the pane reads.
export function filesTab(on: boolean, visible: boolean) {
  F.made = true;
  const key = treeKey();
  if (key !== F.key) showTree(key);
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
  if (F.made && key !== F.key) showTree(key);
}

const treeKey = () => {
  const s = paneSession();
  return s?.cwd && paneRoot() ? `${s.machine_id}\n${paneRoot()}` : "";
};

/// Keeps the tree shown and shows the one of `key`, kept or new.
function showTree(key: string) {
  closeFilesMenu();
  keep();
  const s = paneSession();
  let entry = kept.get(key) ?? null;

  if (!entry && key && s) {
    const made: Kept = { key, listings: new Map(), roots: new Set(), flat: false, open: new Set(), filter: "", ignored: F.ignored, picked: null, anchor: null, selected: new Set(), scroll: 0, unread: new Set(), dirty: false, unwatch: () => {} };
    made.unwatch = watchRoot(s.machine_id, paneRoot(), () => {
      made.dirty = true;
      if (F.kept === made) revalidate();
    });
    entry = made;
  }

  if (entry) { kept.delete(key); kept.set(key, entry); }
  Object.assign(F, {
    key, kept: entry, reading: new Set(), again: new Set(), walking: false, capped: false, busy: false, fresh: false, generation: F.generation + 1, reveal: null,
    listings: entry?.listings ?? new Map(), roots: entry?.roots ?? new Set(), flat: entry?.flat ?? false, open: entry?.open ?? new Set(),
    filter: entry?.filter ?? "", picked: entry?.picked ?? null, anchor: entry?.anchor ?? null, selected: entry?.selected ?? new Set(),
  });
  restoring = true;
  setModel({ label: "", active: null, status: "" });
  filesOnScreen = false;
  trim();
  draw();
  if (!entry) return;

  // Read what was never read or stopped, and all of it when it was read with ignored files shown otherwise.
  if (entry.ignored !== F.ignored) { entry.ignored = F.ignored; entry.dirty = true; }
  if (!F.listings.get("")?.entries) load("");
  for (const dir of entry.unread) load(dir);
  entry.unread.clear();
  if (F.filter.trim()) readAll();
}

/// Puts what the tree shows back in what is kept of it.
function keep() {
  const entry = F.kept;
  if (!entry) return;
  Object.assign(entry, { listings: F.listings, roots: F.roots, flat: F.flat, open: F.open, filter: F.filter, picked: F.picked, anchor: F.anchor, selected: F.selected });
  for (const dir of F.reading) entry.unread.add(dir);
  keepScroll();
}

/// What the tree scrolled to is kept, once it is back where it was left.
export function keepScroll() {
  const tree = treeRef.current;
  if (F.kept && !restoring && tree?.isConnected && tree.offsetParent) F.kept.scroll = tree.scrollTop;
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
export const keptScroll = () => F.kept?.scroll ?? 0;

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
    if (entry === F.kept) continue;
    entry.unwatch();
    kept.delete(entry.key);
  }
}

/// Whether the tree was on screen when last asked.
let filesOnScreen = false;
/// A folder that changes while its tree shows is read again at most this often.
const revalidate = ambient(() => F.kept?.dirty ? refresh() : undefined, AMBIENT_MS, () => filesShown());

/// The tab drew; `visible` says whether the tree is on screen. A tree that
/// comes on screen reads its folders again at once when the folder changed
/// since, one already on screen at a slower pace while the agent works.
function revalidateFiles(visible: boolean) {
  const now = visible && paneShown(), appeared = now && !filesOnScreen;
  filesOnScreen = now;
  if (now && F.kept?.dirty) revalidate(appeared);
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

  F.filter = "";

  for (let i = 1; i < parts.length; i++) {
    const dir = parts.slice(0, i).join("/");
    F.open.add(dir);
    if (!F.listings.get(dir)?.entries) load(dir);
  }

  F.picked = F.reveal = F.anchor = parts.join("/");
  F.selected = new Set([F.picked]);
  draw();
}

/// Reads `dir` again from the node when the tree has it, as after an upload into it.
export function reloadFolder(dir: string) {
  for (let d = dir.replace(/\/+$/, ""); ; d = parentPath(d)) {
    if (F.listings.has(d)) { load(d, false, true); return; }
    if (!d) return;
  }
}

/// Reads every folder the tree was read from again, in place, past the
/// native cache: only for a change, a refresh or an edit.
async function refresh() {
  const key = F.key, generation = F.generation;
  const same = () => F.key === key && F.generation === generation;
  const entry = F.kept;
  if (entry) entry.dirty = false;
  F.fresh = true;
  const roots = [...new Set([...F.roots, ...F.reading])];
  let read = false;

  try {
    for (let offset = 0; offset < roots.length; offset += WALK_AT_ONCE) {
      if (!same()) return;
      await Promise.all(roots.slice(offset, offset + WALK_AT_ONCE).map(dir => load(dir)));
    }
    read = same();
  } finally {
    if (same()) F.fresh = false;
    // A read that stopped short leaves the tree as stale as before, so the next show reads again.
    if (!read && entry) entry.dirty = true;
  }

  if (!same()) return;
  const error = [...F.listings.values()].find(listing => listing.error)?.error;
  if (error) {
    if (entry) entry.dirty = true;
    throw new Error(error);
  }
}

/// Whether the tree can be seen: its tab is picked (a view behind another
/// is `hidden` but keeps its layout) in a dock not folded (`offsetParent` is
/// null under one), and the pane shows.
const filesShown = () => !!filesRef.current?.offsetParent && !filesRef.current.hidden && paneShown();

/// Reads the rest of the tree while a filter is typed, so it finds what no
/// folder opened yet shows.
async function readAll() {
  if (F.walking) return;
  const key = F.key, generation = F.generation;
  F.walking = true;
  draw();

  while (F.key === key && F.generation === generation && F.filter.trim()) {
    let entries = 0;
    for (const listing of F.listings.values()) entries += listing.entries?.length ?? 0;
    F.capped = entries >= WALK_ENTRIES;
    const next = unreadFolders(F.listings).slice(0, WALK_AT_ONCE);
    if (F.capped || !next.length) break;
    await Promise.all(next.map(dir => load(dir, true)));
  }

  if (F.key !== key || F.generation !== generation) return;
  F.walking = false;
  draw();
}

/// Reads the tree from `dir` down and merges it in. `deep` is a filter's
/// read: as far as one read goes, and not read again on refresh. `fresh`
/// asks the node past the copy of the folder this computer may have.
const reads = new Map<string, Promise<void>>();
function load(dir: string, deep = false, fresh = false): Promise<void> {
  const key = F.key, generation = F.generation;
  const token = JSON.stringify([key, generation, dir]);
  const pending = reads.get(token);
  if (pending) { if (!deep) F.again.add(dir); return pending; }
  const read = (async () => {
    do {
      F.again.delete(dir);
      await readFolder(dir, deep, fresh);
      deep = false;
    } while (F.key === key && F.generation === generation && F.again.has(dir));
  })().finally(() => reads.delete(token));
  reads.set(token, read);
  return read;
}

async function readFolder(dir: string, deep: boolean, fresh: boolean) {
  const key = F.key, generation = F.generation;
  await worktreesReady();
  if (F.key !== key || F.generation !== generation) return;
  const s = paneSession();
  // A session with no folder yet has nothing to read.
  if (!s?.cwd) return;
  const had = F.listings.get(dir);
  F.listings.set(dir, { entries: had?.entries ?? null, more: had?.more ?? 0, error: "" });
  F.reading.add(dir);
  // A folder shown stays as it is while it is read again.
  if (!had?.entries) draw();
  let changed = true;

  try {
    const got = await invoke<unknown>("agent_tree", { machineId: s.machine_id, root: paneRoot(), path: dir, ignored: F.ignored, deep, fresh: fresh || F.fresh });
    if (F.key !== key || F.generation !== generation) return;
    if (!isTree(got)) throw new Error("The machine sent no file listing.");
    changed = mergeTree(F.listings, got);
    if (!deep) F.roots.add(dir);
    const top = got.folders[0];
    F.flat ||= got.folders.length === 1 && !got.truncated && !!top?.entries.some(e => e.folder && !e.ignored);
  } catch (e) {
    if (F.key !== key || F.generation !== generation) return;
    F.listings.set(dir, failedListing(String(e instanceof Error ? e.message : e), had));
  } finally {
    if (F.key === key && F.generation === generation) {
      F.reading.delete(dir);
    }
  }

  if (changed || !had?.entries) draw();
  if (F.key === key && F.generation === generation && F.filter.trim()) readAll();
}

function draw() {
  if (!paneSession()?.cwd) { showingRows = false; setModel({ tree: null }); return; }
  const top = F.listings.get("");
  const rows = treeRows(F.listings, F.open, F.filter);
  const visible = new Set(rows.map(row => row.path));
  F.selected = new Set([...F.selected].filter(path => visible.has(path)));
  if (F.picked && !visible.has(F.picked)) F.picked = null;
  const label = F.busy ? filesModel.getState().label : F.selected.size ? `${F.selected.size} selected` : "";
  const shell = { root: paneRoot(), filter: F.filter, ignored: F.ignored, busy: F.busy, label, selected: F.selected, active: F.picked ? rowId(F.picked) : null };

  let message: string | null = null, error = false, loading = false;
  if (top?.error && !top.entries) { message = top.error; error = true; }
  else if (!top?.entries && F.reading.has("") && showingRows) { setModel(shell); return; }
  else if (!top?.entries) { message = "Reading the folder…"; loading = true; }
  else if (!rows.length && F.walking && F.filter.trim()) { message = "Reading folders…"; loading = true; }
  else if (!rows.length && !F.capped) message = F.filter ? "No files match." : "This folder is empty.";
  const searching = !!F.filter.trim() && !!top?.entries && !top.error;
  showingRows = message === null;
  const tree: TreeView = message !== null ? { kind: "message", text: message, error, loading }
    : { kind: "rows", rows, more: top?.more ?? 0, walking: searching && F.walking && !!rows.length, capped: searching && !F.walking && F.capped };
  setModel({ ...shell, tree, reveal: F.reveal });
}

/// The element id of the row of `path`.
export const rowId = (path: string) => `agfile-${encodeURIComponent(path)}`;

/// The tree drew the row `F.reveal` asked for.
export function revealed() {
  restoring = false;
  F.reveal = null;
  setModel({ reveal: null });
}

function toggle(path: string) {
  if (F.open.has(path)) F.open.delete(path);
  else {
    F.open.add(path);
    // Read on from a folder at the edge: shown at once when it was read.
    if (!F.listings.get(path)?.entries || (!F.flat && atEdge(F.listings, path))) load(path);
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
  F.filter = value;
  draw();
  if (F.filter.trim()) readAll();
}

/// Show ignored was ticked or not.
export function setIgnored(on: boolean) {
  F.ignored = on;
  if (F.kept) F.kept.ignored = F.ignored;
  setModel({ ignored: on });
  void refresh().catch(e => console.warn("files:", e));
}

export function refreshFiles() {
  void refresh().catch(e => console.warn("files:", e));
}

/// What the tree does with the pointer, the keys and drags; a row's path is
/// on its `data-fpath`.
export const treeActions = {
  click(e: MouseEvent<HTMLElement>) {
    closeFilesMenu();
    e.currentTarget.focus();
    const row = rowAt(e.target);
    const path = row?.dataset.fpath;
    if (!row || !path) { F.selected.clear(); F.picked = F.anchor = null; draw(); return; }
    Object.assign(F, selectFile(F, visiblePaths(), path, e.metaKey || e.ctrlKey, e.shiftKey));
    if (e.metaKey || e.ctrlKey || e.shiftKey) { draw(); return; }

    if (row.dataset.ffolder) toggle(path);
    else { draw(); openWorkFile(path); }
  },
  contextMenu(e: MouseEvent<HTMLElement>) {
    e.preventDefault();
    e.currentTarget.focus();
    const row = rowAt(e.target);
    const path = row?.dataset.fpath ?? "";

    if (path && !F.selected.has(path)) Object.assign(F, selectFile(F, visiblePaths(), path));
    else if (!path) { F.selected.clear(); F.picked = F.anchor = null; }

    draw();
    menu(path, !!row?.dataset.ffolder, e.clientX, e.clientY);
  },
  keyDown(e: KeyboardEvent<HTMLElement>) {
    if (e.nativeEvent.isComposing || (e.target instanceof Element && e.target.closest("input,textarea,[contenteditable=true]"))) return;
    const cmd = e.metaKey || e.ctrlKey, key = e.key.toLowerCase();
    let handled = true;

    if (cmd && key === "a") { F.selected = new Set(visiblePaths()); draw(); }
    else if (cmd && (key === "c" || key === "x")) clipboard(key === "x");
    else if (cmd && key === "v") void paste(destination());
    else if ((cmd && (key === "backspace" || key === "delete")) || (!cmd && key === "delete")) deleteItems();
    else if (key === "escape") { closeFilesMenu(); F.selected.clear(); F.picked = F.anchor = null; draw(); }
    else if (key === "enter" || key === "f2") renameItem();
    else if (key === " " && F.picked) openPicked();
    else if (key === "arrowup" || key === "arrowdown" || key === "home" || key === "end") {
      const paths = visiblePaths(), at = F.picked ? paths.indexOf(F.picked) : -1;
      const next = key === "home" ? 0 : key === "end" ? paths.length - 1 : Math.max(0, Math.min(paths.length - 1, at + (key === "arrowup" ? -1 : 1)));
      if (paths[next]) { Object.assign(F, selectFile(F, paths, paths[next], false, e.shiftKey)); F.reveal = paths[next]; draw(); }
    } else if ((key === "arrowright" || key === "arrowleft") && F.picked) {
      const row = rowsNow().find(r => r.path === F.picked);
      if (row?.folder && row.open === (key === "arrowleft")) toggle(row.path);
    } else if (key === "contextmenu" || e.shiftKey && key === "f10") {
      // At the first selected row, or the tree's corner while that row is scrolled away.
      const first = rowsNow().find(row => F.selected.has(row.path)) ?? null;
      const box = ((first && document.getElementById(rowId(first.path))) ?? e.currentTarget).getBoundingClientRect();
      menu(F.picked ?? "", !!first?.folder, box.left, box.bottom);
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
  const paths = selectedPaths(), n = paths.length;
  const dir = path && folder ? path : parentPath(path);
  const items: [string, string][] = [
    ...(n === 1 && F.selected.size === 1 ? [["open", folder ? "Open folder" : "Open in viewer"]] as [string, string][] : []),
    ...(n === 1 && F.selected.size === 1 && !folder && isHtmlFile(path) ? [["preview", "Preview"]] as [string, string][] : []),
    ...(n ? [["mention", `Mention ${n > 1 ? `${n} items` : "in message"}`], ["-", ""], ["abs", "Copy path"], ["rel", "Copy relative path"], ["copy", `Copy${n > 1 ? ` ${n} items` : ""}`], ["cut", `Cut${n > 1 ? ` ${n} items` : ""}`]] as [string, string][] : []),
    ...(canPasteFiles(F.clipboard, F.key, dir) ? [["paste", `Paste ${F.clipboard!.paths.length} items into ${dir || "working folder"}`]] as [string, string][] : []),
    ...(n === 1 && F.selected.size === 1 ? [["rename", "Rename..."]] as [string, string][] : []),
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
  if (n === 1 && F.selected.size === 1 && !folder) {
    void localFileItems({ machineId: s.machine_id, root: paneRoot(), path }, { x, y }).then(more => {
      if (!more.length || filesModel.getState().menu !== selected) return;
      setModel({ menu: { ...selected, local: more } });
    });
  }
}

const rowsNow = () => treeRows(F.listings, F.open, F.filter);
const visiblePaths = () => rowsNow().map(r => r.path);
const selectedPaths = () => selectionRoots(visiblePaths().filter(p => F.selected.has(p)));
function destination() {
  const row = rowsNow().find(r => r.path === F.picked);
  return row?.folder ? row.path : parentPath(row?.path ?? "");
}
function target() {
  return { machineId: paneSession()?.machine_id ?? "", root: paneRoot(), key: F.key };
}
function note(text: string) {
  setModel({ status: text });
}
function openPicked() {
  if (!F.picked || F.selected.size !== 1) return;
  const row = rowsNow().find(r => r.path === F.picked);
  if (row?.folder) toggle(row.path); else openWorkFile(F.picked);
}

type FileAction = { CreateFile: { path: string } } | { CreateFolder: { path: string } }
  | { Rename: { path: string; to: string } } | { Copy: { paths: string[]; folder: string } }
  | { Move: { paths: string[]; folder: string } } | { Delete: { paths: string[] } };

async function edit(action: FileAction, at = target()) {
  if (F.busy) throw new Error("Wait for the current file operation to finish.");
  if (F.key !== at.key) throw new Error("The working folder changed. Open the action again.");
  F.busy = true;
  draw();
  note("Updating files...");

  try {
    await invoke("agent_tree_edit", { machineId: at.machineId, root: at.root, action });
    if (F.key === at.key) note("Files updated.");
  } finally {
    if (F.key === at.key) {
      F.busy = false;
      F.fresh = true;
      F.generation++;
      F.walking = false;
      F.reading.clear();
      F.listings.clear();
      F.roots = new Set(["", ...F.open]);
      void refresh().catch(e => console.warn("files:", e));
    }
  }
}
function createItem(folder: boolean, dir = destination()) {
  if (F.busy) return;
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
  if (F.busy || F.selected.size !== 1 || paths.length !== 1) return;
  const path = paths[0], at = target();
  renameSheet({ title: "Rename", value: path.split("/").pop()!, run: async name => {
    const why = fileNameError(name);
    if (why) throw new Error(why);
    await edit({ Rename: { path, to: joinPath(parentPath(path), name) } }, at);
  } });
}
function clipboard(cut: boolean) {
  const paths = selectedPaths();
  if (!paths.length || F.busy) return;
  F.clipboard = { key: F.key, paths, cut };
  note(`${cut ? "Cut" : "Copied"} ${paths.length} items. Select a destination folder and paste.`);
}
async function paste(folder: string) {
  const clip = F.clipboard;
  if (F.busy || !canPasteFiles(clip, F.key, folder) || !clip) return;

  try {
    await edit(clip.cut ? { Move: { paths: clip.paths, folder } } : { Copy: { paths: clip.paths, folder } });
    if (clip.cut && F.clipboard === clip) F.clipboard = null;
  } catch (e) { note(`Could not paste: ${e}`); }
}
function deleteItems() {
  const paths = selectedPaths(), at = target();
  if (!paths.length || F.busy) return;
  const label = paths.length === 1 ? paths[0] : `${paths.length} items`;
  confirmSheet({ title: `Delete ${label} permanently?`, text: paths.join("\n"),
    note: "Folders include everything inside. This cannot be undone.", action: `Delete ${paths.length} ${paths.length === 1 ? "item" : "items"}`,
    run: async () => { await edit({ Delete: { paths } }, at); },
  });
}
async function saveItems() {
  const paths = selectedPaths(), at = target();
  if (!paths.length || F.busy) return;
  if (paths.length === 1 && !rowsNow().find(r => r.path === paths[0])?.folder) { saveWorkFile(paths[0], note, at.root); return; }
  F.busy = true;
  draw();
  note(`Saving ${paths.length} items...`);

  try {
    const to = await invoke<string>("agent_tree_save_many", { machineId: at.machineId, root: at.root, paths });
    if (F.key === at.key) note(`Saved to ${savedIn(to)}.`);
    await invoke("reveal_file", { path: to });
  } catch (e) { if (F.key === at.key) note(`Download stopped: ${e}. The save folder may contain incomplete files.`); }
  finally { if (F.key === at.key) { F.busy = false; draw(); } }
}
