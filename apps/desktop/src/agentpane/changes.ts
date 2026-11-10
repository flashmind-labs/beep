// The Changes tab of a coding agent session's pane: what changed in the
// session's folder against its default branch, commits, uncommitted
// changes and new files alike, read by git on the session's machine the
// first time the tab shows. It is of the worktree the pane shows
// (`worktree.ts`). The views of the last few sessions are kept: showing one
// again draws what it had, and reads git again only when the folder's
// `files` topic said something changed, the worktree moved, or on Refresh.

import type { DiffView } from "../review/diff-state.ts";
import { type DiffViewHandle, type DiffOwner, type DiffSlot, diffView } from "../review/diff.ts";
import { sendReview } from "../agents/messages.ts";
import { openWorkFile, paneSession } from "./media.ts";
import { localFileItems } from "./localfile.ts";
import { paneLinked, paneRoot, refreshWorktrees, removedBranch, watchRoot, worktreesReady, worktreesSettled } from "./worktree.ts";
import { reviewHeading } from "./worktree-state.ts";
import { homeShort } from "../project/projects-state.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";
import { SLOW_MS, ambient, paneShown } from "../ui/active.ts";
import { setSessionChild } from "../agents/session-frame.tsx";
import type { ReactNode } from "react";

/// How many sessions keep their diff, and the diff lines all of them may hold.
const KEPT = 4;
const KEPT_LINES = 200_000;

/// A session's diff view, kept while another session shows.
interface Kept {
  /// The session and folder the view is of.
  key: string;
  machine: string;
  /// The worktree it last read, and its subscription.
  root: string;
  unwatch: () => void;
  el: HTMLElement;
  view: DiffViewHandle;
  owner: SessionDiffOwner;
  /// The line saying the session's worktree was removed.
  note: HTMLElement;
  /// The folder changed since the last full read.
  dirty: boolean;
  /// Lines in the last full read, to drop the largest first.
  lines: number;
  scroll: number;
}

interface SessionDiffOwner extends DiffOwner {
  /// Draws the slots again in the session frame now showing, which starts
  /// without them after another session showed.
  replay(): void;
}

let nextDiff = 0;
function sessionDiffOwner(): SessionDiffOwner {
  const id = ++nextDiff, slots = new Map<DiffSlot, ReactNode>();
  return {
    set(slot: DiffSlot, content: ReactNode) { slots.set(slot, content); setSessionChild(`diff:${id}:${slot}`, content); },
    dispose() { for (const slot of slots.keys()) setSessionChild(`diff:${id}:${slot}`, null); slots.clear(); },
    replay() { for (const [slot, content] of slots) setSessionChild(`diff:${id}:${slot}`, content); },
  };
}

/// Most recently shown last.
const kept = new Map<string, Kept>();
let shown: Kept | null = null;
/// The view of a session with no folder yet, which has no changes to read.
let blank: HTMLElement | null = null;

const sessionKey = () => {
  const s = paneSession();
  return s?.cwd ? `${s.machine_id}\n${s.id}\n${s.cwd}` : "";
};

/// The Changes view of the pane's session, made the first time it shows.
export function changesView() {
  const key = sessionKey();
  if (!key) { hide(); return blank ??= h('<div class="agpview changes"></div>'); }
  const have = kept.get(key);
  return (have ? pick(have) : make(key)).el;
}

/// The view, when one is kept for the pane's session.
export function madeChangesView() {
  const have = kept.get(sessionKey());
  if (!have) { hide(); return null; }
  return pick(have).el;
}

function make(key: string) {
  const s = paneSession();
  if (!s) throw new Error("the pane has no session");
  const el = h('<div class="agpview changes"></div>');
  const note = h('<div class="dv-note" hidden></div>');
  const owner = sessionDiffOwner();
  // The view reads as it is made, so what its reads need comes first.
  const entry: Omit<Kept, "view"> = { key, machine: s.machine_id, root: paneRoot(), unwatch: () => {}, el, owner, note, dirty: false, lines: 0, scroll: 0 };
  hide();
  onScreen = false;
  const view = diffView({
    owner,
    uncommitted: true,
    load: async (only, lockfiles, uncommitted, context) => {
      const full = !only.length;

      // A full read is current until the folder changes again; a refresh reads the worktrees again too.
      if (full) {
        entry.dirty = false;
        if (shown === entry) refreshWorktrees();
      }

      const view = await invoke<DiffView>("work_diff", { machineId: entry.machine, root: entry.root, only, lockfiles, uncommitted, context });
      if (full) { entry.lines = linesOf(view); trim(); }
      return view;
    },
    open: { label: "Open in a tab", run: openWorkFile },
    menu: (path, at) => localFileItems({ machineId: entry.machine, root: entry.root, path }, at),
    review: { key: `${s.machine_id}\n${s.id}`, heading: () => reviewHeading(homeShort(entry.root), paneLinked()), send: sendReview },
    agent: { machine: { id: s.machine_id, name: s.machine_name }, prefer: s.agent, session: s.id },
  });
  const made: Kept = Object.assign(entry, { view });
  shown = made;
  watch(made);
  el.append(note, view.el);
  kept.set(key, made);
  drawNote();
  trim();
  return made;
}

/// Makes `entry` the view on screen, drawn as it was left.
function pick(entry: Kept) {
  if (shown !== entry) {
    hide();
    shown = entry;
    onScreen = false;
    kept.delete(entry.key);
    kept.set(entry.key, entry);
    // The dock it goes back into is not laid out yet.
    requestAnimationFrame(() => {
      const files = entry.el.querySelector<HTMLElement>(".dv-files");
      if (shown === entry && files && entry.scroll) files.scrollTop = entry.scroll;
    });
  }

  entry.owner.replay();
  return entry;
}

/// Takes the shown view out of the pane, keeping it.
function hide() {
  if (!shown) return;
  // A view whose session frame went has no scroll left to keep.
  if (shown.el.isConnected) shown.scroll = shown.el.querySelector<HTMLElement>(".dv-files")?.scrollTop ?? 0;
  shown.el.remove();
  shown = null;
}

function watch(entry: Kept) {
  entry.unwatch();
  entry.unwatch = watchRoot(entry.machine, entry.root, () => {
    entry.dirty = true;
    if (shown === entry) changed();
  });
}

const changesShown = () => !!shown?.el.offsetParent && paneShown();
/// A folder that changes while its diff shows is read again at most this often.
const changed = ambient(() => { if (shown?.dirty && changesShown()) shown.view.reload(); }, SLOW_MS, changesShown);

/// The diff lines of a read, a cheap measure of what the view holds.
function linesOf(view: DiffView) {
  let lines = 0;
  for (const file of view.files) for (const hunk of file.hunks) lines += hunk.lines.length;
  return lines;
}

/// Lets go of the views past `KEPT`, the least recent first, and of the
/// largest while all of them hold more than `KEPT_LINES`.
function trim() {
  const others = () => [...kept.values()].filter(entry => entry !== shown);
  let total = [...kept.values()].reduce((sum, entry) => sum + entry.lines, 0);

  while (kept.size > KEPT || total > KEPT_LINES) {
    const rest = others();
    if (!rest.length) return;
    const drop = kept.size > KEPT ? rest[0] : rest.reduce((a, b) => b.lines > a.lines ? b : a);
    total -= drop.lines;
    release(drop);
  }
}

function release(entry: Kept) {
  kept.delete(entry.key);
  if (shown === entry) shown = null;
  entry.unwatch();
  entry.view.dispose();
  entry.el.remove();
}

/// Says so while the pane shows the main checkout in place of a removed worktree.
function drawNote() {
  const branch = removedBranch();
  if (!shown) return;
  shown.note.hidden = !branch;
  shown.note.textContent = branch ? `The session's worktree was removed. Showing ${branch}.` : "";
}

/// Whether the shown view was on screen when last asked.
let onScreen = false;

/// The tab drew, or the worktree it reads may have moved; `visible` says
/// whether the view is on screen. A view that comes on screen reads the
/// changes again at once when the folder changed since its last read, or
/// the worktree is another; one already on screen at a slower pace while
/// the agent works. Kept as it is otherwise.
export function revalidateChanges(visible = changesShown()) {
  drawNote();
  const now = visible && paneShown(), appeared = now && !onScreen;
  onScreen = now;
  const entry = shown;
  if (!entry || entry.key !== sessionKey()) return;

  // Until the worktrees are read the root is the session's folder by default,
  // not a move away from the worktree the view was of.
  if (worktreesSettled()) check(entry, now, appeared);
  else void worktreesReady().then(() => { if (shown === entry) check(entry, changesShown(), appeared); });
}

function check(entry: Kept, now: boolean, appeared: boolean) {
  const root = paneRoot();

  if (root && root !== entry.root) {
    entry.root = root;
    entry.dirty = true;
    watch(entry);
    if (now) { entry.view.reload(); return; }
  }

  if (!entry.dirty || !now) return;
  if (appeared) entry.view.reload();
  else changed();
}

/// Puts the keyboard in the diff.
export function focusChanges() {
  shown?.view.focus();
}
