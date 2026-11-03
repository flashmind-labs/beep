// The Artifacts tab of a coding agent session's pane: the pages and
// documents the agent published or wrote, newest first, and the picked one
// rendered. An HTML page shows in a sandboxed frame served from its own
// scheme, Markdown as the bar's cleaned HTML, SVG and pictures as images.
// The file comes from the session's machine through the Files tab's read
// path; the claude.ai link only opens in the browser.

import { type Artifact, artifactVersion, frameHtml, KIND_NAMES, readPlace, relativeTo } from "../artifacts-state.ts";
import { clock } from "../agentturns.ts";
import { esc, h, invoke } from "./core.ts";
import { revealPath } from "./filespane.ts";
import { fileSize } from "./media.ts";
import { paneSession, readError } from "./mediapane.ts";

/// An artifact made ready to show, as `agent_artifact` gives it.
type Shown =
  | { kind: "html"; size: number; src: string }
  | { kind: "md"; size: number; html: string }
  | { kind: "svg" | "image"; size: number; mime: string; data: string };

const A = {
  /// The session's machine and folder the list is of.
  key: "",
  list: [] as Artifact[],
  /// The list as last drawn, to draw it again only when it changed.
  drawn: "",
  picked: null as string | null,
  /// The version of the picked one on screen, or being read.
  version: "",
  /// When this window first saw each version, for ones written while it looked.
  seen: new Map<string, number>(),
  /// The list came at least once, so versions after it were seen live.
  primed: false,
  /// When each turn started, by its message's place, as the log knows.
  turnStart: (_turn: number): number | undefined => undefined,
  el: null as HTMLElement | null,
};

/// The sessions whose preview the user folded, by `A.key`, kept while the
/// window is open so going back to a session finds it as it was left.
const folded = new Set<string>();

/// Starts again when the pane shows another session than the one the
/// tab is of.
function syncSession() {
  const s = paneSession();
  const key = s ? `${s.machine_id}\n${s.cwd}` : "";
  if (A.key === key) return;
  A.el?.remove();
  A.el = null;
  A.key = key;
  A.list = [];
  A.drawn = "";
  A.picked = null;
  A.version = "";
  A.seen = new Map();
  A.primed = false;
}

/// The Artifacts view of the pane's session, made once per session.
export function artifactsView() {
  syncSession();
  if (A.el) return A.el;
  const el = h(`<div class="agpview arts"><div class="agartlist" role="listbox" aria-label="Artifacts"></div><div class="agartshow"></div>`
    + `<div class="agpacts"><button class="dbtn" data-aact="fold" title="Fold or show the preview"></button><span class="meta mono"></span><span class="sp"></span>`
    + `<button class="dbtn" data-aact="claude" hidden>Open on claude.ai</button><button class="dbtn" data-aact="file">Open file</button><button class="dbtn" data-aact="copy">Copy path</button></div></div>`);
  A.el = el;
  el.querySelector(".agartlist")!.addEventListener("click", e => {
    const row = (e.target as HTMLElement).closest<HTMLElement>("[data-apath]");
    if (row) pick(row.dataset.apath!);
  });
  el.querySelector(".agpacts")!.addEventListener("click", e => {
    const act = (e.target as HTMLElement).closest<HTMLElement>("[data-aact]")?.dataset.aact;
    if (act) action(act);
  });
  drawFold();
  drawList();
  const a = picked();
  if (a) load(a);
  return el;
}

/// Shows the preview, or folds it away so the list has the room.
function drawFold() {
  const el = A.el;
  if (!el) return;
  const fold = folded.has(A.key);
  el.classList.toggle("folded", fold);
  el.querySelector<HTMLElement>(".agartshow")!.hidden = fold;
  const btn = el.querySelector<HTMLButtonElement>('[data-aact="fold"]')!;
  btn.textContent = fold ? "Show" : "Fold";
  btn.setAttribute("aria-expanded", String(!fold));
}

/// Takes the artifacts the log has now; `turnStart` says when a turn began.
/// The picked one is read again when the agent wrote it again.
export function setArtifacts(list: Artifact[], turnStart: (turn: number) => number | undefined) {
  syncSession();
  A.turnStart = turnStart;
  const now = Date.now();
  for (const a of list) {
    const v = `${a.path}\n${artifactVersion(a)}`;
    if (A.primed && !A.seen.has(v)) A.seen.set(v, now);
  }
  A.primed = true;
  A.list = list;
  if (!A.picked || !list.some(a => a.path === A.picked)) A.picked = list[0]?.path ?? null;
  if (!A.el) return;
  drawList();
  const a = picked();
  if (a && artifactVersion(a) !== A.version) load(a);
  else if (!a) drawShown(null);
}

/// How many artifacts the tab lists.
export function artifactCount() {
  return A.list.length;
}

/// Picks the artifact at `path`, when the tab lists it; false when not.
export function pickArtifact(path: string) {
  if (!A.list.some(a => a.path === path)) return false;
  pick(path);
  return true;
}

function picked() {
  return A.list.find(a => a.path === A.picked) ?? null;
}

function pick(path: string) {
  A.picked = path;
  drawList();
  const a = picked();
  if (a) load(a);
}

/// When `a`'s newest version came: when this window saw it written, else
/// when its turn began, if known.
function when(a: Artifact) {
  return A.seen.get(`${a.path}\n${artifactVersion(a)}`) ?? A.turnStart(a.turn);
}

function rowHtml(a: Artifact, cwd: string) {
  const rel = relativeTo(cwd, a.path) ?? a.path;
  const at = when(a);
  const turn = a.turnNo ? `Turn ${a.turnNo}` : "";
  const meta = [turn, at != null ? clock(at) : ""].filter(Boolean).join(" · ");
  const more = a.versions > 1 ? `<small>${a.versions} versions</small>` : "";
  return `<button class="agart${a.path === A.picked ? " on" : ""}" role="option" aria-selected="${a.path === A.picked}" data-apath="${esc(a.path)}" title="${esc(a.path)}">`
    + `<span class="k ${a.kind}">${KIND_NAMES[a.kind]}</span><span class="t"><b>${esc(a.name)}</b><small>${esc(rel)}</small></span>`
    + `<span class="r">${a.url ? '<span class="pub">claude.ai</span>' : ""}<span>${esc(meta)}</span>${more}</span></button>`;
}

function drawList() {
  const box = A.el?.querySelector<HTMLElement>(".agartlist");
  const s = paneSession();
  if (!box || !s) return;
  const html = A.list.length ? A.list.map(a => rowHtml(a, s.cwd)).join("")
    : '<div class="agpmsg">Pages the agent publishes, or writes outside its folder, show here. Repo files it edits are in Files.</div>';
  if (html !== A.drawn) { box.innerHTML = html; A.drawn = html; }
  const a = picked();
  const acts = A.el!.querySelector<HTMLElement>(".agpacts")!;
  acts.hidden = !a;
  if (!a) return;
  const inside = relativeTo(s.cwd, a.path) != null;
  const claude = acts.querySelector<HTMLButtonElement>('[data-aact="claude"]')!;
  claude.hidden = !a.url;
  claude.title = a.url ?? "";
  const file = acts.querySelector<HTMLButtonElement>('[data-aact="file"]')!;
  file.disabled = !inside;
  file.title = inside ? "Show it in the Files tab" : "It is outside the session's folder";
}

function note(text: string) {
  const meta = A.el?.querySelector(".agpacts .meta");
  if (meta) meta.textContent = text;
}

function about(a: Artifact, size: number | null) {
  const s = paneSession();
  const rel = s ? relativeTo(s.cwd, a.path) ?? a.path : a.path;
  return [rel, KIND_NAMES[a.kind], size != null ? fileSize(size) : ""].filter(Boolean).join(" · ");
}


/// Reads `a` from the machine and shows it, unless a newer read started.
async function load(a: Artifact) {
  const s = paneSession();
  if (!s) return;
  const version = artifactVersion(a), key = A.key;
  A.version = version;
  const keep = A.el?.querySelector(".agartshow > :not(.agpmsg)");
  if (!keep) drawShown(`<div class="agpmsg"><span class="dspin"></span>Reading ${esc(a.name)} from ${esc(s.machine_name)}…</div>`);
  note(about(a, null));
  const place = readPlace(s.cwd, a.path);
  try {
    const got = await invoke<Shown>("agent_artifact", { machineId: s.machine_id, root: place.root, path: place.path });
    if (A.key !== key || A.picked !== a.path || A.version !== version) return;
    drawShown(shownHtml(a, got));
    note(about(a, got.size));
  } catch (e) {
    if (A.key !== key || A.picked !== a.path || A.version !== version) return;
    drawShown(`<div class="agpmsg derr">${esc(readError(e))}</div>`);
  }
}

function shownHtml(a: Artifact, got: Shown) {
  if (got.kind === "html") return `<div class="agartframe-box">${frameHtml(got.src, a.name)}</div>`;
  if (got.kind === "md") return `<div class="agartmd">${got.html}</div>`;
  return `<div class="agartimg"><img alt="${esc(a.name)}" src="data:${esc(got.mime)};base64,${got.data}"></div>`;
}

function drawShown(html: string | null) {
  const box = A.el?.querySelector<HTMLElement>(".agartshow");
  if (!box) return;
  box.innerHTML = html ?? "";
}

function action(act: string) {
  if (act === "fold") {
    if (!folded.delete(A.key)) folded.add(A.key);
    drawFold();
    return;
  }
  const a = picked(), s = paneSession();
  if (!a || !s) return;
  if (act === "claude" && a.url) invoke("open_url", { url: a.url }).catch(e => note(readError(e)));
  else if (act === "file") {
    const rel = relativeTo(s.cwd, a.path);
    if (rel != null) revealPath(rel);
  } else if (act === "copy") {
    navigator.clipboard.writeText(a.path).then(() => note(`Copied ${a.path}`), err => note(`Could not copy: ${err}`));
  }
}
