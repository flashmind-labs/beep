// The diff viewer: diffView makes one over a DiffSource and loads it.
// diff-viewer.ts holds its state, diff-view.tsx renders it, diff-comments.ts
// edits its review comments and diff-keys.ts takes its keys.
import { createElement, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { STALE } from "../ui/active.ts";
import type { MenuItem } from "../ui/menu.ts";
import { readView } from "../agentpane/view-memory.ts";
import { type DiffFile, type DiffView, type Extent, contextFile, hunkToExpand, filesInTreeOrder, reviewText, withLoaded } from "./diff-state.ts";
import { Viewer, reasonOf, reconcile, remember, say } from "./diff-viewer.ts";
import { DiffViewer } from "./diff-view.tsx";
import { addAgentComments } from "./diff-comments.ts";
import { saveDraft } from "./draftstore.ts";
import { feedbackButton } from "./feedback.ts";
import { perfOpen } from "../ui/perf.ts";

/// The one part a diff draws into the window or the session that owns it.
export type DiffSlot = "viewer";
export interface DiffOwner { set(slot: DiffSlot, content: ReactNode): void; dispose(): void }

/// Where a diff comes from and what its files can open in.
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
  /// The scrolling list of files, while it is drawn.
  files: () => HTMLElement | null;
  /// A comment being written has text.
  writing: () => boolean;
  reload: () => void;
  focus: () => void;
  dispose: () => void;
}

export function diffView(src: DiffSource): DiffViewHandle {
  const v = new Viewer(src);

  if (!src.readOnly) v.feedback = feedbackButton({
    key: src.review.key,
    machine: src.agent?.machine ?? null,
    prefer: src.agent?.prefer,
    session: src.agent?.session,
    view: () => v.state.view,
    uncommitted: () => v.state.uncommitted,
    load: async uncommitted => (await src.load([], false, uncommitted)).files,
    add: comments => addAgentComments(v, comments),
    bounds: () => v.root,
    say: text => say(v, text),
  });

  src.owner.set("viewer", createPortal(createElement(DiffViewer, { v }), v.root));
  const opened = perfOpen("diff");
  void load(v).then(() => { if (v.state.view) opened(); });

  return {
    el: v.root,
    files: () => v.files,
    writing: () => !!v.root.querySelector<HTMLTextAreaElement>(".dv-cmt.open textarea")?.value.trim(),
    reload: () => void load(v),
    focus: () => v.root.focus({ preventScroll: true }),
    dispose: () => {
      if (v.disposed) return;
      if (v.rememberTimer) remember(v);
      v.disposed = true;
      v.seq++;
      v.reads.cancel();
      clearTimeout(v.flashTimer);
      document.removeEventListener("selectionchange", v.onSelection);
      v.feedback?.dispose();
      src.owner.dispose();
    },
  };
}

/// Reads the diff again, with the context each file had expanded.
export async function load(v: Viewer) {
  const { src } = v;
  if (v.state.view) { remember(v); v.memory = readView(v.memoryKey); }
  v.seq++;
  v.context.clear();
  v.set({ loading: true, error: "" });
  v.feedback?.redraw();
  let view: DiffView | null = v.state.view, error = "";

  try {
    const read = await v.reads.run(src.load([], false, v.state.uncommitted));
    if (read === STALE) return;
    view = { ...read, files: filesInTreeOrder(read.files) };

    for (const [path, extents] of rememberedContext(v.memory)) {
      const original = read.files.find(file => file.path === path);
      if (!original || original.hunks.length !== extents.length) continue;
      const expanded = await v.reads.run(src.load([original.path], true, v.state.uncommitted, widest(extents)));
      if (expanded === STALE) return;
      const full = expanded.files.find(file => file.path === original.path);

      if (full && !full.left_out) {
        v.context.set(original.path, { original, extents });
        view = withLoaded(view, [contextFile(original, full, extents)]);
      }
    }
  } catch (e) {
    error = reasonOf(e);
  }

  if (v.disposed) return;
  const keep = v.restoredScroll || v.files?.scrollTop || 0;
  v.restoredScroll = 0;
  v.set({ view, error, loading: false });
  reconcile(v);
  if (v.files && v.files.scrollTop !== keep) v.files.scrollTop = keep;
  v.feedback?.redraw();
}

/// The context each file had expanded, as the view memory kept it.
function rememberedContext(memory: unknown): [string, Extent[]][] {
  if (!memory || typeof memory !== "object" || !("context" in memory) || !Array.isArray(memory.context)) return [];
  const lines = (value: unknown) => typeof value === "number" ? Math.max(0, Math.min(49_990, value)) : 0;
  return memory.context.flatMap((item: unknown): [string, Extent[]][] => {
    if (!Array.isArray(item) || typeof item[0] !== "string" || !Array.isArray(item[1])) return [];
    return [[item[0], item[1].map((pair: unknown) => Array.isArray(pair) ? { above: lines(pair[0]), below: lines(pair[1]) } : { above: 0, below: 0 })]];
  });
}

/// The context to read a file with so every extent has its lines.
const widest = (extents: readonly Extent[]) => Math.max(0, ...extents.map(extent => Math.max(extent.above, extent.below))) + 3;

function setPending(v: Viewer, path: string, on: boolean) {
  const pending = new Set(v.state.pending);
  if (on) pending.add(path); else pending.delete(path);
  v.set({ pending });
}

/// Reads file `f` in full, when the diff left it out.
export async function loadFull(v: Viewer, f: DiffFile) {
  if (!v.state.view || v.state.pending.has(f.path)) return;
  const seq = v.seq;
  setPending(v, f.path, true);

  try {
    const got = await v.src.load([f.path], true, v.state.uncommitted);
    if (seq !== v.seq) return;
    const view = v.state.view;
    if (view) v.set({ view: withLoaded(view, got.files) });
  } catch (e) {
    if (seq !== v.seq) return;
    v.set({ error: `Could not read ${f.path}: ${reasonOf(e)}` });
  } finally {
    setPending(v, f.path, false);
  }

  v.set({ folded: new Map(v.state.folded).set(f.path, false) });
  reconcile(v, new Set([f.path]));
}

/// Shows ten more lines of context in file `f`, above or below one hunk:
/// the one next to the old line `after`, in that direction.
export async function expandContext(v: Viewer, f: DiffFile, direction: keyof Extent, after: number) {
  if (!v.state.view || v.state.pending.has(f.path) || !f.hunks.length) return;
  const seq = v.seq;
  const kept = v.context.get(f.path) ?? { original: f, extents: f.hunks.map(() => ({ above: 0, below: 0 })) };
  const at = hunkToExpand(kept.original, direction, after);
  const next = { original: kept.original, extents: kept.extents.map((extent, index) => index === at ? { ...extent, [direction]: extent[direction] + 10 } : extent) };
  setPending(v, f.path, true);

  try {
    const got = await v.src.load([f.path], true, v.state.uncommitted, widest(next.extents));
    const view = v.state.view;
    if (seq !== v.seq || !view) return;
    const full = got.files.find(file => file.path === f.path);
    if (!full || full.left_out) throw new Error("Could not load surrounding lines");
    v.context.set(f.path, next);
    v.set({ view: withLoaded(view, [contextFile(kept.original, full, next.extents)]) });
    reconcile(v, new Set([f.path]));
    remember(v);
  } catch (error) {
    say(v, `Could not expand context: ${reasonOf(error)}`);
  } finally {
    setPending(v, f.path, false);
  }
}

/// Takes file `path` back to the context it was read with.
export function collapseContext(v: Viewer, path: string) {
  const kept = v.context.get(path);
  const view = v.state.view;
  if (!kept || !view) return;

  v.context.delete(path);
  v.set({ view: withLoaded(view, [kept.original]) });
  reconcile(v, new Set([path]));
  remember(v);
}

/// Sends the draft as one review message; what went leaves the draft.
export async function sendReview(v: Viewer) {
  const { sending, draft, view } = v.state;
  if (sending || !draft.length) return;
  const sent = draft.slice();
  v.set({ sending: true, error: "" });
  const why = await v.src.review.send(reviewText(v.src.review.heading(), view?.files ?? [], sent));

  if (why) v.set({ sending: false, error: why });
  else {
    const rest = v.state.draft.filter(c => !sent.includes(c));
    saveDraft(v.src.review.key, rest);
    v.set({ sending: false, draft: rest });
  }

  v.root.focus({ preventScroll: true });
}
