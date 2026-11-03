// The Files tab of a coding agent session's pane: the session's working
// folder on its machine as a tree, read a folder at a time as folders
// open, what git ignores left out unless asked, git's marks beside what
// changed. A click opens a file in the pane; a row drags into the message
// as a mention; files dropped on a folder upload there; right-click has
// the rest.

import { type GitChange, type Listing, type TreeFolder, type TreeRow, parentPath, treeRows } from "../files-state.ts";
import { resolvePath } from "../media-state.ts";
import { esc, h, invoke } from "./core.ts";
import { openWorkFile, paneSession, saveWorkFile } from "./mediapane.ts";
import { addMention, askUpload } from "./workdrop.ts";

/// The type a row of the tree drags as, its path relative to the session's folder.
export const PATH_TYPE = "application/x-beep-path";

const F = {
  /// The session's machine and folder the tree is of.
  key: "",
  listings: new Map<string, Listing>(),
  open: new Set<string>(),
  filter: "",
  ignored: false,
  git: false,
  picked: null as string | null,
  el: null as HTMLElement | null,
  note: "",
};

const MARKS: Record<GitChange, [string, string]> = {
  untracked: ["U", "Untracked"],
  added: ["A", "Added"],
  modified: ["M", "Changed"],
  conflicted: ["C", "In conflict"],
};
const FOLDER = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>';
const FILE = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/></svg>';
const TWIST = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>';
export const FILES_ICON = FOLDER;

/// The Files view of the pane's session, made once per session.
export function filesView() {
  const s = paneSession();
  const key = s ? `${s.machine_id}\n${s.cwd}` : "";
  if (F.el && F.key === key) return F.el;
  F.el?.remove();
  F.key = key;
  F.listings = new Map();
  F.open = new Set();
  F.filter = "";
  F.picked = null;
  F.note = "";
  const el = h(`<div class="agpview files"><div class="agfbar"><input class="agffind" placeholder="Filter files" aria-label="Filter files" spellcheck="false">`
    + `<label class="agfign"><input type="checkbox"${F.ignored ? " checked" : ""}> Show ignored</label>`
    + `<button class="agpbtn" data-frefresh="1" title="Read the folders again">&#8635;</button></div>`
    + `<div class="agtree" role="tree" tabindex="0"></div>`
    + `<div class="agpacts"><span class="meta mono" title="${esc(s?.cwd ?? "")}">${esc(s?.cwd ?? "")}</span><span class="sp"></span><span class="hint">Drag a file into the message, or drop files on a folder to upload</span></div></div>`);
  F.el = el;
  wire(el);
  load("");
  return el;
}

/// Closes the menu a right-click opened; true when one was open.
export function closeFilesMenu() {
  const m = F.el?.querySelector(".agctx");
  m?.remove();
  return !!m;
}

/// Reads `dir` again when the tree has it, as after an upload into it.
export function reloadFolder(dir: string) {
  for (let d = dir.replace(/\/+$/, ""); ; d = parentPath(d)) {
    if (F.listings.has(d)) { load(d); return; }
    if (!d) return;
  }
}

async function load(dir: string) {
  const s = paneSession();
  if (!s) return;
  const key = F.key;
  const had = F.listings.get(dir);
  F.listings.set(dir, { entries: had?.entries ?? null, more: had?.more ?? 0, error: "" });
  draw();
  try {
    const got = await invoke<TreeFolder>("agent_tree", { machineId: s.machine_id, root: s.cwd, path: dir, ignored: F.ignored });
    if (F.key !== key) return;
    F.listings.set(dir, { entries: got.entries, more: got.more, error: "" });
    if (!dir) F.git = got.git;
  } catch (e) {
    if (F.key !== key) return;
    const text = String(e instanceof Error ? e.message : e).replace(/^bad request: /, "");
    F.listings.set(dir, { entries: had?.entries ?? null, more: 0, error: /unknown op|not serve|update/i.test(text) ? "Update Beep's server and beep-node on that machine to browse its files." : text });
  }
  draw();
}

function rowHtml(r: TreeRow) {
  const mark = r.change ? `<span class="gm ${r.change}" title="${MARKS[r.change][1]}">${MARKS[r.change][0]}</span>` : "";
  const twist = r.folder ? `<span class="tw${r.open ? " open" : ""}">${r.loading ? '<span class="dspin"></span>' : TWIST}</span>` : '<span class="tw"></span>';
  const row = `<div class="agtr${r.path === F.picked ? " on" : ""}${r.ignored ? " ign" : ""}" role="treeitem" style="--d:${r.depth}" draggable="true" data-fpath="${esc(r.path)}"${r.folder ? ` data-ffolder="1" aria-expanded="${r.open}"` : ""} title="${esc(r.path)}">`
    + `${twist}${r.folder ? FOLDER : FILE}<span class="nm">${esc(r.name)}</span>${mark}</div>`;
  const pad = `style="--d:${r.depth + 1}"`;
  const extra = r.error ? `<div class="agtr note derr" ${pad}>${esc(r.error)}</div>`
    : r.more ? `<div class="agtr note" ${pad}>${r.more} more not shown</div>` : "";
  return row + extra;
}

function draw() {
  const tree = F.el?.querySelector<HTMLElement>(".agtree");
  if (!tree) return;
  const top = F.listings.get("");
  const rows = treeRows(F.listings, F.open, F.filter);
  let html = rows.map(rowHtml).join("");
  if (top?.error) html = `<div class="agpmsg derr">${esc(top.error)}</div>`;
  else if (!top?.entries) html = `<div class="agpmsg"><span class="dspin"></span>Reading the folder…</div>`;
  else if (!rows.length) html = `<div class="agpmsg">${F.filter ? "Nothing open here matches." : "This folder is empty."}</div>`;
  else if (top.more) html += `<div class="agtr note" style="--d:0">${top.more} more not shown</div>`;
  const keep = tree.scrollTop;
  tree.innerHTML = html;
  tree.scrollTop = keep;
}

function toggle(path: string) {
  if (F.open.has(path)) F.open.delete(path);
  else {
    F.open.add(path);
    if (!F.listings.get(path)?.entries) load(path);
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
  find.addEventListener("input", () => { F.filter = find.value; draw(); });
  find.addEventListener("keydown", e => { if (e.key === "Escape" && find.value) { e.stopPropagation(); e.preventDefault(); find.value = ""; F.filter = ""; draw(); } });
  el.querySelector<HTMLInputElement>(".agfign input")!.addEventListener("change", e => {
    F.ignored = (e.target as HTMLInputElement).checked;
    for (const dir of F.listings.keys()) load(dir);
  });
  el.querySelector("[data-frefresh]")!.addEventListener("click", () => { for (const dir of F.listings.keys()) load(dir); });
  tree.addEventListener("click", e => {
    closeFilesMenu();
    const row = (e.target as HTMLElement).closest<HTMLElement>("[data-fpath]");
    if (!row) return;
    const path = row.dataset.fpath!;
    F.picked = path;
    if (row.dataset.ffolder) toggle(path);
    else { draw(); openWorkFile(path); }
  });
  tree.addEventListener("contextmenu", e => {
    const row = (e.target as HTMLElement).closest<HTMLElement>("[data-fpath]");
    if (!row) return;
    e.preventDefault();
    F.picked = row.dataset.fpath!;
    draw();
    menu(F.picked, !!row.dataset.ffolder, e.clientX, e.clientY);
  });
  tree.addEventListener("dragstart", e => {
    const row = (e.target as HTMLElement).closest<HTMLElement>("[data-fpath]");
    if (!row || !e.dataTransfer) return;
    e.dataTransfer.setData(PATH_TYPE, row.dataset.fpath!);
    e.dataTransfer.setData("text/plain", row.dataset.fpath!);
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
    askUpload([...e.dataTransfer.files], dir);
  });
}

/// The right-click menu of `path`.
function menu(path: string, folder: boolean, x: number, y: number) {
  closeFilesMenu();
  const s = paneSession();
  if (!F.el || !s) return;
  const items: [string, string][] = [
    ...(folder ? [] : [["open", "Open in viewer"]] as [string, string][]),
    ["mention", "Mention in message"],
    ["-", ""],
    ["abs", "Copy path"],
    ["rel", "Copy relative path"],
    ...(folder ? [] : [["-", ""], ["save", "Save to this Mac"]] as [string, string][]),
  ];
  const m = h(`<div class="agctx" role="menu">${items.map(([k, t]) => k === "-" ? "<hr>" : `<button role="menuitem" data-fact="${k}">${esc(t)}</button>`).join("")}</div>`);
  F.el.append(m);
  const box = F.el.getBoundingClientRect();
  m.style.left = `${Math.max(0, Math.min(x - box.left, box.width - m.offsetWidth - 4))}px`;
  m.style.top = `${Math.max(0, Math.min(y - box.top, box.height - m.offsetHeight - 4))}px`;
  m.querySelector<HTMLElement>("button")?.focus();
  const note = (text: string) => {
    const meta = F.el?.querySelector(".agpacts .meta");
    if (meta) { meta.textContent = text; setTimeout(() => { if (meta.isConnected) meta.textContent = s.cwd; }, 3000); }
  };
  m.addEventListener("click", e => {
    const act = (e.target as HTMLElement).closest<HTMLElement>("[data-fact]")?.dataset.fact;
    if (!act) return;
    e.stopPropagation();
    closeFilesMenu();
    if (act === "open") openWorkFile(path);
    else if (act === "mention") addMention(path);
    else if (act === "abs" || act === "rel") {
      const text = act === "abs" ? resolvePath(s.cwd, path) : path;
      navigator.clipboard.writeText(text).then(() => note(`Copied ${text}`), err => note(`Could not copy: ${err}`));
    } else if (act === "save") saveWorkFile(path, note);
  });
  m.addEventListener("keydown", e => {
    const all = [...m.querySelectorAll<HTMLElement>("button")];
    const i = all.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      all[(i + (e.key === "ArrowDown" ? 1 : all.length - 1)) % all.length]?.focus();
    }
  });
  setTimeout(() => document.addEventListener("pointerdown", function away(ev) {
    if (!m.contains(ev.target as Node)) { m.remove(); document.removeEventListener("pointerdown", away); }
  }), 0);
}
