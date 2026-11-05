// The Changes tab of a coding agent session's pane: what changed in the
// session's folder against its default branch, commits, uncommitted
// changes and new files alike, read by git on the session's machine the
// first time the tab shows and again each time it shows after. It is of
// the worktree the pane shows (`worktree.ts`) and reads again on a switch.

import type { DiffView } from "../review/diff-state.ts";
import { type DiffViewHandle, diffView } from "../review/diff.ts";
import { sendReview } from "../agents/messages.ts";
import { openWorkFile, paneSession } from "./media.ts";
import { paneRoot, refreshWorktrees } from "./worktree.ts";
import { reviewIn } from "./worktree-state.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";

const C = {
  /// The session and folder the view is of.
  key: "",
  el: null as HTMLElement | null,
  view: null as DiffViewHandle | null,
};

/// The Changes view of the pane's session, made the first time it shows.
export function changesView() {
  const s = paneSession();
  const key = s ? `${s.machine_id}\n${s.id}\n${s.cwd}` : "";
  if (C.el && C.key === key) return C.el;
  C.el?.remove();
  C.key = key;
  const el = h('<div class="agpview changes"></div>');
  // A session with no folder yet has no changes to read.
  if (!s?.cwd) {
    Object.assign(C, { el, view: null });
    return el;
  }
  const view = diffView({
    load: (only, lockfiles) => {
      // A refresh reads the worktrees again too.
      if (!only.length) refreshWorktrees();
      return invoke<DiffView>("work_diff", { machineId: s.machine_id, root: paneRoot(), only, lockfiles });
    },
    open: { label: "Open in a tab", run: openWorkFile },
    review: { key: `${s.machine_id}\n${s.id}`, send: text => sendReview(reviewIn(text, paneRoot(), s.cwd)) },
  });
  el.append(view.el);
  Object.assign(C, { el, view });
  return el;
}

/// The view, when it was made for the pane's session.
export function madeChangesView() {
  return C.el;
}

/// Lets go of the view, as the pane starts another session or closes.
export function dropChanges() {
  C.el?.remove();
  Object.assign(C, { el: null, view: null, key: "" });
}

/// Reads the changes again, as the tab shows once more.
export function reloadChanges() {
  C.view?.reload();
}

/// Puts the keyboard in the diff.
export function focusChanges() {
  C.view?.focus();
}
