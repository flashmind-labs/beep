// The Changes tab of a coding agent session's pane: what changed in the
// session's folder against its default branch, commits, uncommitted
// changes and new files alike, read by git on the session's machine the
// first time the tab shows and again each time it shows after.

import type { DiffView } from "../review/diff-state.ts";
import { type DiffViewHandle, diffView } from "../review/diff.ts";
import { openWorkFile, paneSession } from "./media.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";

const C = {
  /// The session's machine and folder the view is of.
  key: "",
  el: null as HTMLElement | null,
  view: null as DiffViewHandle | null,
};

/// The Changes view of the pane's session, made the first time it shows.
export function changesView() {
  const s = paneSession();
  const key = s ? `${s.machine_id}\n${s.cwd}` : "";
  if (C.el && C.key === key) return C.el;
  C.el?.remove();
  C.key = key;
  const el = h('<div class="agpview changes"></div>');
  const view = diffView({
    load: (only, lockfiles) => s
      ? invoke<DiffView>("work_diff", { machineId: s.machine_id, root: s.cwd, only, lockfiles })
      : Promise.reject(new Error("No session shows.")),
    open: { label: "Open in a tab", run: openWorkFile },
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
