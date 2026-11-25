// The worktree the pane's Files and Changes tabs read. It starts on the
// session's own folder and follows the agent into the worktrees of the
// repository (`worktree-state.ts` reads where it works from the log). A
// worktree picked by hand stops that until "Follow the agent" is chosen
// again. Nothing here runs git checkout: only what the pane reads changes.
// State is per session and kept in memory.

import { type Entry, type Tree, follow, followSteps, matchesTree, removedCheckout, reportedRoot, rootOf, treeName } from "./worktree-state.ts";
import { onPlaces, placeOf, takePlaces } from "../agents/place.ts";
import { type WorkPlace, placeName } from "../agents/place-state.ts";
import { baseName } from "./media-state.ts";
import type { WorktreeMenuModel } from "./worktree-view.tsx";
import { writingComment } from "./changes.ts";
import { invoke } from "../ui/tauri.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedView } from "../ui/snapshot-state.ts";

/// The session the pane belongs to.
interface Place {
  id: string;
  machine_id: string;
  cwd: string;
  agent: string;
  external?: boolean;
}

const KNOWN = 8;
const bare = (dir: string) => dir.replace(/\/+$/, "") || "/";
const sessionKey = (p: Place) => (p.id ? `${p.machine_id}\n${p.id}\n${bare(p.cwd)}` : "");

/// Per-session worktree tracking, owned directly while shown and kept in `known`.
interface SessionWorktrees {
  trees: Tree[];
  cwd: string;
  follow: boolean;
  placed: WorkPlace | null;
  pick: string | null;
  seen: string | null;
  last: string;
  next: number;
  handled: boolean;
  log: { entries: Entry[]; earlier: number };
  shown: string;
}

const makeSession = (cwd: string): SessionWorktrees => ({
  trees: [], cwd, follow: true, placed: null, pick: null, seen: null, last: cwd, next: 0,
  handled: false, log: { entries: [], earlier: 0 }, shown: cwd,
});

/// The sessions shown last, by machine and session, most recent last.
const known = new Map<string, SessionWorktrees>();
let current: SessionWorktrees = makeSession("");

const view = {
  place: null as Place | null,
  reading: false,
  again: false,
  child: null as string | null,
  open: false,
  search: "",
  changed: (_root: boolean) => {},
};

let firstRead: Promise<void> = Promise.resolve();
let showAll = false;
let settled = false;

/// Initial reads wait until a removed worktree has resolved to its checkout.
export function worktreesReady(): Promise<void> { return firstRead; }
export const worktreesSettled = () => settled;

/// Starts on session `place`; `changed` hears that the root moved (true)
/// or that the menu needs drawing again (false).
export function startWorktrees(place: Place, changed: (root: boolean) => void) {
  const key = sessionKey(place);
  let entry = key ? known.get(key) ?? null : null;
  const kept = entry !== null;

  if (!entry) entry = makeSession(bare(place.cwd));
  if (key) {
    known.delete(key);
    known.set(key, entry);
    for (const old of known.keys()) if (known.size > KNOWN) known.delete(old);
  }

  current = entry;
  Object.assign(view, { place, reading: false, again: false, child: null, open: false, search: "", changed });
  followFiles(place);
  settled = kept;
  const read = readTrees().finally(() => { if (view.place === place) settled = true; });
  firstRead = kept ? Promise.resolve() : read;
  report();
}

/// Reads where the machine says the session works, and follows it.
function report() {
  const p = view.place;
  if (!p?.id) return;
  const got = placeOf({ machine_id: p.machine_id, agent: p.agent, id: p.id, external: p.external });
  if (JSON.stringify(got) === JSON.stringify(current.placed)) return;
  const moved = current.placed !== null && got?.folder !== current.placed.folder;
  current.placed = got;
  if (got && (got.linked || moved)) {
    const root = reportedRoot(current.trees, current.cwd, got.folder);
    if (root) { current.seen = root; current.last = root; }
  }
  apply(true);
  view.changed(false);
}
onPlaces(report);

/// The folder Files and Changes read.
export function paneRoot() {
  const p = view.place;
  return !p ? "" : current.shown === bare(p.cwd) ? p.cwd : current.shown;
}

/// The top of the checkout holding the session's folder when that folder
/// is a subfolder of it, such as a session in `apps/desktop`; else null.
export function checkoutTop() {
  const own = current.trees.find(t => t.own);
  return own && bare(own.path) !== bare(current.cwd) ? bare(own.path) : null;
}

/// Whether the folder the tabs read is a linked worktree, not the main
/// checkout: git lists the main checkout first.
export function paneLinked() {
  const root = paneRoot();
  const at = current.trees.findIndex(t => rootOf(t, current.cwd) === root);
  if (at >= 0) return at > 0;
  return !!current.placed?.linked && bare(current.placed.folder) === bare(root);
}

const heard = new Map<string, number>();

/// Calls `changed` with the update when the node says something in `root` on `machine`
/// changed, its Git metadata included.
export function watchRoot(machine: string, root: string, changed: (data: unknown) => void): () => void {
  const name = topic("files", root), id = `${machine}\n${name}`;
  let seen = heard.get(id) ?? null;
  return subscribeEach(name, event => {
    if (event.revision === seen) return;
    seen = event.revision;
    heard.set(id, seen);
    changed(event.data);
  }, { machine });
}

/// Follows the shown session's folder: each update of it carries the worktree list when it changed.
let followed: { place: Place; stop: () => void } | null = null;

function followFiles(place: Place) {
  if (followed?.place.id === place.id && followed.place.machine_id === place.machine_id && followed.place.cwd === place.cwd) return;
  followed?.stop();
  followed = place.cwd ? { place, stop: subscribeEach(topic("files", place.cwd), event => { takeWorktrees(event.data); takePlaces(event.data); }, { machine: place.machine_id }) } : null;
}

/// Takes the worktree list a `files` update pushes for the shown session.
export function takeWorktrees(data: unknown) {
  const p = view.place;
  if (!p?.cwd) return;
  const pushed = pushedView<Tree[] | null>(data, "agent_worktrees", { machineId: p.machine_id, folder: p.cwd, session: p.id || null });
  if (pushed && "value" in pushed) listTrees(p, current, pushed.value ?? []);
}

/// Keeps the list `got` for session `p` when it is still the one shown, and follows the agent in it.
function listTrees(p: Place, session: SessionWorktrees, got: Tree[]) {
  if (view.place?.id !== p.id || current !== session) return;
  const listed = JSON.stringify(got) !== JSON.stringify(session.trees);

  if (listed) {
    session.trees = got;
    session.cwd = removedCheckout(got, p.cwd) ?? bare(p.cwd);
    if (session.seen && !got.some(t => rootOf(t, session.cwd) === session.seen)) session.seen = null;
    session.next = session.log.earlier;
    session.last = session.seen ?? session.cwd;
    const roots = got.map(t => rootOf(t, session.cwd));
    if (session.pick && !roots.includes(session.pick)) session.pick = null;
  }

  scan();
  apply(true);
  if (listed) view.changed(false);
}

/// Reads the list once when the session opens; the updates of its folder keep it after that.
async function readTrees() {
  const p = view.place, session = current;
  if (!p?.cwd) return;
  if (view.reading) { view.again = true; return; }
  view.reading = true;
  let got: Tree[] | null = null;

  try {
    got = await invoke<Tree[] | null>("agent_worktrees", { machineId: p.machine_id, folder: p.cwd, session: p.id || null }) ?? [];
  } catch {
    // A node from before worktrees, or one out of reach: no list.
  } finally {
    if (view.place?.id === p.id && current === session) view.reading = false;
  }

  if (view.place?.id !== p.id || current !== session) return;
  listTrees(p, session, got ?? session.trees);
  if (view.again) { view.again = false; void readTrees(); }
}

function want() {
  const reported = view.place && current.placed ? reportedRoot(current.trees, current.cwd, current.placed.folder) : null;
  return view.child ?? (current.follow ? current.seen ?? reported : current.pick) ?? current.cwd;
}

function apply(auto: boolean) {
  const next = want();
  if (next === current.shown) return;
  if (auto && writingComment()) return;
  current.shown = next;
  view.changed(true);
}

for (const type of ["click", "keyup", "focusout"]) {
  document.addEventListener(type, () => {
    if (view.place && want() !== current.shown) setTimeout(() => apply(true), 0);
  }, true);
}

function scan() {
  if (!view.place?.cwd) return;
  const { entries, earlier } = current.log;
  if (current.next < earlier || current.next > earlier + entries.length) { current.next = earlier; current.last = current.cwd; }
  const got = follow(entries.slice(current.next - earlier), current.trees, current.cwd, current.last);
  current.next += got.done;
  current.last = got.kept;
  if (got.hit) current.seen = got.last;
}

export function followAgent(entries: Entry[], earlier: number, steps: string[] | null) {
  if (!view.place?.cwd) return;
  if (earlier < current.log.earlier) {
    current.next = earlier;
    current.last = current.cwd;
  }
  current.log = { entries, earlier };
  if (!steps) current.handled = false;
  view.child = null;

  if (steps && !current.handled) {
    const got = followSteps(steps, current.trees, current.cwd);
    if (got.hit) view.child = got.last;
  }

  scan();
  apply(true);
  report();
}

// ----- The dropdown -----

function pickRoot(root: string | null) {
  Object.assign(view, { child: null, open: false, search: "" });
  current.handled = true;
  if (root == null) { current.follow = !current.follow; current.pick = current.follow ? null : current.shown; }
  else { current.follow = false; current.pick = root; }
  apply(false);
  view.changed(false);
}

const removed = (p: Place) => current.cwd !== bare(p.cwd);

export function removedBranch() {
  const p = view.place;
  const main = current.trees.find(t => t.own);
  return p && main && removed(p) && current.shown === current.cwd ? treeName(main) : null;
}

export function worktreeMenu(): WorktreeMenuModel | null {
  const p = view.place;
  if (!p || !(current.trees.length > 1 || current.shown !== bare(p.cwd) || removed(p) || !!current.placed?.linked)) return null;
  const now = current.trees.find(t => rootOf(t, current.cwd) === current.shown);
  const label = now ? treeName(now) : current.placed && bare(current.placed.folder) === current.shown ? placeName(current.placed) : baseName(current.shown);
  const main = current.trees.find(t => t.own);
  const listed = current.trees.filter(t => {
    const path = rootOf(t, current.cwd);
    return showAll || t.own || t.created || path === current.shown || path === current.seen;
  });
  const items = listed.map(t => {
    const path = rootOf(t, current.cwd);
    const sub = t.own ? (removed(p) ? "main checkout" : "the session's folder")
      : main && t.path.startsWith(`${bare(main.path)}/`) ? t.path.slice(bare(main.path).length + 1) : t.path;
    return { path, title: t.path, name: treeName(t), sub: sub + (current.seen === path ? " · agent here" : ""), visible: matchesTree(t, view.search) };
  });
  return {
    label, items, shown: current.shown, follow: current.follow, open: view.open, search: view.search, all: showAll,
    pick: pickRoot, showAll: (all: boolean) => { showAll = all; view.changed(false); }, searchChanged: (value: string) => { view.search = value; view.changed(false); },
    toggle: () => { view.open = !view.open; view.search = ""; view.changed(false); },
  };
}

function closeWorktreeMenu() {
  if (!view.open) return false;
  view.open = false;
  view.changed(false);
  return true;
}

window.addEventListener("pointerdown", e => {
  if (view.open && !(e.target instanceof Element && e.target.closest(".agwtw"))) closeWorktreeMenu();
}, true);
window.addEventListener("keydown", e => {
  if (e.key === "Escape" && closeWorktreeMenu()) { e.preventDefault(); e.stopImmediatePropagation(); }
}, true);
