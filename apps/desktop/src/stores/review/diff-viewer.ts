// The diff viewer's state: what it shows, in a zustand store the React view
// (diff-view.tsx, diff-files.tsx) renders from, and the few elements and the
// TanStack virtualizer the view hands back, which scrolling and the keyboard
// (diff-keys.ts) act on. diff.ts makes one and loads it; diff-comments.ts
// edits its review comments.
import { flushSync } from "react-dom";
import { createStore, type StoreApi } from "zustand";
import type { Virtualizer } from "@tanstack/react-virtual";
import { latest } from "../../ui/active.ts";
import { readView, writeView } from "../agentpane/view-memory.ts";
import type { Feedback } from "../../parts/review/feedback.ts";
import { loadDraft } from "./draftstore.ts";
import { filterHits, matchesFilter, placeComments, sameDiffFile, type DiffFile, type DiffView, type Extent, type Hit, type ReviewComment } from "./diff-state.ts";
import type { DiffSource } from "../../parts/review/diff.tsx";

/// Wider than this, the file list can show on the left.
export const WIDE = 900;
/// At this width or less the lines stay unified, as the CSS hides the button.
export const NARROW = 640;

/// Rows per chunk, how far past the viewport chunks stay drawn, and the width
/// of one character of the code font.
export const CHUNK = 25;
export const MARGIN = 500;
export const CH = 7.2;
/// The space between two files, counted in each file's laid out size.
export const GAP = 10;

export interface DiffState {
  view: DiffView | null;
  /// Every file the list lays out, in order, as its section shows it: a file
  /// held for a selection or an open comment keeps the version drawn, and one
  /// the view dropped stays in place while it is held. Files the filter hides
  /// stay in it with no height, so a hidden open editor keeps its place.
  list: readonly DiffFile[];
  loading: boolean;
  error: string;
  sending: boolean;
  /// A short line in the header, for two seconds.
  flash: string;
  filter: string;
  /// The place the filter stepped to, marked apart from the others.
  hit: Hit | null;
  /// Side by side is chosen; it shows only while the viewer is not narrow.
  split: boolean;
  /// The file list is open; it shows only while the viewer is wide, and in a
  /// side pane only while ⇧esc has it fill the session.
  files: boolean;
  narrow: boolean;
  /// Only the uncommitted changes, against HEAD.
  uncommitted: boolean;
  /// Folded or not, as the user left each file while the view is open.
  folded: ReadonlyMap<string, boolean>;
  closedFolders: ReadonlySet<string>;
  filteredClosedFolders: ReadonlySet<string>;
  /// Files being read in full or with more context.
  pending: ReadonlySet<string>;
  /// The file j, k and x act on: the one stepped to while it shows, else the one at the top.
  cur: string | null;
  /// Changes when the list should scroll the current file into view.
  revealTree: number;
  /// The review comments waiting to be sent.
  draft: readonly ReviewComment[];
  /// The comments with an open editor; true for a new one not added yet.
  editors: ReadonlyMap<ReviewComment, boolean>;
  /// Editors whose line left the diff, shown with the lost comments as they were.
  orphaned: ReadonlySet<ReviewComment>;
}

/// An editor's text and selection as it unmounted, for where it mounts next.
interface EditorSnapshot { text: string; start: number; end: number; focused: boolean }

/// Where each shown file sits in the laid out list `files` (diff-keys.ts).
interface ShownLayout {
  shown: readonly DiffFile[];
  files: readonly DiffFile[];
  at: readonly number[];
}

/// A drawn chunk of rows: whether its rows show, and how to change that.
interface ChunkEntry { live: boolean; set: (state: { live: boolean; height: number }) => void }

export class Viewer {
  readonly src: DiffSource;
  /// The element the view renders into, which the caller places.
  readonly root = document.createElement("div");
  readonly store: StoreApi<DiffState>;
  disposed = false;
  /// Counts loads: a file read that a later load replaced is dropped.
  seq = 0;
  readonly reads = latest();
  /// Lines of context shown around each hunk of an expanded file.
  readonly context = new Map<string, { original: DiffFile; extents: Extent[] }>();
  readonly snapshots = new Map<ReviewComment, EditorSnapshot>();
  feedback: Feedback | null = null;
  flashTimer = 0;

  readonly memoryKey: string;
  memory: unknown;
  restoredScroll = 0;
  rememberTimer = 0;

  /// What the view hands back as it mounts: the scrolling list, the filter,
  /// the drawn sections by path, their chunks and the virtualizer.
  files: HTMLElement | null = null;
  find: HTMLInputElement | null = null;
  readonly sections = new Map<string, HTMLElement>();
  readonly chunks = new Map<Element, ChunkEntry>();
  chunkObserver: IntersectionObserver | null = null;
  virtual: Virtualizer<HTMLDivElement, HTMLElement> | null = null;
  layout: ShownLayout | null = null;
  /// Paths held for a selection, released as it goes.
  readonly selectionHeld = new Set<string>();
  readonly onSelection = () => selectionChanged(this);

  constructor(src: DiffSource) {
    this.src = src;
    this.memoryKey = `${src.review.key}:diff`;
    this.memory = readView(this.memoryKey);
    this.root.className = "dv";
    this.root.tabIndex = -1;
    const memory = this.memory;
    let filter = "";
    const folded = new Map<string, boolean>();

    if (memory && typeof memory === "object") {
      if ("scroll" in memory && typeof memory.scroll === "number") this.restoredScroll = memory.scroll;
      if ("filter" in memory && typeof memory.filter === "string") filter = memory.filter;
      if ("folded" in memory && Array.isArray(memory.folded)) for (const pair of memory.folded) {
        if (Array.isArray(pair) && typeof pair[0] === "string" && typeof pair[1] === "boolean") folded.set(pair[0], pair[1]);
      }
    }

    this.store = createStore<DiffState>(() => ({
      view: null, list: [], loading: false, error: "", sending: false, flash: "", filter, hit: null, split: loadFlag(SPLIT_KEY, false), files: loadFlag(FILES_KEY, true), narrow: false,
      uncommitted: false, folded, closedFolders: new Set(), filteredClosedFolders: new Set(), pending: new Set(), cur: null, revealTree: 0,
      draft: src.readOnly ? [] : loadDraft(src.review.key), editors: new Map(), orphaned: new Set(),
    }));
  }

  get state() { return this.store.getState(); }

  /// Changes the state and renders it before returning, for callers that
  /// measure or scroll the result.
  set(change: Partial<DiffState>) {
    if (this.disposed) { this.store.setState(change); return; }
    flushSync(() => this.store.setState(change));
  }
}

/// Keeps the scroll, filter, folds and context for the next time the diff opens.
export function remember(v: Viewer) {
  clearTimeout(v.rememberTimer);
  v.rememberTimer = 0;
  const s = v.state;
  writeView(v.memoryKey, { scroll: v.files?.scrollTop ?? 0, filter: s.filter, folded: [...s.folded], context: [...v.context].map(([path, value]) => [path, value.extents.map(extent => [extent.above, extent.below])]) });
}

/// The files the filter lets through, kept per view and filter: the toolbar
/// and scrolling ask on every change.
export function shown(s: Pick<DiffState, "view" | "filter">): readonly DiffFile[] {
  if (!s.view) return [];
  const kept = filtered.get(s.view);
  if (kept?.filter === s.filter) return kept.files;
  const files = s.view.files.filter(f => matchesFilter(f, s.filter));
  filtered.set(s.view, { filter: s.filter, files });
  return files;
}

const filtered = new WeakMap<DiffView, { filter: string; files: readonly DiffFile[] }>();

/// Every place in the shown files holding the filter, kept like [`shown`].
export function hits(s: Pick<DiffState, "view" | "filter">): readonly Hit[] {
  if (!s.view) return [];
  const kept = found.get(s.view);
  if (kept?.filter === s.filter) return kept.hits;
  const all = filterHits(shown(s), s.filter);
  found.set(s.view, { filter: s.filter, hits: all });
  return all;
}

const found = new WeakMap<DiffView, { filter: string; hits: readonly Hit[] }>();

/// Whether the user's selection reaches into `box`: emptying it would lose the text.
export function selected(box: Node) {
  if (!anySelection()) return false;
  const sel = document.getSelection();
  return !!sel && sel.containsNode(box, true);
}

/// Whether the document holds a selection, known again after each
/// `selectionchange`: asking the selection lays the page out, and scrolling
/// asks for every drawn file.
let selection: "unknown" | "none" | "some" = "unknown";
let watching = false;

function anySelection() {
  if (!watching) {
    watching = true;
    document.addEventListener("selectionchange", () => { selection = "unknown"; });
  }

  if (selection === "unknown") {
    const sel = document.getSelection();
    selection = sel && !sel.isCollapsed ? "some" : "none";
  }

  return selection === "some";
}

/// Brings the laid out files up to the view. A file whose section holds a
/// selection, or an open comment still on a line it has, keeps the version
/// drawn until they go; an open comment whose line went joins the lost ones.
/// `force` names files drawn again even when they read the same.
export function reconcile(v: Viewer, force: ReadonlySet<string> = new Set()) {
  const { view, list: before, editors, orphaned } = v.state;
  if (!view) return;
  const old = new Map(before.map(file => [file.path, file]));
  const incoming = new Map(view.files.map(file => [file.path, file]));
  const orphans = new Set(orphaned);
  v.selectionHeld.clear();

  const decide = (path: string, next: DiffFile | null): DiffFile | null => {
    const prev = old.get(path);
    if (!prev) return next;
    if (next && !force.has(path) && sameDiffFile(prev, next)) return prev;
    const section = v.sections.get(path);

    if (section && selected(section)) {
      v.selectionHeld.add(path);
      return prev;
    }

    const open = [...editors.keys()].filter(comment => comment.path === path && !orphans.has(comment));
    if (next && open.some(comment => anchored(next, comment))) return prev;
    for (const comment of open) orphans.add(comment);
    return next;
  };

  const list: DiffFile[] = [];
  for (const file of view.files) list.push(decide(file.path, file) ?? file);

  before.forEach((file, index) => {
    if (incoming.has(file.path)) return;
    const kept = decide(file.path, null);
    if (kept) list.splice(Math.min(index, list.length), 0, kept);
  });

  const same = list.length === before.length && list.every((file, index) => file === before[index]);
  const change: Partial<DiffState> = {};
  if (!same) change.list = list;
  if (orphans.size !== orphaned.size) change.orphaned = orphans;
  if (change.list || change.orphaned) v.set(change);
  if (v.selectionHeld.size) document.addEventListener("selectionchange", v.onSelection);
  else document.removeEventListener("selectionchange", v.onSelection);
}

/// Whether `comment` still has its place in `file`.
function anchored(file: DiffFile, comment: ReviewComment) {
  return comment.line === null || !!placeComments([file], [comment]).files.get(file.path)?.lines.size;
}

/// A file held for a selection is drawn again once the selection leaves it.
function selectionChanged(v: Viewer) {
  if (!v.root.isConnected || v.disposed) {
    v.selectionHeld.clear();
    document.removeEventListener("selectionchange", v.onSelection);
    return;
  }

  for (const path of v.selectionHeld) {
    const section = v.sections.get(path);
    if (section && selected(section)) continue;
    const top = v.files?.scrollTop ?? 0;
    reconcile(v);
    if (v.files && v.files.scrollTop !== top) v.files.scrollTop = top;
    return;
  }
}

/// The laid out box of the file at `index`.
export function itemAt(v: Viewer, index: number | undefined) {
  if (index === undefined || index < 0 || !v.virtual) return null;
  v.virtual.getVirtualItems();
  return v.virtual.measurementsCache[index] ?? null;
}

export function indexOf(v: Viewer, path: string | null) {
  if (path === null) return -1;
  const list = v.state.list;
  let at = positions.get(list);
  if (!at) positions.set(list, at = new Map(list.map((file, index) => [file.path, index])));
  return at.get(path) ?? -1;
}

/// Each laid out list's index by path, as scrolling looks files up on every frame.
const positions = new WeakMap<readonly DiffFile[], ReadonlyMap<string, number>>();

/// Lays out every file again from estimates, measuring the drawn ones now.
export function remeasure(v: Viewer) {
  const virtual = v.virtual;
  if (!virtual) return;
  virtual.measure();
  for (const section of v.sections.values()) virtual.measureElement(section);
}

/// Keeps the current file where it is on screen through `run`.
export function keepCurrent(v: Viewer, run: () => void) {
  const before = itemAt(v, indexOf(v, v.state.cur));
  const offset = before && v.files ? v.files.scrollTop - before.start : 0;
  run();
  const after = itemAt(v, indexOf(v, v.state.cur));
  if (before && after && v.files && after.start !== before.start) v.files.scrollTop = after.start + offset;
}

/// Shows `text` in the header for two seconds.
export function say(v: Viewer, text: string) {
  if (v.disposed) return;
  v.store.setState({ flash: text });
  clearTimeout(v.flashTimer);
  v.flashTimer = window.setTimeout(() => v.store.setState({ flash: "" }), 2000);
}

export const SPLIT_KEY = "beep.diffSplit";
export const FILES_KEY = "beep.diffFiles";

function loadFlag(key: string, fallback: boolean) {
  try {
    const saved = localStorage.getItem(key);
    return saved === null ? fallback : saved === "1";
  } catch { return fallback; }
}

export function saveFlag(key: string, on: boolean) {
  try { localStorage.setItem(key, on ? "1" : "0"); } catch { /* kept for this launch only */ }
}

export function reasonOf(e: unknown) {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String(e.message);
  return String(e);
}
