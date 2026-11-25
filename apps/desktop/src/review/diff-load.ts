// Reading a diff into its viewer, and sending its review.
import { STALE } from "../ui/active.ts";
import { readView } from "../agentpane/view-memory.ts";
import { type DiffFile, type DiffView, type ContextEdit, type Extent, contextFile, editExtents, filesInTreeOrder, reviewText, withLoaded } from "./diff-state.ts";
import { type Viewer, reasonOf, reconcile, remember, say } from "./diff-viewer.ts";
import { saveDraft } from "./draftstore.ts";

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
    v.set({ folded: new Map(v.state.folded).set(f.path, false) });
    reconcile(v, new Set([f.path]));
  } catch (e) {
    if (seq !== v.seq) return;
    v.set({ error: `Could not read ${f.path}: ${reasonOf(e)}` });
  } finally {
    setPending(v, f.path, false);
  }
}

/// Shows more context around hunks of file `f`, or takes it away again.
export async function editContext(v: Viewer, f: DiffFile, edits: readonly ContextEdit[]) {
  const view = v.state.view;
  if (!view || v.state.pending.has(f.path)) return;
  const seq = v.seq;
  const kept = v.context.get(f.path) ?? { original: f, extents: f.hunks.map(() => ({ above: 0, below: 0 })) };
  const next = { original: kept.original, extents: editExtents(kept.extents, edits) };

  if (next.extents.every(extent => !extent.above && !extent.below)) {
    v.context.delete(f.path);
    v.set({ view: withLoaded(view, [kept.original]) });
    reconcile(v, new Set([f.path]));
    remember(v);
    return;
  }

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
    say(v, `Could not change context: ${reasonOf(error)}`);
  } finally {
    setPending(v, f.path, false);
  }
}

/// Sends the draft as one review message; what went leaves the draft.
export async function sendReview(v: Viewer) {
  const { sending, draft, view } = v.state;
  if (sending || !draft.length) return;
  const sent = draft.slice();
  v.set({ sending: true, error: "" });
  let why: string;

  try {
    why = await v.src.review.send(reviewText(v.src.review.heading(), view?.files ?? [], sent));
  } catch (error) {
    why = reasonOf(error);
  }

  if (why) v.set({ sending: false, error: why });
  else {
    const rest = v.state.draft.filter(c => !sent.includes(c));
    saveDraft(v.src.review.key, rest);
    v.set({ sending: false, draft: rest });
  }

  v.root.focus({ preventScroll: true });
}
