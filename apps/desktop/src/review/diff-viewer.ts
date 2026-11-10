// The diff viewer's state: what it shows, the elements it draws into and the
// TanStack virtualizer that lays its files out. diff.ts makes one; the
// drawing (diff-render.ts), the comment editors (diff-comments.ts) and the
// keyboard (diff-keys.ts) act on it.
import type { ReactNode } from "react";
import { Virtualizer, type Range } from "@tanstack/react-virtual";
import { latest } from "../ui/active.ts";
import { el } from "../ui/dom.ts";
import { readView, writeView } from "../agentpane/view-memory.ts";
import type { Feedback } from "./feedback.ts";
import { loadDraft } from "./draftstore.ts";
import { matchesFilter, placeComments, type DiffFile, type DiffRow, type DiffView, type Placed, type ReviewComment, type SplitRow } from "./diff-state.ts";
import type { DiffSlot, DiffSource } from "./diff.ts";
import { keepHeld, layoutOptions, onChunks, selectionChanged } from "./diff-render.ts";

/// Wider than this, the file list shows on the left.
export const WIDE = 900;
/// At this width or less the lines stay unified, as the CSS hides the button.
export const NARROW = 640;

/// Rows per chunk, how far past the viewport chunks stay drawn, and the width
/// of one character of the code font.
export const CHUNK = 25;
const MARGIN = 500;
export const CH = 7.2;
/// The space between two files, counted in each file's laid out size.
export const GAP = 10;

/// A run of rows, drawn only while it is near the viewport.
interface Chunk {
  box: HTMLElement;
  id: number;
  path: string;
  rows: (DiffRow | SplitRow)[];
  comments: ReadonlyMap<string, readonly ReviewComment[]>;
  live: boolean;
}

export const noComments: ReadonlyMap<string, readonly ReviewComment[]> = new Map();

/// An open comment editor whose line left the diff, to draw again as it was.
export interface EditorRestore { text: string; focused: boolean; start: number; end: number; isNew: boolean }

/// A drawn comment: one of the draft, or a new one not added yet.
interface CommentEntry { id: number; comment: ReviewComment; label: string; isNew: boolean; chunkOwned: boolean; restored: EditorRestore | null }

export class Viewer {
  readonly src: DiffSource;
  readonly root = el("div", "dv");
  disposed = false;

  view: DiffView | null = null;
  error = "";
  loading = false;
  /// Folded or not, as the user left each file while the view is open.
  readonly folded = new Map<string, boolean>();
  readonly closedFolders = new Set<string>();
  readonly filteredClosedFolders = new Set<string>();
  filter = "";
  /// Side by side is chosen; it shows only while the viewer is not narrow.
  split = loadSplit();
  narrow = false;
  /// Only the uncommitted changes, against HEAD.
  uncommitted = false;
  /// Files being read in full.
  readonly pending = new Set<string>();
  /// The file j, k and x act on: the one stepped to while it shows,
  /// else the one at the top.
  cur: string | null = null;
  markedPath: string | null = null;
  revealTree = 0;
  /// Counts loads: a file read that a later load replaced is dropped.
  seq = 0;
  readonly reads = latest();
  /// Lines of context shown around each expanded file.
  readonly context = new Map<string, { original: DiffFile; above: number; below: number }>();

  /// The review comments waiting to be sent.
  draft: ReviewComment[];
  sending = false;
  placedCache: Placed | null = null;
  readonly orphaned = new Map<ReviewComment, EditorRestore | null>();
  revealOrphan = false;
  readonly comments = new Map<HTMLElement, CommentEntry>();
  nextComment = 0;
  readonly lostComments = new Map<ReviewComment, HTMLElement>();
  readonly lostHeading = el("div", "dv-lh", "On files or lines no longer in the changes");
  readonly lostBox = el("div", "dv-lost");

  /// The sections drawn now, by path: the files near the viewport and any
  /// holding focus, a selection or an open comment.
  readonly sections = new Map<string, HTMLElement>();
  /// What each drawn section shows, which a deferred one keeps until it is free.
  readonly renderedFiles = new Map<string, DiffFile>();
  hasRenderedView = false;
  /// Every file the list lays out, in order: the view's files and any the view
  /// dropped whose section is kept for a selection. Files the filter hides
  /// stay in it with no height, so a hidden open editor keeps its place.
  list: DiffFile[] = [];
  indexOf = new Map<string, number>();
  laidOutFilter = "";
  readonly deferredFiles = new Set<string>();
  readonly selectionDeferred = new Set<string>();
  watchingSelection = false;
  readonly bodyModes = new Map<string, boolean>();
  readonly headers = new Map<string, { host: HTMLElement; file: DiffFile; comments: HTMLElement }>();
  readonly bodyControls = new Map<HTMLElement, { id: number; view: ReactNode }>();
  nextBodyControl = 0;
  bodyControlsDirty = false;
  /// The chunks of rows of every drawn file.
  readonly chunks = new Map<Element, Chunk>();
  readonly liveChunks = new Set<Chunk>();
  nextChunk = 0;

  readonly head = el("div", "dv-head");
  flashText = "";
  flashTimer = 0;
  feedback: Feedback = { el: el("span"), note: el("span"), redraw() {}, dispose() {} };
  readonly errorHost = contents();
  readonly messageHost = contents();
  readonly moreHost = contents();
  readonly matchNote = el("div", "dv-more");
  readonly tree = el("nav", "dv-tree");
  readonly files = el("div", "dv-files");
  /// The box the drawn sections are placed in, as tall as the whole list.
  readonly win = el("div", "dv-win");
  readonly grip = el("div", "dv-resize");
  readonly body = el("div", "dv-body", this.tree, this.grip, this.files);
  readonly keys = el("div", "dv-keys");

  readonly memoryKey: string;
  memory: unknown;
  restoredScroll = 0;
  rememberTimer = 0;
  scrollFrame = 0;

  readonly holdRange = (range: Range) => keepHeld(this, range);
  readonly virtual = new Virtualizer<HTMLElement, HTMLElement>(layoutOptions(this));
  placing = false;
  placeAgain = false;
  readonly chunkObserver = new IntersectionObserver(entries => onChunks(this, entries), { root: this.files, rootMargin: `${MARGIN}px 0px` });
  readonly onSelection = () => selectionChanged(this);
  /// Chunks mount and unmount their saved comments; one function each, so
  /// the memoized chunks are not drawn again.
  readonly mountComment = (host: HTMLElement, comment: ReviewComment) => {
    this.comments.set(host, { id: ++this.nextComment, comment, label: "", isNew: false, chunkOwned: true, restored: null });
  };
  readonly unmountComment = (host: HTMLElement) => { this.comments.delete(host); };

  constructor(src: DiffSource) {
    this.src = src;
    this.draft = src.readOnly ? [] : loadDraft(src.review.key);
    this.memoryKey = `${src.review.key}:diff`;
    this.memory = readView(this.memoryKey);
    this.root.tabIndex = -1;
    this.tree.setAttribute("aria-label", "Changed files");
    this.files.style.position = "relative";
    // Only a file wholly above the viewport moves the scroll as it is measured;
    // the chunk observer keeps the file at the top steady itself.
    this.virtual.shouldAdjustScrollPositionOnItemSizeChange = item => item.end <= this.files.scrollTop;

    const memory = this.memory;
    if (!memory || typeof memory !== "object") return;
    if ("scroll" in memory && typeof memory.scroll === "number") this.restoredScroll = memory.scroll;
    if ("filter" in memory && typeof memory.filter === "string") this.filter = memory.filter;
    if ("folded" in memory && Array.isArray(memory.folded)) for (const pair of memory.folded) {
      if (Array.isArray(pair) && typeof pair[0] === "string" && typeof pair[1] === "boolean") this.folded.set(pair[0], pair[1]);
    }
  }
}

function contents() {
  const host = el("div");
  host.style.display = "contents";
  return host;
}

export function render(v: Viewer, slot: DiffSlot, content: ReactNode) {
  if (!v.disposed) v.src.owner.set(slot, content);
}

/// Keeps the scroll, filter, folds and context for the next time the diff opens.
export function remember(v: Viewer) {
  clearTimeout(v.rememberTimer);
  v.rememberTimer = 0;
  writeView(v.memoryKey, { scroll: v.files.scrollTop, filter: v.filter, folded: [...v.folded], context: [...v.context].map(([path, value]) => [path, value.above, value.below]) });
}

/// Where the draft's comments go, with any orphaned editors among the lost.
export function placed(v: Viewer) {
  if (v.placedCache) return v.placedCache;
  const out = placeComments(v.view?.files ?? [], v.draft.filter(comment => !v.orphaned.has(comment)));
  out.lost.push(...v.orphaned.keys());
  return v.placedCache = out;
}

export function shown(v: Viewer): DiffFile[] {
  return v.view ? v.view.files.filter(f => matchesFilter(f, v.filter)) : [];
}

/// Whether the user's selection reaches into `box`: emptying it would lose the text.
export function selected(box: HTMLElement) {
  const sel = document.getSelection();
  return !!sel && !sel.isCollapsed && sel.containsNode(box, true);
}

const SPLIT_KEY = "beep.diffSplit";

function loadSplit() {
  try { return localStorage.getItem(SPLIT_KEY) === "1"; } catch { return false; }
}

export function saveSplit(on: boolean) {
  try { localStorage.setItem(SPLIT_KEY, on ? "1" : "0"); } catch { /* kept for this launch only */ }
}

export function reasonOf(e: unknown) {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
