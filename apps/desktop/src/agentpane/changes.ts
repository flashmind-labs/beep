// The Changes tab of a coding agent session's pane: what changed in the
// session's folder against its default branch, commits, uncommitted
// changes and new files alike, read by git on the session's machine the
// first time the tab shows. It is of the worktree the pane shows
// (`worktree.ts`). The views of the last few sessions are kept: showing one
// again draws what it had, and reads git again only when the folder's
// `files` topic said something changed, the worktree moved, or on Refresh.

import type { DiffView } from "../review/diff-state.ts";
import { type DiffViewHandle, type DiffOwner, diffView } from "../review/diff.tsx";
import { CHANGES } from "./media-state.ts";
import { askPane, paneSession } from "./pane-state.ts";
import { dockOf, paneModel } from "./media-layout.ts";
import { localFileItems } from "./localfile.ts";
import { touchesRepo } from "./touched.ts";
import { paneLinked, paneRoot, removedBranch, watchRoot, worktreesReady, worktreesSettled } from "./worktree.ts";
import { reviewHeading } from "./worktree-state.ts";
import { homeShort } from "../project/projects-state.ts";
import { viewKey } from "./view-memory.ts";
import { invoke } from "../ui/tauri.ts";
import { paneShown } from "../ui/active.ts";
import { setSessionChild } from "../agents/session-frame-state.ts";
import type { ReactNode } from "react";
import { createStore } from "zustand";

/// How many sessions keep their diff, and the diff lines all of them may hold.
const KEPT = 4;
const KEPT_LINES = 200_000;

/// A session's diff view, kept while another session shows.
export interface Kept {
  /// The session and folder the view is of.
  key: string;
  machine: string;
  /// The worktree it last read, and its subscription.
  root: string;
  unwatch: () => void;
  view: DiffViewHandle;
  owner: SessionDiffOwner;
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
  const key = `diff:${++nextDiff}:viewer`;
  let content: ReactNode = null;
  return {
    set(next: ReactNode) { content = next; setSessionChild(key, content); },
    dispose() { if (content !== null) setSessionChild(key, null); content = null; },
    replay() { if (content !== null) setSessionChild(key, content); },
  };
}

/// Most recently shown last.
const kept = new Map<string, Kept>();
let shown: Kept | null = null;

/// What the Changes view draws: the diff of the session on screen (none for
/// a session with no folder yet, which has no changes to read), and the line
/// saying its worktree was removed.
interface ChangesModel { entry: Kept | null; note: string }
export const changesModel = createStore<ChangesModel>(() => ({ entry: null, note: "" }));

const sessionKey = () => {
  const s = paneSession();
  return s?.cwd ? `${s.machine_id}\n${s.id}\n${s.cwd}` : "";
};

/// The pane drew the Changes tab: `on` when it is the dock's tab on screen,
/// `visible` when it can be seen. Its view is made the first time it shows,
/// and kept for the session after that. True when a view draws.
export function changesTab(on: boolean, visible: boolean) {
  const key = sessionKey(), have = kept.get(key);

  if (have) pick(have);
  else if (key && on) make(key);
  else hide();

  publish();
  const drawn = !!shown || (!key && on);
  if (drawn) revalidateChanges(visible);
  return drawn;
}

function publish() {
  changesModel.setState({ entry: shown, note: noteText() });
}

/// A comment being written in the diff shown has text.
export function writingComment() {
  return shown?.view.writing() ?? false;
}

/// The view shown left the pane, or moved to the other dock: what it scrolled to is kept.
export function leftChanges(entry: Kept) {
  if (entry.view.el.isConnected) entry.scroll = entry.view.files()?.scrollTop ?? 0;
  entry.view.el.remove();
}

/// The view came into the pane, laid out in the next frame: it scrolls back to where it was.
export function placedChanges(entry: Kept) {
  requestAnimationFrame(() => {
    const files = entry.view.files();
    if (shown === entry && files && entry.scroll) files.scrollTop = entry.scroll;
  });
}

function make(key: string) {
  const s = paneSession();
  if (!s) throw new Error("the pane has no session");
  const owner = sessionDiffOwner();
  // The view reads as it is made, so what its reads need comes first.
  const entry: Omit<Kept, "view"> = { key, machine: s.machine_id, root: paneRoot(), unwatch: () => {}, owner, dirty: false, lines: 0, scroll: 0 };
  hide();
  onScreen = false;
  const view = diffView({
    owner,
    uncommitted: true,
    zoom: { zoomed: () => paneModel.getState().zoomed === dockOf(CHANGES), subscribe: paneModel.subscribe, enter: () => askPane({ kind: "show", tab: "changes" }) },
    load: async (only, lockfiles, uncommitted, context) => {
      const full = !only.length;

      // A full read is current until the folder changes again.
      if (full) entry.dirty = false;

      let view: DiffView;
      try {
        view = await invoke<DiffView>("work_diff", { machineId: entry.machine, root: entry.root, only, lockfiles, uncommitted, context });
      } catch (e) {
        // A failed read leaves the view as stale as before, so the next show reads again.
        if (full) entry.dirty = true;
        throw e;
      }

      if (full) { entry.lines = linesOf(view); trim(); }
      return view;
    },
    open: { label: "Open in a tab", run: rel => askPane({ kind: "open", rel, preview: false }) },
    menu: (path, at) => localFileItems({ machineId: entry.machine, root: entry.root, path }, at),
    review: { key: viewKey(s.machine_id, s.id), heading: () => reviewHeading(homeShort(entry.root), paneLinked()), send: async text => await paneSession()?.sendReview?.(text) ?? "No session is open." },
    agent: { machine: { id: s.machine_id, name: s.machine_name }, prefer: s.agent, session: s.id },
  });
  const made: Kept = Object.assign(entry, { view });
  shown = made;
  watch(made);
  kept.set(key, made);
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
  }

  entry.owner.replay();
  return entry;
}

/// Takes the shown view out of the pane, keeping it.
function hide() {
  shown = null;
}

function watch(entry: Kept) {
  entry.unwatch();
  entry.unwatch = watchRoot(entry.machine, entry.root, (_, paths) => {
    if (!touchesRepo(paths)) return;
    entry.dirty = true;
    if (shown === entry) changed();
  });
}

/// The diff's tab is picked (a view behind another keeps its layout) in a dock not folded.
const changesShown = () => !!shown?.view.el.offsetParent && !shown.view.el.parentElement?.hidden && paneShown();
/// A change to the folder's checkout while its diff shows reads it again, once per push.
function changed() {
  if (shown?.dirty && changesShown()) shown.view.reload();
}

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
  entry.view.el.remove();
  if (changesModel.getState().entry === entry) publish();
}

/// Says so while the pane shows the main checkout in place of a removed worktree.
function noteText() {
  const branch = removedBranch();
  return branch ? `The session's worktree was removed. Showing ${branch}.` : "";
}

/// Whether the shown view was on screen when last asked.
let onScreen = false;

/// The tab drew, or the worktree it reads may have moved; `visible` says
/// whether the view is on screen. A view that comes on screen reads the
/// changes again at once when the folder changed since its last read, or
/// the worktree is another; one already on screen at a slower pace while
/// the agent works. Kept as it is otherwise.
export function revalidateChanges(visible = changesShown()) {
  if (changesModel.getState().note !== noteText()) publish();
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
