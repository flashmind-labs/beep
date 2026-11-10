// The diff viewer: diffView makes one over a DiffSource and loads it.
// diff-viewer.ts holds its state, diff-render.ts draws it, diff-comments.ts
// edits its review comments and diff-keys.ts takes its keys.
import type { ReactNode } from "react";
import { STALE } from "../ui/active.ts";
import type { MenuItem } from "../ui/menu.ts";
import { readView } from "../agentpane/view-memory.ts";
import { type DiffFile, type DiffView, contextFile, filesInTreeOrder, reviewText, withLoaded } from "./diff-state.ts";
import { NARROW, Viewer, WIDE, reasonOf, remember, render } from "./diff-viewer.ts";
import { drawFiles, drawHead, drawReview, reconcileFiles, redrawBodies, redrawFile, say } from "./diff-render.ts";
import { addAgentComments, editingIn, onGutter } from "./diff-comments.ts";
import { markCurrent, onKey } from "./diff-keys.ts";
import { initTreeResize } from "./treewidth.ts";
import { saveDraft } from "./draftstore.ts";
import { feedbackButton } from "./feedback.ts";

/// Where a diff comes from and what its files can open in.
export type DiffSlot = "toolbar" | "status" | "tree" | "chunks" | "headers" | "body-controls" | "comments" | "feedback";
export interface DiffOwner { set(slot: DiffSlot, content: ReactNode): void; dispose(): void }

export interface DiffSource {
  owner: DiffOwner;
  /// History views show immutable patches without session review controls.
  readOnly?: boolean;
  /// Reads the diff; with `only`, just those files in full.
  load: (only: string[], lockfiles: boolean, uncommitted: boolean, context?: number) => Promise<DiffView>;
  /// Offers the Uncommitted only toggle, off to begin with.
  uncommitted?: boolean;
  /// Opens a file of the diff, when something can show it.
  open?: { label: string; run: (path: string) => void };
  /// More lines for a file's right-click menu, read once it is open; none for a file that has no more.
  menu?: (path: string, at: { x: number; y: number }) => Promise<MenuItem[]>;
  /// Where review comments are kept and go: `key` names the draft in
  /// storage, `heading` titles the review message, `send` delivers it and
  /// says why it did not ("" when it went).
  review: { key: string; heading: () => string; send: (text: string) => Promise<string> };
  /// The machine the diff's checkout is on, for Review with agent; a diff
  /// without one has the button disabled. `prefer` is the harness offered first;
  /// `session` is the session whose changes these are, whose child the reviewer is.
  agent?: { machine: { id: string; name: string }; prefer?: string; session?: string };
}

export interface DiffViewHandle {
  el: HTMLElement;
  reload: () => void;
  focus: () => void;
  dispose: () => void;
}

export function diffView(src: DiffSource): DiffViewHandle {
  const v = new Viewer(src);
  const { root, files } = v;

  if (!src.readOnly) v.feedback = feedbackButton({
    key: src.review.key,
    machine: src.agent?.machine ?? null,
    prefer: src.agent?.prefer,
    session: src.agent?.session,
    view: () => v.view,
    uncommitted: () => v.uncommitted,
    load: async uncommitted => (await src.load([], false, uncommitted)).files,
    add: comments => addAgentComments(v, comments),
    bounds: () => root,
    say: text => say(v, text),
  }, view => render(v, "feedback", view));

  // Scrolling writes the position at most a few times a second.
  files.addEventListener("scroll", () => { if (!v.rememberTimer) v.rememberTimer = window.setTimeout(() => remember(v), 250); }, { passive: true });
  const stopVirtual = v.virtual._didMount();
  const stopTreeResize = initTreeResize(v.body, v.grip);
  root.append(v.head, v.feedback.note, v.body, v.keys);

  const rootResize = new ResizeObserver(() => {
    root.classList.toggle("wide", root.clientWidth >= WIDE);
    const narrow = root.clientWidth <= NARROW;
    if (narrow === v.narrow) return;
    v.narrow = narrow;
    if (v.split) redrawBodies(v);
  });
  rootResize.observe(root);
  const filesResize = new ResizeObserver(() => markCurrent(v));
  filesResize.observe(files);
  files.addEventListener("scroll", () => {
    if (v.scrollFrame) return;
    v.scrollFrame = requestAnimationFrame(() => { v.scrollFrame = 0; markCurrent(v); });
  }, { passive: true });
  files.addEventListener("click", e => onGutter(v, e));
  root.addEventListener("keydown", e => onKey(v, e));

  drawReview(v);
  void load(v);

  return {
    el: root,
    reload: () => void load(v),
    focus: () => root.focus({ preventScroll: true }),
    dispose: () => {
      if (v.disposed) return;
      if (v.rememberTimer) remember(v);
      v.disposed = true;
      v.seq++;
      v.reads.cancel();
      clearTimeout(v.flashTimer);
      if (v.scrollFrame) cancelAnimationFrame(v.scrollFrame);
      v.chunkObserver.disconnect(); rootResize.disconnect(); filesResize.disconnect(); stopTreeResize(); stopVirtual();
      if (v.watchingSelection) document.removeEventListener("selectionchange", v.onSelection);
      v.bodyControls.clear();
      v.feedback.dispose(); src.owner.dispose();
    },
  };
}

/// Reads the diff again, with the context each file had expanded.
export async function load(v: Viewer) {
  const { src } = v;
  if (v.view) { remember(v); v.memory = readView(v.memoryKey); }
  v.seq++;
  v.context.clear();
  v.loading = true;
  v.error = "";
  drawHead(v);
  if (!v.view) drawFiles(v);

  try {
    const view = await v.reads.run(src.load([], false, v.uncommitted));
    if (view === STALE) return;
    v.view = { ...view, files: filesInTreeOrder(view.files) };

    for (const [path, above, below] of rememberedContext(v.memory)) {
      const original = view.files.find(file => file.path === path);
      if (!original) continue;
      const expanded = await v.reads.run(src.load([original.path], true, v.uncommitted, Math.max(above, below) + 3));
      if (expanded === STALE || !v.view) return;
      const full = expanded.files.find(file => file.path === original.path);

      if (full && !full.left_out) {
        v.context.set(original.path, { original, above, below });
        v.view = withLoaded(v.view, [contextFile(original, full, above, below)]);
      }
    }
  } catch (e) {
    v.error = reasonOf(e);
  }

  v.loading = false;
  const keep = v.restoredScroll || v.files.scrollTop;
  v.restoredScroll = 0;
  drawHead(v);
  if (reconcileFiles(v) && v.files.scrollTop !== keep) v.files.scrollTop = keep;

  if (v.revealOrphan) {
    v.revealOrphan = false;
    const active = document.activeElement;
    if (active instanceof HTMLElement && v.lostBox.contains(active)) active.scrollIntoView({ block: "nearest" });
  }
}

/// The context each file had expanded, as the view memory kept it.
function rememberedContext(memory: unknown): [string, number, number][] {
  if (!memory || typeof memory !== "object" || !("context" in memory) || !Array.isArray(memory.context)) return [];
  return memory.context.flatMap((item: unknown): [string, number, number][] => {
    if (!Array.isArray(item) || typeof item[0] !== "string" || typeof item[1] !== "number" || typeof item[2] !== "number") return [];
    return [[item[0], Math.max(0, Math.min(49_990, item[1])), Math.max(0, Math.min(49_990, item[2]))]];
  });
}

/// Reads file `f` in full, when the diff left it out.
export async function loadFull(v: Viewer, f: DiffFile) {
  if (!v.view || v.pending.has(f.path)) return;
  const seq = v.seq;
  v.pending.add(f.path);
  if (editingIn(v, f.path)) v.deferredFiles.add(f.path);
  else redrawFile(v, f.path);

  try {
    const got = await v.src.load([f.path], true, v.uncommitted);
    if (seq !== v.seq) return;
    if (v.view) v.view = withLoaded(v.view, got.files);
  } catch (e) {
    if (seq !== v.seq) return;
    v.error = `Could not read ${f.path}: ${reasonOf(e)}`;
  } finally {
    v.pending.delete(f.path);
  }

  v.folded.set(f.path, false);
  reconcileFiles(v, new Set([f.path]));
}

/// Shows ten more lines of context around file `f`.
export async function expandContext(v: Viewer, f: DiffFile, direction: "above" | "below") {
  if (!v.view || v.pending.has(f.path)) return;
  const seq = v.seq;
  const kept = v.context.get(f.path) ?? { original: f, above: 0, below: 0 };
  const next = { ...kept, [direction]: kept[direction] + 10 };
  v.pending.add(f.path);

  try {
    const got = await v.src.load([f.path], true, v.uncommitted, Math.max(next.above, next.below) + 3);
    if (seq !== v.seq || !v.view) return;
    const full = got.files.find(file => file.path === f.path);
    if (!full || full.left_out) throw new Error("Could not load surrounding lines");
    v.context.set(f.path, next);
    v.view = withLoaded(v.view, [contextFile(kept.original, full, next.above, next.below)]);
    v.placedCache = null;
    redrawFile(v, f.path);
    remember(v);
  } catch (error) {
    say(v, `Could not expand context: ${reasonOf(error)}`);
  } finally {
    v.pending.delete(f.path);
  }
}

/// Sends the draft as one review message; what went leaves the draft.
export async function sendReview(v: Viewer) {
  if (v.sending || !v.draft.length) return;
  const sent = v.draft.slice();
  v.sending = true;
  v.error = "";
  drawReview(v);
  const why = await v.src.review.send(reviewText(v.src.review.heading(), v.view?.files ?? [], sent));
  v.sending = false;

  if (why) {
    v.error = why;
  } else {
    v.draft = v.draft.filter(c => !sent.includes(c));
    saveDraft(v.src.review.key, v.draft);
  }

  const keep = v.files.scrollTop;
  drawHead(v);
  drawFiles(v);
  v.files.scrollTop = keep;
  v.root.focus({ preventScroll: true });
}
