// The Artifacts tab of a coding agent session's pane: the pages and documents
// the agent published or wrote, newest first, and the picked one rendered.
// `ArtifactsView` in artifacts-list.tsx draws `artifactsModel`.
import { createStore } from "zustand";
import type { ArtifactAction, ArtifactPreview, ArtifactsModel } from "./artifacts-list.tsx";

import { type Artifact, artifactVersion, goneArtifacts, goneMessage, KIND_NAMES, readPlace, relativeTo } from "./artifacts-state.ts";
import { revealPath } from "./files.ts";
import { fileSize } from "../chat/media.tsx";
import { wantPane, paneSession, refollow } from "./pane-state.ts";
import { writeClipboard } from "../ui/clipboard.ts";
import { invoke } from "../ui/tauri.ts";
import { onShownChange, paneShown } from "../ui/active.ts";

/// An artifact made ready to show, as `agent_artifact` gives it.
type Shown =
  | { kind: "html"; size: number; src: string }
  | { kind: "md"; size: number; html: string }
  | { kind: "image"; size: number; mime: string; data: string };

const A = {
  /// The session's machine and folder the list is of.
  key: "",
  list: [] as Artifact[],
  picked: null as string | null,
  /// The version of the picked one on screen, or being read.
  version: "",
  /// When this window first saw each version, for ones written while it looked.
  seen: new Map<string, number>(),
  /// The list came at least once, so versions after it were seen live.
  primed: false,
  /// The listed ones the machine said it no longer has, and the paths and
  /// time of the last ask, so showing the tab asks again only when the list
  /// changed or a while passed.
  gone: new Set<string>(),
  asked: "",
  askedAt: 0,
  /// When each turn started, by its message's place, as the log knows.
  turnStart: (_turn: number): number | undefined => undefined,
  /// The pane drew the tab for this session.
  made: false,
  note: "",
};

/// The sessions whose preview the user folded, by `A.key`, kept while the
/// window is open so going back to a session finds it as it was left.
const folded = new Set<string>();
let preview: { key: number; value: ArtifactPreview } | null = null;
let previewRevision = 0;
/// Whether the last render let the page frame run.
let drawnLive = false;

/// What the view draws: null until the tab is drawn for the session.
export const artifactsModel = createStore<{ model: ArtifactsModel | null; folded: boolean }>(() => ({ model: null, folded: false }));

/// Starts again when the pane shows another session than the one the
/// tab is of.
function syncSession() {
  const s = paneSession();
  const key = s ? `${s.machine_id}\n${s.cwd}` : "";
  if (A.key === key) return;
  preview = null;
  artifactsModel.setState({ model: null, folded: false });
  Object.assign(A, { made: false, key, list: [], picked: null, version: "", seen: new Map(), primed: false, gone: new Set(), asked: "", askedAt: 0, note: "" });
}

/// Whether the tab shows in its dock, as the pane last drew it.
let tabShown = false;
let coveredPreview: string | null = null;

/// The pane drew the Artifacts tab: `on` when it is the dock's tab on
/// screen, `visible` when it can be seen. Drawn the first time for a
/// session, it reads the picked artifact.
export function artifactsTab(on: boolean, visible: boolean, covered: string | null) {
  syncSession();

  if (!A.made) {
    A.made = true;
    render();
    const a = picked();
    if (a) load(a);
  }

  tabShown = visible;
  coveredPreview = covered;
  liveFrames();
  if (on) checkArtifacts();
}

/// Keep the source page running while its export covers it, too.
const isLive = () => tabShown && paneShown() && !folded.has(A.key);
const keepFrame = () => paneShown() && !folded.has(A.key) &&
  (tabShown || (preview?.value.kind === "html" && preview.value.src === coveredPreview));

function liveFrames() {
  if (keepFrame() !== drawnLive) render();
}

onShownChange(liveFrames, true);

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
  if (!A.made) return;
  render();
  const a = picked();
  if (a && artifactVersion(a) !== A.version) load(a);
  else if (!a) drawShown(null);
}

/// How long a check of which artifacts are gone holds while the list stays.
const RECHECK_MS = 15_000;

/// The tab is on screen: asks the machine, in one request, which of the
/// listed files it still has. An offline machine says nothing, and nothing
/// shows as removed.
function checkArtifacts() {
  const s = paneSession();
  if (!s || !A.list.length) return;
  const paths = A.list.map(a => a.path), asked = paths.join("\n"), key = A.key;
  if (asked === A.asked && Date.now() - A.askedAt < RECHECK_MS) return;
  A.asked = asked;
  A.askedAt = Date.now();
  invoke<boolean[] | null>("agent_artifacts_present", { machineId: s.machine_id, paths }).then(present => {
    if (A.key !== key || A.asked !== asked) return;
    const was = A.gone;
    A.gone = goneArtifacts(paths, present);
    render();
    const a = picked();
    if (a && was.has(a.path) !== A.gone.has(a.path)) load(a);
  }, () => {});
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

/// The picked artifact while the tab is drawn for the pane's session,
/// whether it can be seen and the URL its page shows at, for live reload
/// to follow it.
export function followedArtifact() {
  const s = paneSession(), a = picked();
  if (!s || !A.made || A.key !== `${s.machine_id}\n${s.cwd}` || !a || A.gone.has(a.path)) return null;
  return { artifact: a, shown: isLive(), src: preview?.value.kind === "html" ? preview.value.src : null };
}

/// Reads the picked artifact again after its file changed, keeping what shows until then.
export function reloadArtifact() {
  const a = picked();
  if (a) load(a);
}

function picked() {
  return A.list.find(a => a.path === A.picked) ?? null;
}

function pick(path: string) {
  A.picked = path;
  render();
  const a = picked();
  if (a) load(a);
  refollow();
}

/// When `a`'s newest version came: when this window saw it written, else
/// when its turn began, if known.
function when(a: Artifact) {
  return A.seen.get(`${a.path}\n${artifactVersion(a)}`) ?? A.turnStart(a.turn);
}

function render() {
  const s = paneSession(), a = picked();
  if (!A.made || !s || A.key !== `${s.machine_id}\n${s.cwd}`) return;
  const fold = folded.has(A.key), inside = !!a && relativeTo(s.cwd, a.path) !== null && !A.gone.has(a.path);
  drawnLive = keepFrame();
  artifactsModel.setState({
    folded: fold,
    model: {
      artifacts: A.list, cwd: s.cwd, picked: A.picked, gone: A.gone, when,
      actions: { shown: !!a, folded: fold, note: A.note, url: a?.url ?? null, inside },
      preview, live: drawnLive, pick, act: action,
    },
  });
}

function note(text: string) {
  A.note = text;
  render();
}

function about(a: Artifact, size: number | null) {
  const s = paneSession();
  const rel = s ? relativeTo(s.cwd, a.path) ?? a.path : a.path;
  return [rel, KIND_NAMES[a.kind], size != null ? fileSize(size) : ""].filter(Boolean).join(" · ");
}

/// What an error from reading an artifact says to the user.
function readError(e: unknown) {
  const text = String(e instanceof Error ? e.message : e).replace(/^bad request: /, "");
  if (/no longer|not found|gone|is not there/i.test(text)) return `This file is not on ${paneSession()?.machine_name ?? "the machine"} any more, or it is offline.`;
  if (/unknown op|not serve|closes the stream/i.test(text)) return "Update beep-node on that machine to show this.";
  return text;
}

/// Reads `a` from the machine and shows it, unless a newer read started.
async function load(a: Artifact) {
  const s = paneSession();
  if (!s) return;
  const version = artifactVersion(a), key = A.key;

  if (A.gone.has(a.path)) {
    A.version = `gone:${version}`;
    drawShown({ kind: "message", text: goneMessage(a.name, s.machine_name), error: false });
    note(about(a, null));
    return;
  }

  A.version = version;

  if (a.kind === "pdf") {
    drawShown({ kind: "pdf", name: a.name });
    note(about(a, null));
    return;
  }

  // A page, document or picture on show stays until the new version is read.
  const keep = preview?.value.kind === "html" || preview?.value.kind === "md" || preview?.value.kind === "image";
  if (!keep) drawShown({ kind: "loading", name: a.name, machine: s.machine_name });
  note(about(a, null));
  const place = readPlace(s.cwd, a.path);

  try {
    const got = await invoke<Shown>("agent_artifact", { machineId: s.machine_id, root: place.root, path: place.path });
    if (A.key !== key || A.picked !== a.path || A.version !== version) return;
    drawShown(got.kind === "html" ? { kind: "html", name: a.name, src: got.src } : got.kind === "md" ? { kind: "md", html: got.html } : { kind: "image", name: a.name, mime: got.mime, data: got.data });
    note(about(a, got.size));
  } catch (e) {
    if (A.key !== key || A.picked !== a.path || A.version !== version) return;
    drawShown({ kind: "message", text: readError(e), error: true });
  }
}

function drawShown(value: ArtifactPreview | null) {
  preview = value && { key: ++previewRevision, value };
  render();
}

function action(act: ArtifactAction) {
  if (act === "fold") {
    if (!folded.delete(A.key)) folded.add(A.key);
    render();
    return;
  }

  const a = picked(), s = paneSession();
  if (!a || !s) return;
  const key = A.key, version = A.version;
  const stillPicked = () => A.key === key && A.picked === a.path && A.version === version;

  if (act === "pdf") wantPane({ kind: "openPath", path: a.path });
  else if (act === "claude" && a.url) invoke("open_url", { url: a.url }).catch(e => { if (stillPicked()) note(readError(e)); });
  else if (act === "file") {
    const rel = relativeTo(s.cwd, a.path);
    if (rel != null) revealPath(rel);
  } else if (act === "copy") {
    writeClipboard(a.path).then(() => { if (stillPicked()) note(`Copied ${a.path}`); }, err => { if (stillPicked()) note(`Could not copy: ${err}`); });
  }
}
