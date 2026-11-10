// The diff's review comment editors: the draft's comments on lines and files,
// new ones opened from a line number or a header, and open editors whose
// line left the diff, kept with their text under the lost comments.
import { createElement, Fragment } from "react";
import { createPortal } from "react-dom";
import { DiffComment } from "./diff-comment.tsx";
import { lineLabel, type ReviewComment } from "./diff-state.ts";
import { type EditorRestore, type Viewer, placed, render } from "./diff-viewer.ts";
import { drawReview, reconcileFiles } from "./diff-render.ts";
import { saveDraft } from "./draftstore.ts";
import { mergeComments } from "./feedback-state.ts";
import { el } from "../ui/dom.ts";

/// A click on a line's number opens a box under its row (under the pair, side by side).
export function onGutter(v: Viewer, e: MouseEvent) {
  if (v.src.readOnly || !(e.target instanceof Element)) return;
  const g = e.target.closest<HTMLElement>(".dv-g");
  const row = g?.parentElement;
  const path = g?.closest<HTMLElement>(".dv-file")?.dataset.path;
  if (!row || !path || row.matches(".hunk, .gap, .none")) return;
  const pair = row.parentElement?.classList.contains("dv-pair") ? row.parentElement : null;
  const del = row.classList.contains("del");
  // On an unchanged line the right cell has the new number.
  const nums = (!del && pair ? pair.lastElementChild : row)?.firstElementChild?.children;
  if (!nums) return;
  const line = Number((del ? nums[0] : nums[nums.length - 1]).textContent);
  if (!line) return;
  let at: Element = pair ?? row;
  while (at.nextElementSibling?.matches(".dv-cmt, .dv-comment-slot")) at = at.nextElementSibling;
  addBox(v, { path, side: del ? "old" : "new", line, text: "" }, box => at.after(box));
}

/// A new comment goes where `put` says and enters the draft on Add.
export function addBox(v: Viewer, comment: ReviewComment, put: (box: HTMLElement) => void) {
  put(commentHost(v, comment, "", true));
  drawComments(v);
}

/// Draft comments from an agent's review, left out when the draft has them already; says how many were new.
export function addAgentComments(v: Viewer, add: ReviewComment[]) {
  const merged = mergeComments(v.draft, add);
  if (!merged.added) return 0;
  v.draft = merged.draft;
  // A review may finish after its viewer closes; keep the draft without reviving its UI.
  if (v.disposed) { saveDraft(v.src.review.key, v.draft); return merged.added; }
  changed(v);
  const keep = v.files.scrollTop;
  if (reconcileFiles(v, new Set(add.map(comment => comment.path))) && v.files.scrollTop !== keep) v.files.scrollTop = keep;
  return merged.added;
}

/// The draft changed: keep it and show it.
function changed(v: Viewer) {
  v.placedCache = null;
  saveDraft(v.src.review.key, v.draft);
  drawReview(v);
}

export function commentHost(v: Viewer, comment: ReviewComment, label: string, isNew: boolean, restored: EditorRestore | null = null) {
  const host = el("span", "dv-comment-slot");
  host.style.display = "contents";
  v.comments.set(host, { id: ++v.nextComment, comment, label, isNew, chunkOwned: false, restored });
  return host;
}

export function forgetCommentsIn(v: Viewer, container: Element) {
  for (const host of v.comments.keys()) if (container.contains(host)) v.comments.delete(host);
}

/// Whether a comment on `path` is being edited.
export function editingIn(v: Viewer, path: string) {
  return [...v.comments].some(([node, item]) => item.comment.path === path && node.querySelector(".dv-cmt.open"));
}

export function drawComments(v: Viewer) {
  const portals = [...v.comments].map(([host, entry]) => createPortal(createElement(DiffComment, {
    comment: entry.comment, label: entry.label, initiallyEditing: entry.isNew || !!entry.restored, isNew: entry.isNew, restored: entry.restored,
    onSave: value => {
      entry.comment.text = value;
      if (entry.isNew) { v.draft.push(entry.comment); entry.isNew = false; }
      entry.restored = null;
      if (v.orphaned.has(entry.comment)) v.orphaned.set(entry.comment, null);
      changed(v);
    },
    onRemove: () => {
      const index = v.draft.indexOf(entry.comment);
      if (index >= 0) v.draft.splice(index, 1);
      v.comments.delete(host);
      v.orphaned.delete(entry.comment);
      v.lostComments.delete(entry.comment);
      if (!entry.chunkOwned) host.remove();
      changed(v);
    },
    onCancelNew: () => { v.comments.delete(host); v.orphaned.delete(entry.comment); v.lostComments.delete(entry.comment); host.remove(); changed(v); },
    onEscape: () => v.root.focus({ preventScroll: true }),
    onEditEnd: () => {
      entry.restored = null;
      if (v.orphaned.has(entry.comment)) v.orphaned.set(entry.comment, null);
      const path = entry.comment.path;
      if (!v.deferredFiles.has(path)) return;

      queueMicrotask(() => {
        if (editingIn(v, path)) return;
        v.deferredFiles.delete(path);
        v.renderedFiles.delete(path);
        const top = v.files.scrollTop;
        if (reconcileFiles(v) && v.files.scrollTop !== top) v.files.scrollTop = top;
      });
    },
  }), host, String(entry.id)));
  render(v, "comments", createElement(Fragment, null, ...portals));
}

/// Keeps the text of open editors among the lost comments before the
/// files are drawn again.
export function keepOrphans(v: Viewer) {
  for (const [comment, host] of v.lostComments) {
    if (!v.orphaned.has(comment)) continue;
    const input = host.querySelector<HTMLTextAreaElement>(".dv-cmt.open textarea"), entry = v.comments.get(host);
    if (!input || !entry) continue;
    const focused = document.activeElement === input;
    if (focused) v.revealOrphan = true;
    v.orphaned.set(comment, { text: input.value, focused, start: input.selectionStart, end: input.selectionEnd, isNew: entry.isNew });
  }
}

/// The comments on files or lines the diff no longer has, above the files.
export function drawLostComments(v: Viewer) {
  const lost = v.view ? placed(v).lost : [];
  const kept = new Set(lost);

  for (const [comment, host] of v.lostComments) {
    if (kept.has(comment)) continue;
    v.comments.delete(host);
    host.remove();
    v.lostComments.delete(comment);
  }

  if (lost.length && v.lostHeading.parentElement !== v.lostBox) v.lostBox.prepend(v.lostHeading);
  if (!lost.length) v.lostHeading.remove();
  let cursor = v.lostHeading.nextSibling;

  for (const comment of lost) {
    let host = v.lostComments.get(comment);

    if (!host) {
      const restored = v.orphaned.get(comment) ?? null;
      host = commentHost(v, comment, `${comment.path} ${lineLabel(comment)}`.trim(), restored?.isNew ?? false, restored);
      v.lostComments.set(comment, host);
    }

    if (host === cursor) cursor = cursor.nextSibling;
    else v.lostBox.insertBefore(host, cursor);
  }

  drawComments(v);

  for (const [comment, restored] of v.orphaned) {
    if (!restored?.focused) continue;
    const input = v.lostComments.get(comment)?.querySelector<HTMLTextAreaElement>("textarea");
    if (input && document.activeElement === input) { input.scrollIntoView({ block: "nearest" }); restored.focused = false; }
  }
}
