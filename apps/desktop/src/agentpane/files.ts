import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { FilesTree } from "./files-tree.tsx";
import { isHtmlFile } from "./html-preview-state.ts";
import { subscribeChanges as subscribeTopics, topic } from "../ui/mesh-events.ts";
// The Files tab of a coding agent session's pane: the session's working folder
// on its machine as a tree, read three levels at a time, what git ignores left
// out unless asked, git's marks beside what changed.

import { type Listing, type Tree, atEdge, unreadFolders, failedListing, mergeTree, parentPath, joinPath, treeRows } from "./files-state.ts";
import { relativeTo } from "./artifacts-state.ts";
import { resolvePath } from "./media-state.ts";
import { paneRoot, worktreesReady } from "./worktree.ts";
import { mentioned } from "./worktree-state.ts";
import { openWorkFile, previewWorkFile, paneSession, saveWorkFile, showFiles } from "./media.ts";
import { localFileItems } from "./localfile.ts";
import { addMention, askUpload } from "../agents/workdrop.ts";
import { selectFile, selectionRoots, fileNameError, canPasteFiles, type FileClipboard } from "./file-actions-state.ts";
import { confirmSheet, renameSheet, arrowFocus } from "../ui/menu.ts";
import { esc } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";
import { paneShown } from "../ui/active.ts";

/// The type a row of the tree drags as, its path relative to the session's folder.
export const PATH_TYPE = "application/x-beep-path";

const F = {
  /// The session's machine and folder the tree is of.
  key: "",
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
  unfollow: null as (() => void) | null,
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
  el: null as HTMLElement | null,
  /// A path to scroll into view once its row is drawn.
  reveal: null as string | null,
};

let treeRoot: Root | null = null;
let showingRows = false;

/// Folder reads run at once, and the entries a filter stops at.
const WALK_AT_ONCE = 8;
const WALK_ENTRIES = 50_000;

/// The Files view of the pane's session, made once per session.
export function filesView() {
  const s = paneSession();
  const key = s ? `${s.machine_id}\n${paneRoot()}` : "";
  if (F.el && F.key === key) return F.el;
  treeRoot?.unmount(); treeRoot = null; showingRows = false;
  F.el?.remove();
  F.unfollow?.();
  Object.assign(F, {
    key, listings: new Map(), roots: new Set(), reading: new Set(), again: new Set(), flat: false, walking: false, capped: false, open: new Set(), filter: "", picked: null, anchor: null, selected: new Set(), busy: false, fresh: false, generation: F.generation + 1, reveal: null,
    unfollow: s?.machine_id && paneRoot() ? subscribeTopics(topic("files", paneRoot()), () => refresh(), s.machine_id, filesShown) : null,
  });
  const el = h(`<div class="agpview files"><div class="agfbar"><input class="agffind" placeholder="Filter files" aria-label="Filter files" spellcheck="false">`
    + `<label class="agfign"><input type="checkbox"${F.ignored ? " checked" : ""}> Show ignored</label>`
    + `<button class="agpbtn" data-frefresh="1" title="Read the folders again">&#8635;</button></div>`
    + `<div class="agtree" role="tree" aria-label="Working files" aria-multiselectable="true" tabindex="0"></div>`
    + `<div class="agpacts"><span class="meta mono" title="${esc(paneRoot())}">${esc(paneRoot())}</span><span data-fstatus role="status"></span><span class="sp"></span><span class="hint">Cmd-click to select, Shift-click for a range. Cmd+Delete asks before deleting.</span></div></div>`);
  F.el = el;
  wire(el);
  const tree = el.querySelector<HTMLElement>(".agtree");
  if (tree) treeRoot = createRoot(tree);
  load("");
  return el;
}

/// Closes the menu a right-click opened; true when one was open.
export function closeFilesMenu() {
  const m = F.el?.querySelector(".agctx");
  const focused = !!m && m.contains(document.activeElement);
  m?.remove();
  if (focused) F.el?.querySelector<HTMLElement>(".agtree")?.focus({ preventScroll: true });
  return !!m;
}

/// Reads the tree of the worktree the pane shows now, in place: the filter
/// and what git ignores stay as they are, the open file tabs are not touched.
export function pointFilesAt() {
  const s = paneSession();
  const key = s ? `${s.machine_id}\n${paneRoot()}` : "";
  if (!F.el || !s || key === F.key) return;
  closeFilesMenu();
  F.unfollow?.();
  F.unfollow = s.machine_id && paneRoot() ? subscribeTopics(topic("files", paneRoot()), () => refresh(), s.machine_id, filesShown) : null;
  Object.assign(F, {
    key, listings: new Map(), roots: new Set(), reading: new Set(), again: new Set(), flat: false, walking: false, capped: false, open: new Set(), picked: null, anchor: null, selected: new Set(), busy: false, fresh: false, generation: F.generation + 1, reveal: null,
  });
  const meta = F.el.querySelector(".agpacts .meta");
  if (meta) { meta.textContent = paneRoot(); meta.setAttribute("title", paneRoot()); }
  load("");
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

  if (F.filter) {
    F.filter = "";
    const find = F.el?.querySelector<HTMLInputElement>(".agffind");
    if (find) find.value = "";
  }

  for (let i = 1; i < parts.length; i++) {
    const dir = parts.slice(0, i).join("/");
    F.open.add(dir);
    if (!F.listings.get(dir)?.entries) load(dir);
  }

  F.picked = F.reveal = F.anchor = parts.join("/");
  F.selected = new Set([F.picked]);
  draw();
}

/// Reads `dir` again when the tree has it, as after an upload into it.
export function reloadFolder(dir: string) {
  for (let d = dir.replace(/\/+$/, ""); ; d = parentPath(d)) {
    if (F.listings.has(d)) { load(d); return; }
    if (!d) return;
  }
}

/// Reads every folder the tree was read from again, in place.
async function refresh() {
  const key = F.key, generation = F.generation;
  F.fresh = true;
  const roots = [...new Set([...F.roots, ...F.reading])];

  for (let offset = 0; offset < roots.length; offset += WALK_AT_ONCE) {
    if (F.key !== key || F.generation !== generation) return;
    await Promise.all(roots.slice(offset, offset + WALK_AT_ONCE).map(dir => load(dir)));
  }

  if (F.key !== key || F.generation !== generation) return;
  const error = [...F.listings.values()].find(listing => listing.error)?.error;
  if (error) throw new Error(error);
}

/// Whether the tree can be seen: its tab is picked in a dock not folded
/// (`offsetParent` is null under a hidden one), and the pane shows.
const filesShown = () => !!F.el?.offsetParent && paneShown();

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
/// read: as far as one read goes, and not read again on refresh.
const reads = new Map<string, Promise<void>>();
function load(dir: string, deep = false): Promise<void> {
  const key = F.key, generation = F.generation;
  const token = JSON.stringify([key, generation, dir]);
  const pending = reads.get(token);
  if (pending) { if (!deep) F.again.add(dir); return pending; }
  const read = (async () => {
    do {
      F.again.delete(dir);
      await readFolder(dir, deep);
      deep = false;
    } while (F.key === key && F.generation === generation && F.again.has(dir));
  })().finally(() => reads.delete(token));
  reads.set(token, read);
  return read;
}

async function readFolder(dir: string, deep: boolean) {
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
    const got = await invoke<Tree>("agent_tree", { machineId: s.machine_id, root: paneRoot(), path: dir, ignored: F.ignored, deep, fresh: F.fresh });
    if (F.key !== key || F.generation !== generation) return;
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
  const tree = F.el?.querySelector<HTMLElement>(".agtree");
  if (!tree || !treeRoot) return;
  if (!paneSession()?.cwd) { flushSync(() => treeRoot?.render(null)); showingRows = false; return; }
  const top = F.listings.get("");
  const rows = treeRows(F.listings, F.open, F.filter);
  const visible = new Set(rows.map(row => row.path));
  F.selected = new Set([...F.selected].filter(path => visible.has(path)));
  if (F.picked && !visible.has(F.picked)) F.picked = null;
  tree.setAttribute("aria-busy", String(F.busy));
  if (F.picked) tree.setAttribute("aria-activedescendant", `agfile-${encodeURIComponent(F.picked)}`);
  else tree.removeAttribute("aria-activedescendant");
  const count = F.el?.querySelector(".agpacts .meta");
  if (count && !F.busy) count.textContent = F.selected.size ? `${F.selected.size} selected` : paneRoot();

  let message: string | null = null, error = false, loading = false;
  if (top?.error && !top.entries) { message = top.error; error = true; }
  else if (!top?.entries && F.reading.has("") && showingRows) return;
  else if (!top?.entries) { message = "Reading the folder…"; loading = true; }
  else if (!rows.length && F.walking && F.filter.trim()) { message = "Reading folders…"; loading = true; }
  else if (!rows.length && !F.capped) message = F.filter ? "No files match." : "This folder is empty.";
  const searching = !!F.filter.trim() && !!top?.entries && !top.error;
  showingRows = message === null;
  flushSync(() => treeRoot?.render(createElement(FilesTree, {
    rows, selected: F.selected, message, error, more: message === null ? top?.more ?? 0 : 0,
    walking: message !== null ? loading : searching && F.walking && !!rows.length,
    capped: message === null && searching && !F.walking && F.capped, limit: WALK_ENTRIES,
  })));
  const shown = F.reveal ? tree.querySelector<HTMLElement>(`[data-fpath="${CSS.escape(F.reveal)}"]`) : null;
  if (shown) { shown.scrollIntoView({ block: "center" }); F.reveal = null; }
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

/// The folder a drop on `row` goes into: the folder itself, or a file's.
function dropFolder(row: HTMLElement | null) {
  if (!row?.dataset.fpath) return "";
  return row.dataset.ffolder ? row.dataset.fpath : parentPath(row.dataset.fpath);
}

function wire(el: HTMLElement) {
  const tree = el.querySelector<HTMLElement>(".agtree")!;
  const find = el.querySelector<HTMLInputElement>(".agffind")!;
  find.addEventListener("input", () => { F.filter = find.value; draw(); if (F.filter.trim()) readAll(); });
  find.addEventListener("keydown", e => { if (e.key === "Escape" && find.value) { e.stopPropagation(); e.preventDefault(); find.value = ""; F.filter = ""; draw(); } });
  el.querySelector<HTMLInputElement>(".agfign input")!.addEventListener("change", e => {
    F.ignored = (e.target as HTMLInputElement).checked;
    void refresh().catch(e => console.warn("files:", e));
  });
  el.querySelector("[data-frefresh]")!.addEventListener("click", refresh);
  tree.addEventListener("click", e => {
    closeFilesMenu();
    tree.focus();
    const row = (e.target as HTMLElement).closest<HTMLElement>("[data-fpath]");
    if (!row) { F.selected.clear(); F.picked = F.anchor = null; draw(); return; }
    const path = row.dataset.fpath!;
    Object.assign(F, selectFile(F, visiblePaths(), path, e.metaKey || e.ctrlKey, e.shiftKey));
    if (e.metaKey || e.ctrlKey || e.shiftKey) { draw(); return; }

    if (row.dataset.ffolder) toggle(path);
    else { draw(); openWorkFile(path); }
  });
  tree.addEventListener("contextmenu", e => {
    e.preventDefault();
    tree.focus();
    const row = (e.target as HTMLElement).closest<HTMLElement>("[data-fpath]");
    const path = row?.dataset.fpath ?? "";

    if (path && !F.selected.has(path)) Object.assign(F, selectFile(F, visiblePaths(), path));
    else if (!path) { F.selected.clear(); F.picked = F.anchor = null; }

    draw();
    menu(path, !!row?.dataset.ffolder, e.clientX, e.clientY);
  });
  tree.addEventListener("keydown", e => {
    if (e.isComposing || (e.target as HTMLElement).closest("input,textarea,[contenteditable=true]")) return;
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
      const row = tree.querySelector<HTMLElement>(".agtr.on"), box = row?.getBoundingClientRect() ?? tree.getBoundingClientRect();
      menu(F.picked ?? "", !!row?.dataset.ffolder, box.left, box.bottom);
    } else handled = false;

    if (handled) { e.preventDefault(); e.stopPropagation(); }
  });
  tree.addEventListener("dragstart", e => {
    const row = (e.target as HTMLElement).closest<HTMLElement>("[data-fpath]");
    if (!row || !e.dataTransfer) return;
    const named = mentioned(paneRoot(), paneSession()?.cwd ?? "", row.dataset.fpath!);
    e.dataTransfer.setData(PATH_TYPE, named);
    e.dataTransfer.setData("text/plain", named);
    e.dataTransfer.effectAllowed = "copy";
  });
  // Files from Finder dropped on a folder (or a file's folder, or the
  // tree's empty space for the top) upload there, once the user says so.
  const over = (row: HTMLElement | null) => {
    tree.querySelectorAll(".agtr.drop").forEach(r => r.classList.remove("drop"));
    const dir = dropFolder(row);
    if (dir) tree.querySelector<HTMLElement>(`[data-fpath="${CSS.escape(dir)}"]`)?.classList.add("drop");
    tree.classList.toggle("dropping", !dir);
  };
  tree.addEventListener("dragover", e => {
    if (!e.dataTransfer?.types.includes("Files")) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = "copy";
    over((e.target as HTMLElement).closest<HTMLElement>("[data-fpath]"));
  });
  tree.addEventListener("dragleave", e => {
    if (!tree.contains(e.relatedTarget as Node | null)) { over(null); tree.classList.remove("dropping"); }
  });
  tree.addEventListener("drop", e => {
    if (!e.dataTransfer?.files.length) return;
    e.preventDefault();
    e.stopPropagation();
    const dir = dropFolder((e.target as HTMLElement).closest<HTMLElement>("[data-fpath]"));
    over(null);
    tree.classList.remove("dropping");
    document.getElementById("ddrop")?.remove();
    askUpload([...e.dataTransfer.files], dir, paneRoot());
  });
}

/// The right-click menu of `path`.
function menu(path: string, folder: boolean, x: number, y: number) {
  closeFilesMenu();
  const s = paneSession();
  if (!F.el || !s) return;
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
  const m = h(`<div class="agctx" role="menu" aria-label="File actions">${items.map(([k, t]) => k === "-" ? "<hr>" : `<button role="menuitem" data-fact="${k}"${k === "delete" ? ' class="warn"' : ""}>${esc(t)}</button>`).join("")}</div>`);
  F.el.append(m);
  m.style.maxHeight = `${F.el.clientHeight}px`;
  m.style.overflowY = "auto";
  const box = F.el.getBoundingClientRect();
  m.style.left = `${Math.max(0, Math.min(x - box.left, box.width - m.offsetWidth - 4))}px`;
  m.style.top = `${Math.max(0, Math.min(y - box.top, box.height - m.offsetHeight - 4))}px`;
  m.querySelector<HTMLElement>("button")?.focus();

  // A file this Mac has can be opened here, in another app, or shown in Finder.
  if (n === 1 && F.selected.size === 1 && !folder) {
    void localFileItems({ machineId: s.machine_id, root: paneRoot(), path }, { x, y }).then(more => {
      if (!more.length || !m.isConnected) return;
      m.append(document.createElement("hr"));

      for (const item of more) {
        const b = h(`<button role="menuitem">${esc(item.label)}</button>`);
        b.addEventListener("click", e => { e.stopPropagation(); closeFilesMenu(); item.on(); });
        m.append(b);
      }
    });
  }

  m.addEventListener("click", e => {
    const act = (e.target as HTMLElement).closest<HTMLElement>("[data-fact]")?.dataset.fact;
    if (!act) return;
    e.stopPropagation();
    closeFilesMenu();

    if (act === "open") openPicked();
    else if (act === "preview") previewWorkFile(path);
    else if (act === "mention") paths.forEach(p => addMention(mentioned(paneRoot(), s.cwd, p)));
    else if (act === "abs" || act === "rel") {
      const text = paths.map(p => act === "abs" ? resolvePath(paneRoot(), p) : p).join("\n");
      navigator.clipboard.writeText(text).then(() => note(`Copied ${paths.length} paths`), err => note(`Could not copy: ${err}`));
    } else if (act === "copy" || act === "cut") clipboard(act === "cut");
    else if (act === "paste") void paste(dir);
    else if (act === "rename") renameItem();
    else if (act === "newfile" || act === "newfolder") createItem(act === "newfolder", dir);
    else if (act === "refresh") void refresh().catch(e => console.warn("files:", e));
    else if (act === "delete") deleteItems();
    else if (act === "save") void saveItems();
  });
  m.addEventListener("keydown", e => {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); closeFilesMenu(); F.el?.querySelector<HTMLElement>(".agtree")?.focus(); return; }
    if (arrowFocus(m, e.key)) e.preventDefault();
  });
  setTimeout(() => document.addEventListener("pointerdown", function away(ev) {
    if (!m.isConnected || !m.contains(ev.target as Node)) { m.remove(); document.removeEventListener("pointerdown", away); }
  }), 0);
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
  const status = F.el?.querySelector("[data-fstatus]");
  if (status) status.textContent = text;
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
    if (F.key === at.key) note("Saved to Downloads.");
    await invoke("reveal_file", { path: to });
  } catch (e) { if (F.key === at.key) note(`Download stopped: ${e}. Downloads may contain incomplete files.`); }
  finally { if (F.key === at.key) { F.busy = false; draw(); } }
}
