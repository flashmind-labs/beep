// The worktree the pane's Files and Changes tabs read. It starts on the
// session's own folder and follows the agent into the worktrees of the
// repository (`worktree-state.ts` reads where it works from the log). A
// worktree picked by hand stops that until "Follow the agent" is chosen
// again. Nothing here runs git checkout: only what the pane reads changes.
// State is per session and kept in memory.

import { type Entry, type Tree, follow, followSteps, removedCheckout, rootOf, treeName } from "./worktree-state.ts";
import { baseName } from "./media-state.ts";
import { esc } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";

/// The session the pane belongs to.
interface Place {
  id: string;
  machine_id: string;
  cwd: string;
}

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
  changed: (_root: boolean) => {},
};

const bare = (dir: string) => dir.replace(/\/+$/, "") || "/";

/// Starts on session `place`; `changed` hears that the root moved (true)
/// or that the menu needs drawing again (false).
export function startWorktrees(place: Place, changed: (root: boolean) => void) {
  Object.assign(W, {
    place, trees: [], reading: false, again: false, follow: true, pick: null, seen: null, cwd: bare(place.cwd), last: bare(place.cwd), next: 0, child: null, handled: false,
    log: { entries: [], earlier: 0 }, asked: new Set(), shown: bare(place.cwd), open: false, changed,
  });
  readTrees();
}

/// The folder Files and Changes read.
export function paneRoot() {
  const p = W.place;
  return !p ? "" : W.shown === bare(p.cwd) ? p.cwd : W.shown;
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
      Object.assign(W, { next: 0, last: W.cwd, seen: null });
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
  return W.child ?? (W.follow ? W.seen : W.pick) ?? W.cwd;
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
  if (!steps) W.handled = false;
  W.child = null;
  if (steps && !W.handled) {
    const got = followSteps(steps, W.trees, W.cwd);
    if (got.hit) W.child = got.last;
    ask(got.unknown);
  }
  scan();
  apply(true);
}

// ----- The dropdown -----

function pickRoot(root: string | null) {
  W.handled = true;
  W.child = null;
  W.open = false;
  if (root == null) W.follow = true;
  else Object.assign(W, { follow: false, pick: root });
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

function menuHtml(p: Place) {
  const main = W.trees.find(t => t.own);
  const gone = removed(p);
  const items = W.trees.map(t => {
    const root = rootOf(t, W.cwd);
    const sub = t.own ? (gone ? "main checkout" : "the session's folder")
      : main && t.path.startsWith(`${bare(main.path)}/`) ? t.path.slice(bare(main.path).length + 1) : t.path;
    const here = W.seen === root ? " · agent here" : "";
    return `<button role="menuitemradio" aria-checked="${root === W.shown}" data-wt="${esc(root)}" title="${esc(t.path)}"><code>${esc(treeName(t))}</code><small>${esc(sub)}${here}</small></button>`;
  }).join("");
  return `<div class="agwtm" role="menu"><button role="menuitemcheckbox" aria-checked="${W.follow}" data-wt="follow"><b>Follow the agent</b></button><hr>${items}</div>`;
}

/// Draws the dropdown into `host`, in the header of a dock: only while the
/// dock shows Files or Changes (`here`) and there is more than one worktree
/// to show, or the pane is not on the session's folder.
export function drawWorktrees(host: HTMLElement, here: boolean) {
  const p = W.place;
  const show = !!p && here && (W.trees.length > 1 || W.shown !== bare(p.cwd) || removed(p));
  const sig = show ? JSON.stringify([W.trees, W.shown, W.follow, W.seen, W.open, W.cwd]) : "";
  if (host.dataset.sig === sig) return;
  host.dataset.sig = sig;
  host.hidden = !show;
  if (!show || !p) return host.replaceChildren();
  const now = W.trees.find(t => rootOf(t, W.cwd) === W.shown);
  const label = now ? treeName(now) : baseName(W.shown);
  host.innerHTML = `<button class="agpbtn agwt" aria-haspopup="menu" aria-expanded="${W.open}" title="The worktree Files and Changes show"><code>${esc(label)}</code><span aria-hidden="true">&#9662;</span></button>${W.open ? menuHtml(p) : ""}`;
  host.onclick = e => {
    const target = e.target as HTMLElement;
    const item = target.closest<HTMLElement>("[data-wt]");
    if (item) return pickRoot(item.dataset.wt === "follow" ? null : item.dataset.wt!);
    if (target.closest(".agwt")) { W.open = !W.open; W.changed(false); }
  };
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
