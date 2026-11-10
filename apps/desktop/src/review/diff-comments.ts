// The diff's review comments: the draft's comments on lines and files, new
// ones opened from a line number or a header, and open editors whose line
// left the diff, kept with their text under the lost comments.
import type { ReviewComment } from "./diff-state.ts";
import { type DiffState, type Viewer, reconcile } from "./diff-viewer.ts";
import { saveDraft } from "./draftstore.ts";
import { mergeComments } from "./feedback-state.ts";

const ids = new WeakMap<ReviewComment, number>();
let nextId = 0;

/// A key for `comment` that lasts as long as the comment.
export function commentKey(comment: ReviewComment) {
  let id = ids.get(comment);
  if (id === undefined) ids.set(comment, id = ++nextId);
  return id;
}

/// Opens an editor on `comment`: a new one from a line number or a header, or a saved one to edit.
export function openEditor(v: Viewer, comment: ReviewComment, isNew: boolean) {
  if (v.src.readOnly) return;
  v.set({ editors: new Map(v.state.editors).set(comment, isNew) });
}

/// The editor's text becomes the comment; a new one enters the draft.
export function saveComment(v: Viewer, comment: ReviewComment, text: string) {
  const { draft, editors, orphaned } = v.state;
  const saved = { ...comment, text };
  const next = editors.get(comment) ? [...draft, saved] : draft.map(c => c === comment ? saved : c);
  const open = new Map(editors);
  open.delete(comment);
  const lost = new Set(orphaned);
  if (lost.delete(comment)) lost.add(saved);
  changed(v, { draft: next, editors: open, orphaned: lost });
}

/// Closes the editor; a new comment goes with it.
export function cancelComment(v: Viewer, comment: ReviewComment) {
  const { editors, orphaned } = v.state;
  const open = new Map(editors);
  open.delete(comment);
  const lost = new Set(orphaned);
  if (editors.get(comment)) lost.delete(comment);
  changed(v, { editors: open, orphaned: lost });
}

export function removeComment(v: Viewer, comment: ReviewComment) {
  const { draft, editors, orphaned } = v.state;
  const open = new Map(editors);
  open.delete(comment);
  const lost = new Set(orphaned);
  lost.delete(comment);
  changed(v, { draft: draft.filter(c => c !== comment), editors: open, orphaned: lost });
}

/// Draft comments from an agent's review, left out when the draft has them already; says how many were new.
export function addAgentComments(v: Viewer, add: ReviewComment[]) {
  const merged = mergeComments(v.state.draft, add);
  if (!merged.added) return 0;
  // A review may finish after its viewer closes; keep the draft without reviving its UI.
  if (v.disposed) { saveDraft(v.src.review.key, merged.draft); v.store.setState({ draft: merged.draft }); return merged.added; }
  changed(v, { draft: merged.draft });
  return merged.added;
}

/// The draft or its editors changed: keep the draft, and draw any file held for an editor that closed.
function changed(v: Viewer, change: Partial<DiffState>) {
  if (change.draft) saveDraft(v.src.review.key, change.draft);
  v.set(change);
  const top = v.files?.scrollTop ?? 0;
  reconcile(v);
  if (v.files && v.files.scrollTop !== top) v.files.scrollTop = top;
}
