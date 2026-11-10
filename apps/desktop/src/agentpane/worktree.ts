// The worktree the pane's Files and Changes tabs read. It starts on the
// session's own folder and follows the agent into the worktrees of the
// repository (`worktree-state.ts` reads where it works from the log). A
// worktree picked by hand stops that until "Follow the agent" is chosen
// again. Nothing here runs git checkout: only what the pane reads changes.
// State is per session and kept in memory.

import { type Entry, type Tree, finishedTools, follow, followSteps, matchesTree, removedCheckout, reportedRoot, rootOf, treeName } from "./worktree-state.ts";
import { onPlaces, placeOf } from "../agents/place.ts";
import { type WorkPlace, placeName } from "../agents/place-state.ts";
import { baseName } from "./media-state.ts";
import { createElement } from "react";
import { hasSessionPortal, removeSessionPortal, setSessionPortal } from "../agents/session-frame.tsx";
import { WorktreeMenu } from "./worktree-view.tsx";
import { invoke } from "../ui/tauri.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";

/// The session the pane belongs to.
interface Place {
  id: string;
  machine_id: string;
  cwd: string;
  agent: string;
  external?: boolean;
}

/// How old the session's reported place may be, in milliseconds.
const REPORT_MS = 5000;

const W = {
  place: null as Place | null,
  trees: [] as Tree[],
  /// The folder the pane works from: the session's, or its main checkout once
  /// the session's own is gone (a removed worktree).
  cwd: "",
  reading: false,
  /// Asked to read the list while it was being read.
  again: false,
  follow: true,
  /// Where the machine says the session works; ahead of what the log shows.
  placed: null as WorkPlace | null,
  /// The root picked by hand.
  pick: null as string | null,
  /// The root the agent last worked in, once it left the session's folder or came back.
  seen: null as string | null,
  /// The root relative paths in the log are taken from, and the place of
  /// the first entry not read yet.
  last: "",
  next: 0,
  /// The root of the subagent shown, and a pick made while it shows.
  child: null as string | null,
  handled: false,
  log: { entries: [] as Entry[], earlier: 0 },
  /// What made the list be read again, each once.
  asked: new Set<string>(),
  /// The root the tabs read now.
  shown: "",
  open: false,
  search: "",
  finished: new Set<number>(),
  changed: (_root: boolean) => {},
};

let firstRead: Promise<void> = Promise.resolve();

/// Initial reads wait until a removed worktree has resolved to its checkout.
export function worktreesReady(): Promise<void> { return firstRead; }

/// Whether the first read of the session's worktrees finished, so the root
/// shown is no longer only the session's folder by default.
let settled = false;
export const worktreesSettled = () => settled;

const bare = (dir: string) => dir.replace(/\/+$/, "") || "/";

/// Starts on session `place`; `changed` hears that the root moved (true)
/// or that the menu needs drawing again (false).
export function startWorktrees(place: Place, changed: (root: boolean) => void) {
  Object.assign(W, {
    place, trees: [], reading: false, again: false, follow: true, placed: null, pick: null, seen: null, cwd: bare(place.cwd), last: bare(place.cwd), next: 0, child: null, handled: false,
    log: { entries: [], earlier: 0 }, asked: new Set(), shown: bare(place.cwd), open: false, search: "", finished: new Set<number>(), changed,
  });
  settled = false;
  firstRead = readTrees().finally(() => { if (W.place === place) settled = true; });
  report();
}

/// Reads where the machine says the session works, and follows it.
function report() {
  const p = W.place;
  // The empty agent view has no session to ask about yet.
  if (!p?.id) return;
  const got = placeOf({ machine_id: p.machine_id, agent: p.agent, id: p.id, external: p.external }, REPORT_MS);
  if (JSON.stringify(got) === JSON.stringify(W.placed)) return;
  W.placed = got;
  if (got?.linked) W.seen = reportedRoot(W.trees, W.cwd, got.folder) ?? W.seen;
  if (got && !W.trees.some(t => bare(t.path) === bare(got.folder))) ask([got.folder]);
  apply(true);
  W.changed(false);
}
onPlaces(report);

/// The folder Files and Changes read.
export function paneRoot() {
  const p = W.place;
  return !p ? "" : W.shown === bare(p.cwd) ? p.cwd : W.shown;
}

/// Whether the folder the tabs read is a linked worktree, not the main
/// checkout: git lists the main checkout first.
export function paneLinked() {
  const root = paneRoot();
  const at = W.trees.findIndex(t => rootOf(t, W.cwd) === root);
  if (at >= 0) return at > 0;
  return !!W.placed?.linked && bare(W.placed.folder) === bare(root);
}

/// The last revision heard of each folder's `files` topic, by machine. A new
/// subscription is primed with the last event again, which is no change.
const heard = new Map<string, number>();

/// Calls `changed` when the node says something in `root` on `machine`
/// changed, its Git metadata included. The subscription stays while a view
/// of the folder is kept, shown or not, so returning to it knows whether to
/// read again. Returns the unsubscribe.
export function watchRoot(machine: string, root: string, changed: () => void): () => void {
  const name = topic("files", root), id = `${machine}\n${name}`;
  let seen = heard.get(id) ?? null;
  return subscribeEach(name, event => {
    if (event.revision === seen) return;
    seen = event.revision;
    heard.set(id, seen);
    changed();
  }, { machine });
}

/// Reads the list of worktrees again, as the Changes tab does on a refresh.
export function refreshWorktrees() {
  readTrees();
}

async function readTrees() {
  const p = W.place;
  if (!p?.cwd) return;
  if (W.reading) { W.again = true; return; }
  W.reading = true;
  let listed = false;

  try {
    const asked = await invoke<Tree[] | null>("agent_worktrees", { machineId: p.machine_id, folder: p.cwd });
    const got = asked ?? [];
    if (W.place?.id !== p.id) return;

    if (JSON.stringify(got) !== JSON.stringify(W.trees)) {
      W.trees = got;
      listed = true;
      // A removed worktree: go on from the main checkout. The log is read
      // again from its start, against the new list and folder.
      W.cwd = removedCheckout(got, p.cwd) ?? bare(p.cwd);
      if (W.seen && !got.some(t => rootOf(t, W.cwd) === W.seen)) W.seen = null;
      Object.assign(W, { next: W.log.earlier, last: W.seen ?? W.cwd });
      const roots = got.map(t => rootOf(t, W.cwd));
      if (W.pick && !roots.includes(W.pick)) W.pick = null;
    }
  } catch {
    // A node from before worktrees, or a machine only a server reaches: no list.
  } finally {
    if (W.place?.id === p.id) W.reading = false;
  }

  if (W.place?.id !== p.id) return;
  scan();
  apply(true);
  if (listed) W.changed(false);
  if (W.again) { W.again = false; readTrees(); }
}

/// The root the tabs should read: the subagent's, else the hand pick or
/// where the agent works.
function want() {
  const p = W.place;
  const reported = p && W.placed ? reportedRoot(W.trees, W.cwd, W.placed.folder) : null;
  return W.child ?? (W.follow ? W.seen ?? reported : W.pick) ?? W.cwd;
}

/// A comment box with text in the Changes tab: a switch waits for it to close.
function typing() {
  return !!document.querySelector<HTMLTextAreaElement>(".agpview.changes .dv-cmt.open textarea")?.value.trim();
}

/// Points the tabs at `want()`. A switch the agent or a subagent caused
/// waits while a comment is being written.
function apply(auto: boolean) {
  const next = want();
  if (next === W.shown) return;
  if (auto && typing()) return;
  W.shown = next;
  W.changed(true);
}

// The switch that waited goes ahead as the comment box closes.
for (const type of ["click", "keyup", "focusout"]) {
  document.addEventListener(type, () => {
    if (W.place && want() !== W.shown) setTimeout(() => apply(true), 0);
  }, true);
}

/// Reads the entries not read yet, or the log again after the list changed.
function scan() {
  const p = W.place;
  const { entries, earlier } = W.log;
  if (!p?.cwd) return;
  if (W.next < earlier || W.next > earlier + entries.length) Object.assign(W, { next: earlier, last: W.cwd });
  const got = follow(entries.slice(W.next - earlier), W.trees, W.cwd, W.last);
  W.next += got.done;
  W.last = got.kept;
  if (got.hit) W.seen = got.last;
  ask(got.unknown);
}

/// Reads the list again for something in the log it may not have.
function ask(keys: string[]) {
  // No list: not a repository, or a node that does not say.
  if (!W.trees.length) return;
  const fresh = keys.filter(k => !W.asked.has(k));
  for (const k of fresh) W.asked.add(k);
  if (fresh.length) readTrees();
}

/// The log as it was drawn: its `entries` from place `earlier`, and the
/// titles of the steps of the subagent open, null on the session itself.
export function followAgent(entries: Entry[], earlier: number, steps: string[] | null) {
  const p = W.place;
  if (!p?.cwd) return;
  W.log = { entries, earlier };
  const finished = finishedTools(entries, earlier);
  const changed = finished.some(index => !W.finished.has(index));
  for (const index of finished) W.finished.add(index);
  for (const index of W.finished) if (index < earlier) W.finished.delete(index);
  if (changed) readTrees();
  if (!steps) W.handled = false;
  W.child = null;

  if (steps && !W.handled) {
    const got = followSteps(steps, W.trees, W.cwd);
    if (got.hit) W.child = got.last;
    ask(got.unknown);
  }

  scan();
  apply(true);
  report();
}

// ----- The dropdown -----

function pickRoot(root: string | null) {
  W.handled = true;
  W.child = null;
  W.open = false;
  W.search = "";
  if (root == null) {
    W.follow = !W.follow;
    W.pick = W.follow ? null : W.shown;
  } else Object.assign(W, { follow: false, pick: root });
  apply(false);
  W.changed(false);
}

/// The session's folder is gone and the pane works from its main checkout.
function removed(p: Place) {
  return W.cwd !== bare(p.cwd);
}

/// The branch the pane shows in place of a removed worktree, while it is the
/// one showing; null otherwise.
export function removedBranch() {
  const p = W.place;
  const main = W.trees.find(t => t.own);
  return p && main && removed(p) && W.shown === W.cwd ? treeName(main) : null;
}

/// Draws the dropdown into `host`, in the header of a dock: only while the
/// dock shows Files or Changes (`here`) and there is more than one worktree
/// to show, or the pane is not on the session's folder.
export function drawWorktrees(host: HTMLElement, here: boolean) {
  const p = W.place;
  const show = !!p && here && (W.trees.length > 1 || W.shown !== bare(p.cwd) || removed(p) || !!W.placed?.linked);
  const sig = show ? JSON.stringify([W.trees, W.shown, W.follow, W.seen, W.open, W.search, W.cwd, W.placed]) : "";
  if (host.dataset.sig === sig && (!show || hasSessionPortal(host))) return;
  host.dataset.sig = sig;
  host.hidden = !show;
  if (!show || !p) { removeSessionPortal(host); return; }
  const now = W.trees.find(t => rootOf(t, W.cwd) === W.shown);
  const label = now ? treeName(now) : W.placed && bare(W.placed.folder) === W.shown ? placeName(W.placed) : baseName(W.shown);
  const main = W.trees.find(t => t.own);
  const items = W.trees.map(t => {
    const path = rootOf(t, W.cwd);
    const sub = t.own ? (removed(p) ? "main checkout" : "the session's folder")
      : main && t.path.startsWith(`${bare(main.path)}/`) ? t.path.slice(bare(main.path).length + 1) : t.path;
    return { path, title: t.path, name: treeName(t), sub: sub + (W.seen === path ? " · agent here" : ""), visible: matchesTree(t, W.search) };
  });
  setSessionPortal(host, createElement(WorktreeMenu, {
    label, items, shown: W.shown, follow: W.follow, open: W.open, search: W.search,
    pick: pickRoot, searchChanged: (value: string) => { W.search = value; W.changed(false); },
    toggle: () => {
      W.open = !W.open;
      W.search = "";
      W.changed(false);
      if (W.open) host.querySelector<HTMLInputElement>(".agwt-search")?.focus();
    },
  }));
}

/// Closes the menu; true when one was open.
export function closeWorktreeMenu() {
  if (!W.open) return false;
  W.open = false;
  W.changed(false);
  return true;
}

window.addEventListener("pointerdown", e => {
  if (W.open && !(e.target as Element).closest(".agwtw")) closeWorktreeMenu();
}, true);
window.addEventListener("keydown", e => {
  if (e.key === "Escape" && closeWorktreeMenu()) { e.preventDefault(); e.stopImmediatePropagation(); }
}, true);
