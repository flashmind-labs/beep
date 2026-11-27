// The state of a coding agent session's pane that the tabs and the docks share:
// what a tab is, the session the pane belongs to, and the signals that ask the
// pane to draw again. It imports no tab, so every tab may import it.
import type { EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { createStore } from "zustand";
import type { AgentSession } from "../agents/agents-state.ts";
import type { TerminalRow } from "../terminals/terminal-state.ts";
import type { Dock, ImageView, MediaKind } from "./media-state.ts";
import type { ModelView } from "./model3d.ts";

import type { PaneTerm, TermPlace } from "./terminal.ts";


/// What the node gives of a file, a part at a time (`agent_file`).
export interface FilePart {
  size: number;
  mime: string;
  /// Base64.
  data: string;
}

/// The start of a file of the Files tab, as `agent_tree_text` reads it.
export interface TreeText {
  size: number;
  text: string;
  cut: boolean;
  binary: boolean;
  /// `text` is all of the file, read without change, so it can be edited.
  editable: boolean;
}

/// A text tab being edited: the text it was opened or last saved with, the
/// editor once it shows and its state while it does not (with its undo
/// history), and how a save goes.
export interface FileEdit {
  base: string;
  view: EditorView | null;
  state: EditorState | null;
  /// Counts reloads, so the editor starts again from `base`.
  generation: number;
  dirty: boolean;
  saving: boolean;
  /// A save found the file changed since `base`; Overwrite or Reload decide.
  conflict: boolean;
  /// Where the cursor starts, such as the place double-clicked to edit.
  at: TextPlace | null;
}

/// Creates a fresh `FileEdit` from `base`, optionally placing the cursor at `at`.
export function newFileEdit(base: string, at: TextPlace | null = null): FileEdit {
  return { base, view: null, state: null, generation: 0, dirty: false, saving: false, conflict: false, at };
}

/// A place in a text by its 0-based line and column.
export interface TextPlace { line: number; column: number }

/// What a tab shows: a media file, a file of the Files tab as text, a
/// terminal, or another coding agent session beside this one.
export type TabKind = MediaKind | "text" | "term" | "session";

/// A coding agent session opened beside the primary conversation in a dock tab.
export interface SplitSession {
  machine_id: string;
  machine_name: string;
  agent: string;
  id: string;
  title: string | null;
  cwd: string;
  status: string;
  external: boolean;
}

/// One tab: a file of the session's machine, a picture already here, or a
/// terminal on the machine.
export interface MediaTab {
  /// The path on the machine, the picture's data URL, `text:` and the
  /// path in the session's folder, or `term:` and its name.
  id: string;
  kind: TabKind;
  name: string;
  /// Where the node reads it, for a file of the machine.
  path: string | null;
  /// Its bytes as a URL once here, and how many there were.
  url: string | null;
  size: number;
  /// When it started loading, bytes read so far, and why it could not be.
  started: number;
  done: number;
  error: string;
  /// Counts changes to what its view shows, so the view draws again.
  rev: number;
  /// Its view was last drawn hidden behind another tab.
  viewHidden: boolean;
  /// What a save or copy said, shown in its footer.
  note: string | null;
  /// Local copy of an HTML export, and the preview it came from.
  exported?: { path: string; blob: Blob; source: { tab: string; url: string } };
  /// A video or audio tab's element, and where it stood when its view went.
  player: HTMLMediaElement | null;
  playerAt: number;
  image?: { view: ImageView; w: number; h: number };
  model?: ModelView | null;
  wire?: boolean;
  bytes?: ArrayBuffer;
  /// A text tab's path in the worktree it was opened from, that worktree's
  /// folder, and what was read of it.
  rel?: string;
  root?: string;
  text?: TreeText;
  /// The line a link pointed at, marked and scrolled to in the text view.
  line?: number | null;
  /// Increments on explicit jumps so jumping to the same line scrolls again.
  lineJump?: number;
  /// Pending edit cursor target from go-to-definition, applied once text loads.
  editAt?: TextPlace | null;
  /// Where else to look, in order, when the file is not where it was
  /// looked for: the session's folder, then the top of its checkout.
  elsewhere?: WorkFile[];
  /// Its HTML preview opens as soon as its text is read.
  previewQueued?: boolean;
  preview?: { active: boolean; loading: boolean; src: string; html: string; error: string };
  edit?: FileEdit | null;
  /// Positive while file search is open. Each Find shortcut refocuses the field.
  findRequest?: number;
  /// Reads that failed in a row, and the timer of the next, while its file
  /// may still come.
  tries?: number;
  retry?: ReturnType<typeof setTimeout>;
  /// A terminal tab's terminal, once it started.
  term?: PaneTerm;
  /// Where a new terminal tab starts its shell (its machine and folder stay
  /// those when the pane's session changes), or the terminal already
  /// running that the tab follows.
  at?: Omit<TermPlace, "name">;
  existing?: TerminalRow;
  /// Its tab was closed: the terminal is being stopped, or is stopped once
  /// it started.
  closed?: boolean;
  /// Another coding agent session shown live in this tab.
  splitSession?: SplitSession | null;
}

/// The session the pane belongs to.
export interface PaneSession extends Partial<Pick<AgentSession, "assistant" | "reviewer" | "parent">> {
  id: string;
  machine_id: string;
  machine_name: string;
  cwd: string;
  agent: string;
  external?: boolean;
  /// The project it belongs to, which new terminals are listed under.
  project?: string;
  /// Sends a review of the changes to the session: "" when it went, else why not.
  sendReview?: (text: string) => Promise<string>;
}

/// A file as a folder the node reads and a path inside it.
export interface WorkFile {
  root: string;
  rel: string;
}

/// How far a tab moves under the pointer before it drags.
export const DRAG_START = 5;

export const P = {
  session: null as PaneSession | null,
  /// Tabs moved to a dock whose move is not kept (a picture by its data URL).
  moved: new Map<string, Dock>(),
  watch: null as ResizeObserver | null,
  /// A tab drag just ended: the click that follows it does nothing.
  dragged: false,
  /// The user opened a tab while Assistant's right pane was hidden for lack of content.
  rightRequested: false,
};

/// The session the pane shows, while one does.
export function paneSession(): Readonly<PaneSession> | null {
  return P.session;
}

// ----- What the tabs ask of the pane -----

/// Counts the asks to draw the pane again, and to follow its files again; the
/// owner of each, media-draw.ts and reload.ts, hears them.
export const paneAsks = createStore(() => ({ draws: 0, follows: 0 }));

/// Asks the pane to draw again.
export function redraw() {
  paneAsks.setState(s => ({ draws: s.draws + 1 }));
}

/// Asks live reload to look at the files the pane shows now.
export function refollow() {
  paneAsks.setState(s => ({ follows: s.follows + 1 }));
}
