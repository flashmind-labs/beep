// The diff viewer: diffView makes one over a DiffSource and loads it.
// diff-viewer.ts holds its state, diff-view.tsx renders it, diff-comments.ts
// edits its review comments and diff-keys.ts takes its keys.
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import type { MenuItem } from "../ui/menu.tsx";
import type { DiffView } from "./diff-state.ts";
import { Viewer, remember, say } from "./diff-viewer.ts";
import { DiffViewer } from "./diff-view.tsx";
import { load } from "./diff-load.ts";
import { addAgentComments } from "./diff-comments.ts";
import { feedbackButton } from "./feedback.ts";
import { perfOpen } from "../ui/perf.ts";

/// The window or session that owns a diff and draws its one viewer.
export interface DiffOwner { set(content: ReactNode): void; dispose(): void }

/// Where a diff comes from and what its files can open in.
export interface DiffSource {
  owner: DiffOwner;
  /// History views show immutable patches without session review controls.
  readOnly?: boolean;
  /// Reads the diff; with `only`, just those files in full.
  load: (only: string[], lockfiles: boolean, uncommitted: boolean, context?: number) => Promise<DiffView>;
  /// Offers the Uncommitted only toggle, off to begin with.
  uncommitted?: boolean;
  /// A side pane that ⇧esc fills the session with: its file list shows only
  /// then, and the Files button zooms it first.
  zoom?: { zoomed: () => boolean; subscribe: (changed: () => void) => () => void; enter: () => void };
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
    say: text => say(v, text),
  });

  src.owner.set(createPortal(<DiffViewer v={v} />, v.root));
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
