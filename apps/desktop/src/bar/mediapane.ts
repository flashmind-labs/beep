// The media pane of a coding agent session: pictures, videos, 3D models
// and PDFs the agent made or looked at, and its subagents, each a tab.
// It docks to the right of the conversation or under it, resizes by its
// divider and folds to a strip; where it sits is kept per window.

import {
  actualView, baseName, type Dock, effectiveDock, fitView, type ImageView, type MediaKind, mediaKind,
  type PaneLayout, paneSize, readLayout, resolvePath, zoomAt,
} from "../media-state.ts";
import { highlight, languageOf, MAX_LINES, shortSize, terminalName } from "../files-state.ts";
import type { ModelView } from "../model3d.ts";
import { changeListeners, esc, h, ICON, invoke, PANE, S } from "./core.ts";
import { closeFilesMenu, FILES_ICON, filesView } from "./filespane.ts";
import { fileSize, openViewer } from "./media.ts";
import { attach, dropTerm, forgetTerm, killTerm, openTerm, type PaneTerm, termStatus } from "./paneterm.ts";
import { addMention } from "./workdrop.ts";

/// What the node gives of a file, a part at a time (`agent_file`).
interface FilePart {
  size: number;
  mime: string;
  /// Base64.
  data: string;
}

/// The start of a file of the Files tab, as `agent_tree_text` reads it.
interface TreeText {
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
interface MediaTab {
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
  /// A text tab's path in the session's folder, and what was read of it.
  rel?: string;
  text?: TreeText;
  /// A terminal tab's terminal, once it started.
  term?: PaneTerm;
}

/// The session the pane belongs to.
interface PaneSession {
  machine_id: string;
  machine_name: string;
  cwd: string;
}

const SUBS = "subagents";
const FILES = "files";
const KEY = `beep.mediaPane.${PANE ? "window" : "bar"}`;
const ARROW_STEP = 24;

const P = {
  session: null as PaneSession | null,
  tabs: [] as MediaTab[],
  /// The tab on screen: a tab's id, `FILES` or `SUBS`.
  on: FILES,
  /// The subagents' rows and how many, as the log last drew them.
  subs: "",
  subCount: 0,
  layout: loadLayout(),
  watch: null as ResizeObserver | null,
};

function loadLayout(): PaneLayout {
  try { return readLayout(localStorage.getItem(KEY)); } catch { return readLayout(null); }
}

function saveLayout() {
  try { localStorage.setItem(KEY, JSON.stringify(P.layout)); } catch { /* kept while the window is open */ }
}

const ICONS: Record<TabKind | "subagents" | "files", string> = {
  files: FILES_ICON,
  text: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 7l-5 5 5 5M16 7l5 5-5 5"/></svg>',
  term: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>',
  image: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 17l-5-5-8 8"/></svg>',
  model: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 2l9 5v10l-9 5-9-5V7z"/><path d="M12 22V12M3 7l9 5 9-5"/></svg>',
  video: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="5" width="13" height="14" rx="2"/><path d="M16 10l5-3v10l-5-3"/></svg>',
  pdf: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/></svg>',
  subagents: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V13a5 5 0 0 0 5 5h4.5"/></svg>',
};
const DOCK_RIGHT = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/></svg>';
const DOCK_BOTTOM = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 14h18"/></svg>';
const FOLD_RIGHT = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>';
const FOLD_DOWN = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>';

// ----- The pane's place -----

const shell = () => document.getElementById("agshell");

/// The pane's frame, drawn with the session's: the divider, the pane and
/// the strip it folds to, after the conversation.
export function paneFrame() {
  return `<div class="agdiv" id="agdiv" role="separator" tabindex="0" title="Drag to resize, double-click to reset" hidden></div>`
    + `<aside class="agpane" id="agpane" hidden><div class="agphead"><div class="agtabs" id="agtabs" role="tablist"></div>`
    + `<button class="agpbtn agaddterm" id="agterm" title="New terminal in this folder (⌃\`)">+ Terminal</button><button class="agpbtn" id="agdock"></button><button class="agpbtn" id="agfold" title="Hide the pane"></button></div><div class="agpviews" id="agpviews"></div></aside>`
    + `<button class="agpstrip" id="agpstrip" hidden title="Show the pane"></button>`;
}

/// Starts the pane for session `s`, closing what the last one had open.
export function startPane(s: PaneSession) {
  closeAll();
  P.session = s;
  P.on = FILES;
  P.subs = "";
  P.subCount = 0;
  const el = shell();
  if (!el) return;
  P.watch?.disconnect();
  P.watch = new ResizeObserver(() => place());
  P.watch.observe(el);
  wireDivider();
  document.getElementById("agpane")!.addEventListener("click", paneClick);
  document.getElementById("agpstrip")!.addEventListener("click", () => { P.layout.hidden = false; saveLayout(); draw(); });
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
changeListeners.push(() => { if (S.view !== "agent") { closeAll(); P.watch?.disconnect(); P.watch = null; } });

/// Lays the pane out for the shell's size: its dock, its size, folded or not.
function place() {
  const el = shell(), pane = document.getElementById("agpane"), div = document.getElementById("agdiv"), strip = document.getElementById("agpstrip");
  if (!el || !pane || !div || !strip) return;
  // The Files tab is always there while a session shows.
  const has = !!P.session;
  const dock = effectiveDock(P.layout.dock, el.clientWidth);
  el.classList.toggle("dock-bottom", dock === "bottom");
  el.classList.toggle("narrow", dock !== P.layout.dock);
  pane.hidden = div.hidden = !has || P.layout.hidden;
  strip.hidden = !has || !P.layout.hidden;
  if (pane.hidden) return;
  const total = dock === "right" ? el.clientWidth : el.clientHeight;
  pane.style.flexBasis = `${paneSize(dock, P.layout[dock], total)}px`;
  div.setAttribute("aria-orientation", dock === "right" ? "vertical" : "horizontal");
}

function wireDivider() {
  const div = document.getElementById("agdiv")!;
  const dockNow = (): Dock => shell()?.classList.contains("dock-bottom") ? "bottom" : "right";
  const set = (dock: Dock, size: number | null) => {
    const el = shell();
    if (!el) return;
    P.layout[dock] = size == null ? null : paneSize(dock, size, dock === "right" ? el.clientWidth : el.clientHeight);
    place();
  };
  div.addEventListener("pointerdown", e => {
    if (e.button !== 0) return;
    e.preventDefault();
    div.setPointerCapture(e.pointerId);
    const el = shell()!, dock = dockNow();
    el.classList.add("resizing");
    const move = (ev: PointerEvent) => {
      const r = el.getBoundingClientRect();
      set(dock, dock === "right" ? r.right - ev.clientX : r.bottom - ev.clientY);
    };
    const up = () => {
      div.removeEventListener("pointermove", move);
      div.removeEventListener("pointerup", up);
      div.removeEventListener("pointercancel", up);
      el.classList.remove("resizing");
      saveLayout();
    };
    div.addEventListener("pointermove", move);
    div.addEventListener("pointerup", up);
    div.addEventListener("pointercancel", up);
  });
  div.addEventListener("dblclick", () => { set(dockNow(), null); saveLayout(); });
  div.addEventListener("keydown", e => {
    const dock = dockNow(), pane = document.getElementById("agpane")!;
    const now = dock === "right" ? pane.offsetWidth : pane.offsetHeight;
    const grow: Record<string, number> = dock === "right" ? { ArrowLeft: 1, ArrowRight: -1 } : { ArrowUp: 1, ArrowDown: -1 };
    const by = grow[e.key];
    if (!by) return;
    e.preventDefault();
    e.stopPropagation();
    set(dock, now + by * ARROW_STEP);
    saveLayout();
  });
}

// ----- Tabs -----

/// Draws the pane again: its tabs and the one on screen. `subs` are the
/// subagents' rows and `count` how many, as the log has them now.
export function drawPane(subs?: { html: string; count: number }) {
  if (subs) { P.subs = subs.html; P.subCount = subs.count; }
  draw();
}

function draw() {
  const tabs = document.getElementById("agtabs"), views = document.getElementById("agpviews");
  if (!tabs || !views) return;
  // The tab on screen went: the newest file, else the files.
  if (P.on === SUBS ? !P.subCount : P.on !== FILES && !P.tabs.some(t => t.id === P.on)) P.on = P.tabs.at(-1)?.id ?? FILES;
  const tab = (id: string, kind: TabKind | "subagents" | "files", name: string, extra: string) =>
    `<button class="agtab${P.on === id ? " on" : ""}" role="tab" aria-selected="${P.on === id}" data-mtab="${esc(id)}" title="${esc(name)}">${ICONS[kind]}<span>${esc(name)}</span>${extra}</button>`;
  tabs.innerHTML = tab(FILES, FILES, "Files", "")
    + (P.subCount ? tab(SUBS, SUBS, "Subagents", `<small>${P.subCount}</small>`) : "")
    + P.tabs.map(t => tab(t.id, t.kind, t.name, `<em data-mclose="${esc(t.id)}" aria-label="Close ${esc(t.name)}" title="Close">${ICON.x}</em>`)).join("");
  const box = shell();
  const bottom = (box ? effectiveDock(P.layout.dock, box.clientWidth) : P.layout.dock) === "bottom";
  const dock = document.getElementById("agdock")!;
  dock.innerHTML = bottom ? DOCK_RIGHT : DOCK_BOTTOM;
  dock.title = bottom ? "Dock on the right" : "Dock at the bottom";
  document.getElementById("agfold")!.innerHTML = bottom ? FOLD_DOWN : FOLD_RIGHT;
  const strip = document.getElementById("agpstrip")!;
  const count = P.tabs.length + (P.subCount ? 1 : 0) + 1;
  const shown = P.tabs.find(t => t.id === P.on);
  const label = shown ? shown.name : P.on === SUBS ? "Subagents" : "Files";
  strip.innerHTML = `${ICONS[shown?.kind ?? (P.on === SUBS ? SUBS : FILES)]}<span>${esc(label)}</span><small>${count}</small>`;
  let filesEl = views.querySelector<HTMLElement>(":scope > .agpview.files");
  const made = filesView();
  if (filesEl !== made) { filesEl?.remove(); views.prepend(filesEl = made); }
  filesEl.hidden = P.on !== FILES;
  if (P.on !== FILES) closeFilesMenu();
  // Subagents redraw each time; a file's view is drawn once and kept.
  let subView = views.querySelector<HTMLElement>(":scope > .agpview.subs");
  if (P.subCount) {
    if (!subView) views.append(subView = h('<div class="agpview subs"></div>'));
    if (subView.innerHTML !== P.subs) subView.innerHTML = P.subs;
    subView.hidden = P.on !== SUBS;
  } else subView?.remove();
  for (const t of P.tabs) {
    if (!t.el) views.append(t.el = viewOf(t));
    const hide = t.id !== P.on;
    if (hide && !t.el.hidden) t.el.querySelector("video")?.pause();
    t.el.hidden = hide;
    if (!hide) t.model?.resize();
    if (!hide && t.term) requestAnimationFrame(() => { t.term?.screen.fit(); t.term?.screen.focus(); });
  }
  place();
}

function paneClick(e: MouseEvent) {
  const target = e.target as HTMLElement;
  const close = target.closest<HTMLElement>("[data-mclose]");
  if (close) { e.stopPropagation(); return closeTab(close.dataset.mclose!); }
  const tab = target.closest<HTMLElement>("[data-mtab]");
  if (tab) { P.on = tab.dataset.mtab!; return draw(); }
  if (target.closest("#agdock")) {
    P.layout.dock = P.layout.dock === "right" ? "bottom" : "right";
    saveLayout();
    return draw();
  }
  if (target.closest("#agfold")) { P.layout.hidden = true; saveLayout(); return draw(); }
  if (target.closest("#agterm")) return newTerminal();
}

function closeTab(id: string) {
  const i = P.tabs.findIndex(t => t.id === id);
  if (i < 0) return;
  dropTab(P.tabs[i]);
  P.tabs.splice(i, 1);
  if (P.on === id) P.on = P.tabs[Math.min(i, P.tabs.length - 1)]?.id ?? SUBS;
  draw();
}

/// Shows the subagents' tab, unfolding the pane.
export function showSubagents() {
  P.on = SUBS;
  P.layout.hidden = false;
  draw();
}

/// Opens a picture the log already shows (a data URL) in its own tab.
export function openPicture(url: string, name: string) {
  open({ id: url, kind: "image", name: baseName(name), path: null, url });
}

/// Opens the file at `path` (relative to the session's folder, absolute,
/// or under `~/`) on the session's machine, when it is one the pane shows.
/// False for a file it does not.
export function openPath(path: string) {
  const kind = mediaKind(path);
  if (!kind || !P.session) return false;
  const full = resolvePath(P.session.cwd, path);
  open({ id: full, kind, name: baseName(full), path: full, url: null });
  return true;
}

function open(t: Pick<MediaTab, "id" | "kind" | "name" | "path" | "url"> & Partial<MediaTab>) {
  P.layout.hidden = false;
  saveLayout();
  if (!P.tabs.some(x => x.id === t.id)) {
    const tab: MediaTab = { ...t, size: 0, done: 0, error: "", el: null };
    P.tabs.push(tab);
    if (tab.kind === "text") loadText(tab);
    else if (tab.kind === "term") startTerminal(tab);
    else if (!tab.url) load(tab);
  }
  P.on = t.id;
  draw();
}

/// The session the pane shows, while one does.
export function paneSession(): Readonly<PaneSession> | null {
  return P.session;
}

/// Opens `rel`, a file in the session's folder, from the Files tab: a
/// picture, video, 3D model or PDF in its viewer, anything else as text.
export function openWorkFile(rel: string) {
  if (openPath(rel)) return;
  open({ id: `text:${rel}`, kind: "text", name: baseName(rel), path: null, url: null, rel });
}

/// Saves `rel`, a file in the session's folder, to Downloads; `note`
/// hears how it went.
export function saveWorkFile(rel: string, note: (text: string) => void) {
  const s = P.session;
  if (!s) return;
  note(`Saving ${baseName(rel)}…`);
  invoke<string>("agent_tree_save", { machineId: s.machine_id, root: s.cwd, path: rel })
    .then(to => { note(`Saved to Downloads as ${baseName(to)}`); invoke("reveal_file", { path: to }).catch(() => {}); })
    .catch(e => note(readError(e)));
}

/// Opens a terminal tab on the session's machine, in its folder (⌃`).
export function newTerminal() {
  const s = P.session;
  if (!s) return;
  const taken = P.tabs.filter(t => t.kind === "term").map(t => t.name);
  const name = terminalName(s.cwd, taken);
  open({ id: `term:${name}`, kind: "term", name, path: null, url: null });
}

// ----- Reading files from the machine -----

/// The most the pane reads of one file, as the node allows.
export const MAX_FILE = 128 * 1024 * 1024;

function bytesOf(b64: string) {
  const s = atob(b64), out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

/// Reads all of `path` from `machine`, a part at a time; `progress` hears
/// how far it got.
async function readFile(machine: string, path: string, progress: (done: number, size: number) => void) {
  const parts: Uint8Array[] = [];
  let done = 0, size = 0, mime = "";
  do {
    const part = await invoke<FilePart>("agent_file", { machineId: machine, path, offset: done });
    size = part.size;
    mime = part.mime;
    if (size > MAX_FILE) throw new Error(`It is ${fileSize(size)}; the pane opens files up to ${fileSize(MAX_FILE)}.`);
    const bytes = bytesOf(part.data);
    if (!bytes.length) break;
    parts.push(bytes);
    done += bytes.length;
    progress(done, size);
  } while (done < size);
  const all = new Uint8Array(done);
  let at = 0;
  for (const p of parts) { all.set(p, at); at += p.length; }
  return { bytes: all, mime };
}

/// What an error from reading a file says to the user.
function readError(e: unknown) {
  const text = e instanceof Error ? e.message : String(e);
  if (/no longer|not found|gone/i.test(text)) return "This file is not on the machine any more, or the machine is offline.";
  if (/unknown op|unknown request|not serve|too old/i.test(text)) return "Beep cannot read this file yet: update Beep and beep-node on that machine.";
  return text;
}

/// Reads the start of a text tab's file.
async function loadText(t: MediaTab) {
  const s = P.session;
  if (!s || !t.rel) return;
  try {
    t.text = await invoke<TreeText>("agent_tree_text", { machineId: s.machine_id, root: s.cwd, path: t.rel });
    t.size = t.text.size;
  } catch (e) {
    t.error = readError(e);
  }
  if (!P.tabs.includes(t)) return;
  t.el?.remove();
  t.el = null;
  draw();
}

/// Starts a terminal tab's shell; when it exits the tab goes.
async function startTerminal(t: MediaTab) {
  const s = P.session;
  if (!s) return;
  try {
    const term = await openTerm({ machine_id: s.machine_id, cwd: s.cwd, name: t.name }, {
      onEnd: () => {
        if (t.term) forgetTerm(t.term);
        t.term = undefined;
        closeTab(t.id);
      },
      onLink: () => drawTermStatus(t),
    });
    if (!P.tabs.includes(t)) return dropTerm(term);
    t.term = term;
  } catch (e) {
    if (!P.tabs.includes(t)) return;
    t.error = readError(e);
  }
  t.el?.remove();
  t.el = null;
  draw();
}

function drawTermStatus(t: MediaTab) {
  const meta = t.el?.querySelector(".agpacts .meta");
  if (meta && t.term) meta.textContent = termStatus(t.term, P.session?.machine_name ?? "the machine");
  const again = t.el?.querySelector<HTMLElement>("[data-tattach]");
  if (again && t.term) again.hidden = t.term.link !== "detached";
}

async function load(t: MediaTab) {
  const s = P.session;
  if (!s || !t.path) return;
  try {
    const { bytes, mime } = await readFile(s.machine_id, t.path, (done, size) => {
      t.done = done;
      t.size = size;
      const bar = t.el?.querySelector<HTMLElement>(".agpload i");
      if (bar) bar.style.width = `${size ? Math.round((done / size) * 100) : 0}%`;
    });
    if (!P.tabs.includes(t)) return;
    t.size = bytes.length;
    t.bytes = bytes.buffer as ArrayBuffer;
    t.url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime || "application/octet-stream" }));
  } catch (e) {
    if (!P.tabs.includes(t)) return;
    t.error = readError(e);
  }
  // Drawn again now its bytes are here.
  t.el?.remove();
  t.el = null;
  draw();
}

// ----- Views -----

function viewOf(t: MediaTab): HTMLElement {
  if (t.error) return h(`<div class="agpview"><div class="agpmsg derr">${esc(t.error)}</div></div>`);
  if (t.kind === "text") return t.text ? textView(t, t.text) : waiting(t);
  if (t.kind === "term") return t.term ? termView(t, t.term) : h(`<div class="agpview"><div class="agpmsg"><span class="dspin"></span>Opening a terminal on ${esc(P.session?.machine_name ?? "the machine")}…</div></div>`);
  if (!t.url) {
    const where = P.session?.machine_name ?? "the machine";
    return h(`<div class="agpview"><div class="agpmsg"><span class="dspin"></span>Reading ${esc(t.name)} from ${esc(where)}…<span class="agpload"><i></i></span></div></div>`);
  }
  if (t.kind === "image") return imageView(t);
  if (t.kind === "video") return videoView(t);
  if (t.kind === "pdf") return pdfView(t);
  return modelView(t);
}

function waiting(t: MediaTab) {
  return h(`<div class="agpview"><div class="agpmsg"><span class="dspin"></span>Reading ${esc(t.name)} from ${esc(P.session?.machine_name ?? "the machine")}…</div></div>`);
}

/// A file of the Files tab as text: numbered lines, lightly highlighted,
/// read only. Only its start shows when it is large.
function textView(t: MediaTab, text: TreeText) {
  const rel = t.rel!;
  const lang = languageOf(rel);
  let body: string;
  let lines = 0;
  if (text.binary) {
    body = `<div class="agpmsg">${esc(t.name)} is not text (${esc(shortSize(text.size))}). Save it to open it on this Mac.</div>`;
  } else {
    const all = text.text.split("\n");
    if (all.length > 1 && all.at(-1) === "") all.pop();
    lines = all.length;
    const shown = all.slice(0, MAX_LINES);
    const width = String(shown.length).length;
    body = `<div class="agcode" style="--lnw:${width}ch"><pre>${shown.map((l, i) => `<span class="ln">${i + 1}</span>${highlight(l, lang)}`).join("\n")}</pre></div>`;
  }
  const cut = text.cut || lines > MAX_LINES ? ` · the first ${lines > MAX_LINES ? `${MAX_LINES.toLocaleString()} lines` : shortSize(text.text.length)} shown` : "";
  const about = [rel, lang, text.binary ? "" : `${lines.toLocaleString()} lines`, shortSize(text.size)].filter(Boolean).join(" · ") + cut;
  const el = h(`<div class="agpview">${body}<div class="agpacts"><span class="meta mono" title="${esc(rel)}">${esc(about)}</span><span class="sp"></span><button class="dbtn" data-tment="1">Mention</button><button class="dbtn" data-tsave="1">Save</button></div></div>`);
  const meta = el.querySelector(".meta")!;
  el.querySelector("[data-tment]")!.addEventListener("click", () => addMention(rel));
  el.querySelector("[data-tsave]")!.addEventListener("click", () => saveWorkFile(rel, note => { meta.textContent = note; }));
  return el;
}

/// A terminal tab: its live screen, and a footer to attach again or stop it.
function termView(t: MediaTab, term: PaneTerm) {
  const el = h(`<div class="agpview term"><div class="agpacts"><span class="meta mono"></span><span class="sp"></span><button class="dbtn" data-tattach="1" hidden>Attach</button><button class="dbtn warn" data-tkill="1">Close</button></div></div>`);
  el.prepend(term.screen.el);
  el.querySelector("[data-tattach]")!.addEventListener("click", () => { term.tries = 0; attach(term); });
  el.querySelector("[data-tkill]")!.addEventListener("click", () => {
    if (term.row.state === "running" && term.link !== "ended" && !confirm(`Stop ${t.name} on ${P.session?.machine_name ?? "the machine"}?`)) return;
    killTerm(term).then(() => { t.term = undefined; closeTab(t.id); }).catch(e => { el.querySelector(".meta")!.textContent = readError(e); });
  });
  queueMicrotask(() => drawTermStatus(t));
  return el;
}

/// The buttons that act on a file of the machine: open it in the Mac's
/// own app, save it to Downloads.
function fileButtons(t: MediaTab, openLabel: string) {
  if (!t.path) return "";
  return `<button class="dbtn" data-mopen="1">${esc(openLabel)}</button><button class="dbtn" data-msave="1">Save</button>`;
}

function wireFileButtons(t: MediaTab, el: HTMLElement) {
  const s = P.session;
  if (!s || !t.path) return;
  const note = (text: string) => { const m = el.querySelector(".agpacts .meta"); if (m) m.textContent = text; };
  el.querySelector("[data-mopen]")?.addEventListener("click", () => {
    invoke("agent_file_open", { machineId: s.machine_id, path: t.path }).catch(e => note(readError(e)));
  });
  el.querySelector("[data-msave]")?.addEventListener("click", () => {
    invoke<string>("agent_file_save", { machineId: s.machine_id, path: t.path })
      .then(to => { note(`Saved to Downloads as ${baseName(to)}`); invoke("reveal_file", { path: to }).catch(() => {}); })
      .catch(e => note(readError(e)));
  });
}

/// A picture that fits its box, or shows at its own size; the wheel or a
/// pinch zooms about the pointer, a drag pans.
function imageView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage img"><img alt="${esc(t.name)}" draggable="false"></div>
    <div class="agpacts"><button class="dbtn on" data-fit="1">Fit</button><button class="dbtn" data-fit="0">Actual size</button><span class="zoom"></span><span class="meta"></span><span class="sp"></span><span class="hint">Wheel or pinch to zoom, drag to pan</span>${t.path ? fileButtons(t, "Open in Preview") : '<button class="dbtn" data-full="1" title="Full size, to save or copy">Open full size</button>'}</div></div>`);
  const stage = el.querySelector<HTMLElement>(".agpstage")!, img = el.querySelector("img")!;
  const apply = () => {
    const im = t.image;
    if (!im) return;
    img.style.transform = `translate(${im.view.x}px, ${im.view.y}px) scale(${im.view.scale})`;
    el.querySelector(".zoom")!.textContent = `${Math.round(im.view.scale * 100)}%`;
    el.querySelector('[data-fit="1"]')!.classList.toggle("on", im.view.fit);
    el.querySelector('[data-fit="0"]')!.classList.toggle("on", !im.view.fit && im.view.scale === 1);
  };
  const fit = () => { if (t.image) { t.image.view = fitView(t.image.w, t.image.h, stage.clientWidth, stage.clientHeight); apply(); } };
  img.onload = () => {
    t.image = { view: { scale: 1, x: 0, y: 0, fit: true }, w: img.naturalWidth, h: img.naturalHeight };
    el.querySelector(".meta")!.textContent = `${img.naturalWidth}×${img.naturalHeight}${t.size ? ` · ${fileSize(t.size)}` : ""}`;
    fit();
  };
  img.src = t.url!;
  new ResizeObserver(() => { if (t.image?.view.fit) fit(); }).observe(stage);
  const zoom = (factor: number, cx: number, cy: number) => {
    if (!t.image) return;
    const r = stage.getBoundingClientRect();
    t.image.view = zoomAt(t.image.view, factor, cx - r.left, cy - r.top);
    apply();
  };
  stage.addEventListener("wheel", e => {
    e.preventDefault();
    // A pinch comes as the wheel with Control held, in small steps.
    zoom(Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.002)), e.clientX, e.clientY);
  }, { passive: false });
  // WebKit's own pinch.
  let pinch = 1;
  stage.addEventListener("gesturestart", e => { e.preventDefault(); pinch = 1; });
  stage.addEventListener("gesturechange", e => {
    e.preventDefault();
    const g = e as Event & { scale: number; clientX: number; clientY: number };
    zoom(g.scale / pinch, g.clientX, g.clientY);
    pinch = g.scale;
  });
  stage.addEventListener("pointerdown", e => {
    if (e.button !== 0 || !t.image) return;
    stage.setPointerCapture(e.pointerId);
    stage.classList.add("drag");
    let lx = e.clientX, ly = e.clientY;
    const move = (ev: PointerEvent) => {
      if (!t.image) return;
      t.image.view = { ...t.image.view, x: t.image.view.x + ev.clientX - lx, y: t.image.view.y + ev.clientY - ly, fit: false };
      lx = ev.clientX;
      ly = ev.clientY;
      apply();
    };
    const up = () => {
      stage.classList.remove("drag");
      stage.removeEventListener("pointermove", move);
      stage.removeEventListener("pointerup", up);
    };
    stage.addEventListener("pointermove", move);
    stage.addEventListener("pointerup", up);
  });
  stage.addEventListener("dblclick", () => {
    if (!t.image) return;
    t.image.view.fit ? (t.image.view = actualView(t.image.w, t.image.h, stage.clientWidth, stage.clientHeight), apply()) : fit();
  });
  el.querySelector('[data-fit="1"]')!.addEventListener("click", fit);
  el.querySelector('[data-fit="0"]')!.addEventListener("click", () => {
    if (!t.image) return;
    t.image.view = actualView(t.image.w, t.image.h, stage.clientWidth, stage.clientHeight);
    apply();
  });
  el.querySelector("[data-full]")?.addEventListener("click", () =>
    openViewer([{ url: t.url!, name: t.name, caption: "", saves: t.url!.startsWith("data:") }], 0));
  wireFileButtons(t, el);
  return el;
}

function videoView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage"><video controls playsinline preload="metadata"></video></div>
    <div class="agpacts"><span class="meta">${esc(fileSize(t.size))}</span><span class="sp"></span>${fileButtons(t, "Open in QuickTime")}</div></div>`);
  const video = el.querySelector("video")!;
  video.onloadedmetadata = () => {
    const secs = Math.round(video.duration);
    const time = Number.isFinite(secs) ? `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")} · ` : "";
    el.querySelector(".meta")!.textContent = `${time}${video.videoWidth}×${video.videoHeight} · ${fileSize(t.size)}`;
  };
  video.onerror = () => {
    el.querySelector(".agpstage")!.replaceWith(h('<div class="agpmsg">This video does not play here. Open it in QuickTime instead.</div>'));
  };
  video.src = t.url!;
  wireFileButtons(t, el);
  return el;
}

function pdfView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage pdf"><iframe title="${esc(t.name)}"></iframe></div>
    <div class="agpacts"><span class="meta">PDF · ${esc(fileSize(t.size))}</span><span class="sp"></span>${fileButtons(t, "Open in Preview")}</div></div>`);
  el.querySelector("iframe")!.src = t.url!;
  wireFileButtons(t, el);
  return el;
}

function modelView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage model"><div class="agpmsg"><span class="dspin"></span>Drawing ${esc(t.name)}…</div></div>
    <div class="agpacts"><button class="dbtn" data-reset="1" disabled>Reset view</button><button class="dbtn" data-wire="1" disabled>Wireframe</button><span class="meta"></span><span class="sp"></span><span class="hint">Drag to orbit, right-drag to pan, wheel to zoom</span>${fileButtons(t, "Open")}</div></div>`);
  const stage = el.querySelector<HTMLElement>(".agpstage")!;
  const dir = t.path ? t.path.slice(0, t.path.lastIndexOf("/") + 1) : "";
  // A .gltf's buffers and textures are files beside it.
  const sibling = async (uri: string) => {
    if (!P.session || !dir || /^[a-z]+:/i.test(uri)) return null;
    try {
      return (await readFile(P.session.machine_id, resolvePath(dir, decodeURIComponent(uri)), () => {})).bytes;
    } catch {
      return null;
    }
  };
  import("../model3d.ts")
    .then(m => m.mountModel(stage, t.bytes!, t.name.slice(t.name.lastIndexOf(".") + 1).toLowerCase(), sibling))
    .then(view => {
      if (!P.tabs.includes(t)) return view.dispose();
      t.model = view;
      stage.querySelector(".agpmsg")?.remove();
      const i = view.info;
      el.querySelector(".meta")!.textContent = `${i.format} · ${i.triangles.toLocaleString()} triangles${i.meshes > 1 ? ` · ${i.meshes} meshes` : ""} · ${fileSize(t.size)}`;
      el.querySelectorAll<HTMLButtonElement>("[data-reset], [data-wire]").forEach(b => { b.disabled = false; });
      view.resize();
    })
    .catch(e => {
      stage.innerHTML = `<div class="agpmsg derr">${esc(`Could not draw ${t.name}: ${e instanceof Error ? e.message : String(e)}`)}</div>`;
    });
  el.querySelector("[data-reset]")!.addEventListener("click", () => t.model?.reset());
  el.querySelector("[data-wire]")!.addEventListener("click", e => {
    t.wire = !t.wire;
    t.model?.wireframe(t.wire);
    (e.currentTarget as HTMLElement).classList.toggle("on", t.wire);
  });
  wireFileButtons(t, el);
  return el;
}
