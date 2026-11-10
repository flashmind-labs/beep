import { createElement, Fragment, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Virtualizer, defaultRangeExtractor, elementScroll, observeElementOffset, observeElementRect, type Range, type VirtualizerOptions } from "@tanstack/react-virtual";
import { DiffToolbar } from "./diff-toolbar.tsx";
import { DiffTree } from "./diff-tree.tsx";
import { DiffFileHeader } from "./diff-file-header.tsx";
import { DiffContext, DiffOmitted } from "./diff-body-controls.tsx";
import { DiffChunk } from "./diff-chunk.tsx";
import { DiffComment } from "./diff-comment.tsx";
import { DiffKeys, DiffMessage, DiffMore } from "./diff-status.tsx";
import { readView, writeView } from "../agentpane/view-memory.ts";
// The diff viewer: a one-line header with the counts, a filter, icon toggles
// and the review buttons; a list of the files at wide widths; each file folding under
// its sticky header, its lines in a unified view that scrolls sideways on its
// own. Only the files near the viewport have sections, laid out by a
// TanStack virtualizer; within a section, only nearby chunks of rows are drawn.
import {
  type DiffFile, type DiffRow, type DiffView, type ReviewComment, type SplitRow, allFolded, currentFile, contextFile, diffRows, estimateHeight, isFolded, leftOutNote, sameDiffFile,
  filesInTreeOrder, inView, lineLabel, matchesFilter, numberWidth, placeComments, reviewText, splitHeight, splitRows, stepFile, withLoaded,
} from "./diff-state.ts";
import { initTreeResize } from "./treewidth.ts";
import { loadDraft, saveDraft } from "./draftstore.ts";
import { feedbackButton } from "./feedback.ts";
import { mergeComments } from "./feedback-state.ts";
import { el } from "../ui/dom.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";

/// Where a diff comes from and what its files can open in.
export type DiffSlot = "toolbar" | "status" | "tree" | "chunks" | "headers" | "body-controls" | "comments" | "feedback";
export interface DiffOwner { set(slot: DiffSlot, content: ReactNode): void; dispose(): void }

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
  reload: () => void;
  focus: () => void;
  dispose: () => void;
}

/// Wider than this, the file list shows on the left.
const WIDE = 900;
/// At this width or less the lines stay unified, as the CSS hides the button.
const NARROW = 640;

/// Rows per chunk, how far past the viewport chunks stay drawn, and the width
/// of one character of the code font.
const CHUNK = 25;
const MARGIN = 500;
const CH = 7.2;
/// The space between two files, counted in each file's laid out size.
const GAP = 10;

/// A run of rows, drawn only while it is near the viewport.
interface Chunk {
  box: HTMLElement;
  id: number;
  path: string;
  rows: (DiffRow | SplitRow)[];
  comments: ReadonlyMap<string, readonly ReviewComment[]>;
  live: boolean;
}

const SPLIT_KEY = "beep.diffSplit";

function loadSplit() {
  try { return localStorage.getItem(SPLIT_KEY) === "1"; } catch { return false; }
}

function saveSplit(on: boolean) {
  try { localStorage.setItem(SPLIT_KEY, on ? "1" : "0"); } catch { /* kept for this launch only */ }
}

function feedbackLoad(load: DiffSource["load"]) {
  return async (uncommitted: boolean) => (await load([], false, uncommitted)).files;
}

export function diffView(src: DiffSource): DiffViewHandle {
  const root = el("div", "dv");
  let disposed = false;
  const render = (slot: DiffSlot, content: ReactNode) => { if (!disposed) src.owner.set(slot, content); };
  root.tabIndex = -1;
  const st = {
    view: null as DiffView | null,
    error: "",
    loading: false,
    /// Folded or not, as the user left each file while the view is open.
    folded: new Map<string, boolean>(),
    closedFolders: new Set<string>(),
    filter: "",
    /// Side by side is chosen; it shows only while the viewer is not narrow.
    split: loadSplit(),
    narrow: false,
    /// Only the uncommitted changes, against HEAD.
    uncommitted: false,
    /// Files being read in full.
    pending: new Set<string>(),
    /// The file j, k and x act on: the one stepped to while it shows,
    /// else the one at the top.
    cur: null as string | null,
    seq: 0,
    /// The review comments waiting to be sent.
    draft: src.readOnly ? [] : loadDraft(src.review.key),
    sending: false,
  };
  type EditorRestore = { text: string; focused: boolean; start: number; end: number; isNew: boolean };
  const orphaned = new Map<ReviewComment, EditorRestore | null>();
  let revealOrphan = false;
  let placedCache: ReturnType<typeof placeComments> | null = null;
  const placed = () => {
    if (placedCache) return placedCache;
    const out = placeComments(st.view?.files ?? [], st.draft.filter(comment => !orphaned.has(comment)));
    out.lost.push(...orphaned.keys());
    return placedCache = out;
  };
  /// The sections drawn now, by path: the files near the viewport and any
  /// holding focus, a selection or an open comment.
  const sections = new Map<string, HTMLElement>();
  /// What each drawn section shows, which a deferred one keeps until it is free.
  const renderedFiles = new Map<string, DiffFile>();
  let hasRenderedView = false;
  /// Every file the list lays out, in order: the view's files and any the view
  /// dropped whose section is kept for a selection. Files the filter hides
  /// stay in it with no height, so a hidden open editor keeps its place.
  let list: DiffFile[] = [];
  let indexOf = new Map<string, number>();
  let laidOutFilter = "";
  /// The chunks of rows of every drawn file.
  const chunks = new Map<Element, Chunk>();
  const liveChunks = new Set<Chunk>();
  const noComments = new Map<string, readonly ReviewComment[]>();
  let nextChunk = 0;
  const comments = new Map<HTMLElement, { id: number; comment: ReviewComment; label: string; isNew: boolean; chunkOwned: boolean; restored: EditorRestore | null }>();
  const lostComments = new Map<ReviewComment, HTMLElement>();
  const lostHeading = el("div", "dv-lh", "On files or lines no longer in the changes");
  let nextComment = 0;
  const mountLineComment = (host: HTMLElement, comment: ReviewComment) => {
    comments.set(host, { id: ++nextComment, comment, label: "", isNew: false, chunkOwned: true, restored: null });
  };
  const unmountLineComment = (host: HTMLElement) => { comments.delete(host); };
  const headers = new Map<string, { host: HTMLElement; file: DiffFile; comments: HTMLElement }>();
  const bodyControls = new Map<HTMLElement, { id: number; view: ReactNode }>();
  let nextBodyControl = 0;
  let bodyControlsDirty = false;
  const deferredFiles = new Set<string>();
  const selectionDeferred = new Set<string>();
  let watchingSelection = false;

  function selectionChanged() {
    if (!root.isConnected) {
      selectionDeferred.clear();
      document.removeEventListener("selectionchange", selectionChanged);
      watchingSelection = false;
      return;
    }
    let ready = false;
    for (const path of selectionDeferred) {
      const section = sections.get(path);
      if (section && selected(section)) continue;
      selectionDeferred.delete(path);
      deferredFiles.delete(path);
      renderedFiles.delete(path);
      ready = true;
    }
    if (!selectionDeferred.size) {
      document.removeEventListener("selectionchange", selectionChanged);
      watchingSelection = false;
    }
    if (ready) {
      const top = files.scrollTop;
      if (reconcileFiles() && files.scrollTop !== top) files.scrollTop = top;
    }
  }
  const errorHost = el("div"), messageHost = el("div"), moreHost = el("div");
  errorHost.style.display = messageHost.style.display = moreHost.style.display = "contents";
  const bodyModes = new Map<string, boolean>();
  const matchNote = el("div", "dv-more");
  const head = el("div", "dv-head");
  let flashText = "";
  let flashTimer = 0;
  const say = (text: string) => {
    if (disposed) return;
    flashText = text;
    drawToolbar();
    clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => { flashText = ""; drawToolbar(); }, 2000);
  };
  const lostBox = el("div", "dv-lost");
  const feedback = src.readOnly ? { el: el("span"), note: el("span"), redraw() {}, dispose() {} } : feedbackButton({
    key: src.review.key,
    machine: src.agent?.machine ?? null,
    prefer: src.agent?.prefer,
    session: src.agent?.session,
    view: () => st.view,
    uncommitted: () => st.uncommitted,
    load: feedbackLoad(src.load),
    add: addAgentComments,
    bounds: () => root,
    say,
  }, view => render("feedback", view));
  const tree = el("nav", "dv-tree");
  tree.setAttribute("aria-label", "Changed files");
  let revealTree = 0;
  const filteredClosedFolders = new Set<string>();
  const files = el("div", "dv-files");
  /// The box the drawn sections are placed in, as tall as the whole list.
  const win = el("div", "dv-win");
  const memoryKey = `${src.review.key}:diff`;
  let memory = readView(memoryKey);
  let restoredScroll = 0;
  if (memory && typeof memory === "object") {
    if ("scroll" in memory && typeof memory.scroll === "number") restoredScroll = memory.scroll;
    if ("filter" in memory && typeof memory.filter === "string") st.filter = memory.filter;
    if ("folded" in memory && Array.isArray(memory.folded)) for (const pair of memory.folded) {
      if (Array.isArray(pair) && typeof pair[0] === "string" && typeof pair[1] === "boolean") st.folded.set(pair[0], pair[1]);
    }
  }
  const remember = () => {
    clearTimeout(rememberTimer);
    rememberTimer = 0;
    writeView(memoryKey, { scroll: files.scrollTop, filter: st.filter, folded: [...st.folded], context: [...context].map(([path, value]) => [path, value.above, value.below]) });
  };
  // Scrolling writes the position at most a few times a second.
  let rememberTimer = 0;
  files.addEventListener("scroll", () => { if (!rememberTimer) rememberTimer = window.setTimeout(remember, 250); }, { passive: true });

  files.style.position = "relative";
  const virtual = new Virtualizer<HTMLElement, HTMLElement>(layoutOptions());
  // Only a file wholly above the viewport moves the scroll as it is measured;
  // the chunk observer keeps the file at the top steady itself.
  virtual.shouldAdjustScrollPositionOnItemSizeChange = item => item.end <= files.scrollTop;
  const stopVirtual = virtual._didMount();
  let placing = false, placeAgain = false;

  function layoutOptions(): VirtualizerOptions<HTMLElement, HTMLElement> {
    const laidOut = list;
    return {
      count: laidOut.length,
      getScrollElement: () => files,
      estimateSize: index => sizeOf(laidOut[index]),
      getItemKey: index => laidOut[index]?.path ?? index,
      measureElement: (node, entry) => node.hidden ? 0 : Math.round(entry?.borderBoxSize?.[0]?.blockSize ?? node.offsetHeight) + GAP,
      rangeExtractor: keepHeld,
      overscan: 2,
      paddingStart: GAP,
      scrollPaddingStart: GAP,
      scrollMargin: win.offsetTop,
      useAnimationFrameWithResizeObserver: true,
      observeElementRect, observeElementOffset, scrollToFn: elementScroll,
      onChange: () => placeFiles(),
    };
  }

  /// A file's height before it is measured, with the gap after it.
  function sizeOf(f: DiffFile | undefined) {
    return f && matchesFilter(f, st.filter) ? estimateHeight(f, isFolded(f, st.folded)) + GAP : 0;
  }

  /// The files near the viewport, and the drawn ones that must stay: the
  /// focus, a selection or an open comment would be lost with them.
  function keepHeld(range: Range) {
    const out = defaultRangeExtractor(range);
    const extra: number[] = [];
    for (const [path, section] of sections) {
      const index = indexOf.get(path);
      if (index === undefined || (index >= out[0] && index <= out[out.length - 1])) continue;
      if (deferredFiles.has(path) || section.contains(document.activeElement) || section.querySelector(".dv-cmt.open") || selected(section)) extra.push(index);
    }
    return extra.length ? [...new Set([...out, ...extra])].sort((a, b) => a - b) : out;
  }

  /// Sets what the list lays out, keeping measured sizes by path.
  function relist(next: DiffFile[]) {
    list = next;
    indexOf = new Map(next.map((f, i) => [f.path, i]));
    virtual.setOptions(layoutOptions());
  }

  /// Positions the drawn sections and adds or drops sections as files come
  /// near the viewport or leave it.
  function placeFiles() {
    if (placing) { placeAgain = true; return; }
    placing = true;
    try {
      for (let pass = 0; pass < 4; pass++) {
        placeAgain = false;
        placeOnce();
        virtual._willUpdate();
        if (!placeAgain) break;
      }
    } finally { placing = false; }
  }

  function placeOnce() {
    if (win.isConnected && virtual.options.scrollMargin !== win.offsetTop) virtual.setOptions(layoutOptions());
    const items = virtual.getVirtualItems();
    const height = `${virtual.getTotalSize()}px`;
    if (win.style.height !== height) win.style.height = height;
    const wanted = new Set(items.map(item => list[item.index].path));
    let changed = false;

    for (const [path, section] of sections) {
      if (wanted.has(path)) continue;
      dropSection(path, section);
      changed = true;
    }

    const added: HTMLElement[] = [];
    const ordered: HTMLElement[] = [];

    for (const item of items) {
      const f = list[item.index];
      let section = sections.get(f.path);

      if (!section) {
        section = fileSection(f);
        sections.set(f.path, section);
        renderedFiles.set(f.path, f);
        added.push(section);
      }

      section.dataset.index = String(item.index);
      const top = `${item.start - virtual.options.scrollMargin}px`;
      if (section.style.top !== top) section.style.top = top;
      ordered.push(section);
    }

    orderSections(ordered);

    if (added.length || changed) {
      pruneChunks();
      drawHeaders();
      drawComments();
      markedPath = null;
      markCurrent(false);
    }

    for (const section of added) virtual.measureElement(section);
  }

  /// Puts the sections in list order, so a selection reads in order, without
  /// moving the one holding the focus or a selection.
  function orderSections(ordered: HTMLElement[]) {
    const selection = document.getSelection();
    const anchor = ordered.findIndex(section => section.parentElement === win
      && (section.contains(document.activeElement) || (!!selection && !selection.isCollapsed && selection.containsNode(section, true))));

    if (anchor >= 0) {
      let before: Node = ordered[anchor];
      for (let i = anchor - 1; i >= 0; i--) {
        if (ordered[i].nextSibling !== before) win.insertBefore(ordered[i], before);
        before = ordered[i];
      }
    }

    let cursor = anchor >= 0 ? ordered[anchor].nextSibling : win.firstChild;
    for (const section of ordered.slice(anchor + 1)) {
      if (section === cursor) cursor = cursor.nextSibling;
      else win.insertBefore(section, cursor);
    }
  }

  function dropSection(path: string, section: HTMLElement) {
    forgetCommentsIn(section);
    headers.delete(path);
    section.remove();
    sections.delete(path);
    renderedFiles.delete(path);
    bodyModes.delete(path);
  }

  /// Lays out every file again from estimates, measuring the drawn ones now.
  function remeasure() {
    virtual.measure();
    for (const section of sections.values()) virtual.measureElement(section);
    placeFiles();
  }

  /// The laid out box of the file at `index`.
  function itemAt(index: number | undefined) {
    if (index === undefined) return null;
    virtual.getVirtualItems();
    return virtual.measurementsCache[index] ?? null;
  }

  /// Keeps the current file where it is on screen through `run`.
  function keepCurrent(run: () => void) {
    const before = itemAt(st.cur ? indexOf.get(st.cur) : undefined);
    const offset = before ? files.scrollTop - before.start : 0;
    run();
    const after = itemAt(st.cur ? indexOf.get(st.cur) : undefined);
    if (before && after && after.start !== before.start) files.scrollTop = after.start + offset;
  }

  const chunkObserver = new IntersectionObserver(entries => {
    const top = files.getBoundingClientRect().top;
    const changes = entries.flatMap(entry => {
      const c = chunks.get(entry.target);
      if (!c || !c.box.isConnected || c.live === entry.isIntersecting) return [];
      if (!entry.isIntersecting && (c.box.querySelector(".dv-cmt.open") || selected(c.box))) return [];
      // A file wholly above the viewport is kept steady by the virtualizer.
      const spans = (c.box.closest(".dv-file")?.getBoundingClientRect().bottom ?? 0) > top;
      return [{ c, want: entry.isIntersecting, rect: c.box.getBoundingClientRect(), spans }];
    });

    // Read all old heights before changing the DOM, then measure the new ones
    // together. Scrolling itself never walks the offscreen chunks.
    for (const { c, want, rect } of changes) {
      c.box.style.height = want ? "" : `${rect.height}px`;
      c.live = want;
      if (want) { c.comments = placed().files.get(c.path)?.lines ?? noComments; liveChunks.add(c); }
      else liveChunks.delete(c);
    }

    for (const { c, want } of changes) if (!want) forgetCommentsIn(c.box);
    if (changes.length) drawChunks();
    for (const { c, want } of changes) if (!want) c.box.replaceChildren();
    let shift = 0;

    for (const { c, want, rect, spans } of changes) {
      if (want && spans && rect.bottom <= top) shift += c.box.getBoundingClientRect().height - rect.height;
    }

    // Even assigning the same scrollTop stops WebKit's trackpad momentum.
    if (shift) files.scrollTop += shift;
    markCurrent();
  }, { root: files, rootMargin: `${MARGIN}px 0px` });
  const grip = el("div", "dv-resize");
  const body = el("div", "dv-body", tree, grip, files);
  const stopTreeResize = initTreeResize(body, grip);
  const keys = el("div", "dv-keys");
  root.append(head, feedback.note, body, keys);

  const rootResize = new ResizeObserver(() => {
    root.classList.toggle("wide", root.clientWidth >= WIDE);
    const narrow = root.clientWidth <= NARROW;
    if (narrow === st.narrow) return;
    st.narrow = narrow;
    if (st.split) redrawBodies();
  });
  rootResize.observe(root);
  const filesResize = new ResizeObserver(() => markCurrent());
  filesResize.observe(files);
  let scrollFrame = 0;
  files.addEventListener("scroll", () => {
    if (scrollFrame) return;
    scrollFrame = requestAnimationFrame(() => { scrollFrame = 0; markCurrent(); });
  }, { passive: true });
  files.addEventListener("click", onGutter);
  root.addEventListener("keydown", onKey);

  function shown(): DiffFile[] {
    return st.view ? st.view.files.filter(f => matchesFilter(f, st.filter)) : [];
  }

  function drawToolbar() {
    render("toolbar", createPortal(createElement(DiffToolbar, {
      view: st.view, loading: st.loading, filter: st.filter, split: st.split, uncommitted: st.uncommitted,
      showUncommitted: !!src.uncommitted, allFolded: allFolded(shown(), st.folded), flash: flashText,
      comments: st.draft.length, sending: st.sending, readOnly: !!src.readOnly, feedback: feedback.el,
      onFilter: value => { st.filter = value; applyFilter(); files.scrollTop = 0; remember(); },
      onSplit: () => { st.split = !st.split; saveSplit(st.split); drawToolbar(); redrawBodies(); },
      onUncommitted: () => { st.uncommitted = !st.uncommitted; void load(); },
      onFold: () => foldAll(!allFolded(shown(), st.folded)), onRefresh: () => void load(), onSend: () => void sendReview(),
    }), head, "toolbar"));
  }

  function drawHead() {
    drawToolbar();
    feedback.redraw();
  }

  function drawStatus() {
    render("status", createElement(Fragment, null,
      createPortal(st.error ? createElement("div", { className: "dv-err" }, st.error) : null, errorHost, "error"),
      createPortal(createElement(DiffMessage, { loading: st.loading, view: st.view }), messageHost, "message"),
      createPortal(createElement(DiffMore, { count: st.view?.more ?? 0 }), moreHost, "more"),
      createPortal(createElement(DiffKeys), keys, "keys"),
    ));
  }

  function drawFiles() {
    const v = st.view;
    for (const [comment, host] of lostComments) {
      if (!orphaned.has(comment)) continue;
      const input = host.querySelector<HTMLTextAreaElement>(".dv-cmt.open textarea"), entry = comments.get(host);
      if (!input || !entry) continue;
      const focused = document.activeElement === input;
      if (focused) revealOrphan = true;
      orphaned.set(comment, { text: input.value, focused, start: input.selectionStart, end: input.selectionEnd, isNew: entry.isNew });
    }
    hasRenderedView = !!v;
    chunkObserver.disconnect();
    for (const [path, section] of [...sections]) dropSection(path, section);
    comments.clear();
    lostComments.clear();
    drawComments();
    lostBox.replaceChildren();
    chunks.clear();
    liveChunks.clear();
    drawChunks();
    markedPath = null;
    bodyControls.clear();
    bodyControlsDirty = false;
    render("body-controls", null);
    drawHeaders();
    files.replaceChildren();
    placedCache = null;
    files.append(errorHost, lostBox, messageHost);
    drawStatus();
    relist([]);
    laidOutFilter = st.filter;

    if (!v || !v.files.length) {
      drawTree();
      if (v) drawReview();
      return;
    }

    files.append(matchNote, win, moreHost);
    relist(v.files);
    remeasure();
    applyFilter();
    drawReview();
  }

  function reconcileFiles(force = new Set<string>()): boolean {
    const view = st.view;
    if (!view || !hasRenderedView || !list.length) {
      if (view && !view.files.length && !list.length && hasRenderedView) { drawStatus(); return false; }
      drawFiles();
      return true;
    }
    const incoming = new Map(view.files.map(file => [file.path, file]));
    const laidOut = new Map(list.map(file => [file.path, file]));
    const before = (path: string) => sections.has(path) ? renderedFiles.get(path) : laidOut.get(path);
    const differs = (file: DiffFile) => {
      const old = before(file.path);
      return force.has(file.path) || !old || !sameDiffFile(old, file);
    };
    const kept = list.filter(file => incoming.has(file.path));
    const changed = view.files.some(differs) || kept.length !== list.length
      || kept.length !== view.files.length || kept.some((file, index) => view.files[index].path !== file.path);
    if (!changed) { drawStatus(); return false; }
    placedCache = null;

    for (const [path, section] of [...sections]) {
      const next = incoming.get(path), previous = renderedFiles.get(path);
      if (next && previous && !force.has(path) && sameDiffFile(previous, next)) continue;
      if (selected(section)) {
        deferredFiles.add(path);
        selectionDeferred.add(path);
        if (!watchingSelection) { document.addEventListener("selectionchange", selectionChanged); watchingSelection = true; }
        continue;
      }
      const editing = [...comments].flatMap(([host, entry]) => {
        const input = host.querySelector<HTMLTextAreaElement>(".dv-cmt.open textarea");
        return section.contains(host) && input ? [{ entry, input }] : [];
      });
      const stillAnchored = editing.filter(({ entry }) => {
        if (!next) return false;
        if (entry.comment.line === null) return true;
        return !!placeComments([next], [entry.comment]).files.get(path)?.lines.size;
      });
      if (stillAnchored.length) { deferredFiles.add(path); continue; }
      deferredFiles.delete(path);
      for (const { entry, input } of editing) {
        if (document.activeElement === input) revealOrphan = true;
        orphaned.set(entry.comment, { text: input.value, focused: document.activeElement === input, start: input.selectionStart, end: input.selectionEnd, isNew: entry.isNew });
      }
      dropSection(path, section);
    }

    // A file the view dropped keeps its place while its section is held.
    const next = view.files.slice();
    list.forEach((file, index) => {
      if (!incoming.has(file.path) && sections.has(file.path) && deferredFiles.has(file.path)) next.splice(Math.min(index, next.length), 0, file);
    });
    const stale = view.files.filter(file => !sections.has(file.path) && differs(file));
    const top = files.scrollTop;
    relist(next);
    for (const file of stale) virtual.resizeItem(indexOf.get(file.path) ?? -1, sizeOf(file));
    if (!next.length) { matchNote.remove(); win.remove(); moreHost.remove(); }
    placeFiles();
    if (files.scrollTop !== top) files.scrollTop = top;
    markedPath = null;
    applyFilter();
    drawReview();
    drawStatus();
    return true;
  }

  function drawTree(_synchronous = false) {
    const view = createElement(DiffTree, {
      files: st.view?.files ?? [], filter: st.filter, current: st.cur, closedFolders: st.closedFolders, filteredClosedFolders,
      scroller: tree, reveal: revealTree,
      onFolder: (path, open) => {
        const closed = st.filter.trim() ? filteredClosedFolders : st.closedFolders;
        if (open) closed.add(path); else closed.delete(path);
        drawTree(true);
      },
      onFile: jump, onMenu: (event, file) => fileMenu(event.nativeEvent, file),
    });
    render("tree", createPortal(view, tree, "tree"));
  }

  /// Hides the files that do not match, without drawing any again.
  function applyFilter() {
    const v = st.view;
    if (!v) return;
    for (const [path, section] of sections) {
      const f = list[indexOf.get(path) ?? -1];
      section.hidden = !!f && !matchesFilter(f, st.filter);
    }

    if (laidOutFilter !== st.filter) {
      laidOutFilter = st.filter;
      filteredClosedFolders.clear();
      remeasure();
    }

    matchNote.hidden = !st.filter.trim();
    matchNote.textContent = `${shown().length} of ${v.files.length} files match`;
    markCurrent(false);
    drawTree(true);
    drawToolbar();
  }

  /// The right-click menu of a file, on its header or its row in the list.
  function fileMenu(e: MouseEvent, f: DiffFile) {
    const items: MenuItem[] = [{ label: "Copy path", on: () => void copy(f.path) }];

    if (src.open && f.status !== "deleted") {
      const open = src.open;
      items.unshift({ label: open.label, on: () => open.run(f.path) });
    }

    const fuller = openMenu(e, f.path, items);
    if (!src.menu || f.status === "deleted") return;
    src.menu(f.path, { x: e.clientX, y: e.clientY }).then(more => { if (more.length) fuller([...items, ...more]); }, () => {});
  }

  function fileSection(f: DiffFile) {
    const folded = isFolded(f, st.folded);
    const section = el("section", `dv-file${folded ? " closed" : ""}${st.cur === f.path ? " cur" : ""}`);
    section.dataset.path = f.path;
    section.hidden = !matchesFilter(f, st.filter);
    // Comments on the file itself sit outside the fold, so a folded, deleted or binary file shows them.
    const fcs = el("div", "dv-fcs");
    const mine = placed().files.get(f.path);
    if (mine) fcs.append(...[...mine.file, ...mine.top].map(c => commentHost(c, lineLabel(c), false)));
    const fh = el("div");
    fh.style.display = "contents";
    headers.set(f.path, { host: fh, file: f, comments: fcs });
    const inner = el("div");
    // The lines are drawn the first time the file opens.
    if (!folded) inner.append(fileBody(f));
    section.append(fh, fcs, el("div", "dv-fold", inner));
    return section;
  }

  const context = new Map<string, { original: DiffFile; above: number; below: number }>();
  async function expandContext(f: DiffFile, direction: "above" | "below") {
    if (!st.view || st.pending.has(f.path)) return;
    const seq = st.seq;
    const kept = context.get(f.path) ?? { original: f, above: 0, below: 0 };
    const next = { ...kept, [direction]: kept[direction] + 10 };
    st.pending.add(f.path);
    try {
      const got = await src.load([f.path], true, st.uncommitted, Math.max(next.above, next.below) + 3);
      if (seq !== st.seq || !st.view) return;
      const full = got.files.find(file => file.path === f.path);
      if (!full || full.left_out) throw new Error("Could not load surrounding lines");
      context.set(f.path, next);
      st.view = withLoaded(st.view, [contextFile(kept.original, full, next.above, next.below)]);
      placedCache = null;
      redrawFile(f.path);
      remember();
    } catch (error) { say(`Could not expand context: ${reasonOf(error)}`); }
    finally { st.pending.delete(f.path); }
  }

  function fileBody(f: DiffFile): HTMLElement {
    bodyModes.set(f.path, st.split && !st.narrow);
    const note = leftOutNote(f);

    if (note) {
      return bodyControl(createElement(DiffOmitted, { text: note.text, label: note.load, busy: st.pending.has(f.path), load: () => void loadFull(f) }));
    }

    const split = st.split && !st.narrow;
    const lines = el("div", split ? "dv-lines split" : "dv-lines");
    lines.style.setProperty("--nw", String(Math.max(2, numberWidth(f))));
    const rows = diffRows(f);
    // The widest line is known up front, so the sideways scroll does not
    // change as rows come and go.
    lines.style.setProperty("--len", String(rows.reduce((m, r) => (r.kind === "line" ? Math.max(m, r.text.length) : m), 0)));
    // Rows are drawn in chunks, only while they are near the viewport; a
    // chunk off screen is an empty box as tall as it last was.
    const cols = Math.floor(((files.clientWidth || 900) / 2 - Math.max(2, numberWidth(f)) * CH - 50) / CH);
    const items: (DiffRow | SplitRow)[] = split ? splitRows(rows) : rows;

    for (let i = 0; i < items.length; i += CHUNK) {
      const slice = items.slice(i, i + CHUNK);
      const box = el("div", "dv-chunk");
      box.style.height = `${split ? splitHeight(slice as SplitRow[], cols) : slice.reduce((h, r) => h + (r.kind === "gap" ? 22 : 19), 0)}px`;
      chunks.set(box, { box, id: ++nextChunk, path: f.path, rows: slice, comments: noComments, live: false });
      chunkObserver.observe(box);
      lines.append(box);
    }

    const more = (direction: "above" | "below") => {
      return bodyControl(createElement(DiffContext, { direction, expand: () => void expandContext(f, direction) }));
    };
    lines.prepend(more("above"));
    lines.append(more("below"));

    return el("div", "dv-scroll", lines);
  }

  function drawChunks() {
    const portals = [...liveChunks].map(chunk => createPortal(createElement(DiffChunk, {
      rows: chunk.rows, comments: chunk.comments, mountComment: mountLineComment, unmountComment: unmountLineComment,
    }), chunk.box, String(chunk.id)));
    render("chunks", createElement(Fragment, null, ...portals));
    drawComments();
  }

  /// Stop observing chunks removed by a file or mode change.
  function pruneChunks() {
    let removed = false;
    for (const [box] of chunks) {
      if (root.contains(box)) continue;
      chunkObserver.unobserve(box);
      forgetCommentsIn(box);
      const chunk = chunks.get(box);
      if (chunk && liveChunks.delete(chunk)) removed = true;
      chunks.delete(box);
    }
    if (removed) drawChunks();
  }

  /// Whether the user's selection reaches into `box`: emptying it would lose the text.
  function selected(box: HTMLElement) {
    const sel = document.getSelection();
    return !!sel && !sel.isCollapsed && sel.containsNode(box, true);
  }

  function countLabel(path: string) {
    const n = st.draft.filter(c => c.path === path).length;
    return n ? `${n} ${n === 1 ? "comment" : "comments"}` : "";
  }

  function drawHeaders() {
    const portals = [...headers.entries()].map(([path, item]) => createPortal(createElement(DiffFileHeader, {
      file: item.file, count: countLabel(path), readOnly: !!src.readOnly, openLabel: src.open?.label ?? null,
      folded: isFolded(item.file, st.folded),
      onToggle: select => { if (select) st.cur = path; toggle(path); if (select) markCurrent(); },
      onMenu: event => fileMenu(event.nativeEvent, item.file),
      onComment: () => addBox({ path, side: "new", line: null, text: "" }, box => item.comments.append(box)),
      onCopy: () => void copy(path), onOpen: () => src.open?.run(path),
    }), item.host, path));
    render("headers", createElement(Fragment, null, ...portals));
    drawBodyControls();
  }

  function bodyControl(view: ReactNode) {
    const host = el("div");
    host.style.display = "contents";
    bodyControls.set(host, { id: ++nextBodyControl, view });
    bodyControlsDirty = true;
    return host;
  }

  function drawBodyControls() {
    for (const host of bodyControls.keys()) if (!root.contains(host)) { bodyControls.delete(host); bodyControlsDirty = true; }
    if (!bodyControlsDirty) return;
    bodyControlsDirty = false;
    render("body-controls", createElement(Fragment, null, ...[...bodyControls].map(([host, item]) => createPortal(item.view, host, String(item.id)))));
  }

  /// The header's count and Send button, each folded file's count and the
  /// comments on files the diff no longer has.
  function drawReview() {
    drawToolbar();

    drawHeaders();

    const lost = st.view ? placed().lost : [];
    const kept = new Set(lost);
    for (const [comment, host] of lostComments) {
      if (kept.has(comment)) continue;
      comments.delete(host);
      host.remove();
      lostComments.delete(comment);
    }
    if (lost.length && lostHeading.parentElement !== lostBox) lostBox.prepend(lostHeading);
    if (!lost.length) lostHeading.remove();
    let cursor = lostHeading.nextSibling;
    for (const comment of lost) {
      let host = lostComments.get(comment);
      if (!host) {
        const restored = orphaned.get(comment) ?? null;
        host = commentHost(comment, `${comment.path} ${lineLabel(comment)}`.trim(), restored?.isNew ?? false, restored);
        lostComments.set(comment, host);
      }
      if (host === cursor) cursor = cursor.nextSibling;
      else lostBox.insertBefore(host, cursor);
    }
    drawComments();
    for (const [comment, restored] of orphaned) {
      if (!restored?.focused) continue;
      const input = lostComments.get(comment)?.querySelector<HTMLTextAreaElement>("textarea");
      if (input && document.activeElement === input) { input.scrollIntoView({ block: "nearest" }); restored.focused = false; }
    }
    // The lost comments sit above the list, which then starts lower.
    if (win.isConnected) placeFiles();
  }

  /// The draft changed: keep it and show it.
  function changed() {
    placedCache = null;
    saveDraft(src.review.key, st.draft);
    drawReview();
  }

  /// Draft comments from an agent's review, left out when the draft has them already; says how many were new.
  function addAgentComments(add: ReviewComment[]) {
    const merged = mergeComments(st.draft, add);
    if (!merged.added) return 0;
    st.draft = merged.draft;
    // A review may finish after its viewer closes; keep the draft without reviving its UI.
    if (disposed) { saveDraft(src.review.key, st.draft); return merged.added; }
    changed();
    const keep = files.scrollTop;
    if (reconcileFiles(new Set(add.map(comment => comment.path))) && files.scrollTop !== keep) files.scrollTop = keep;
    return merged.added;
  }

  function commentHost(comment: ReviewComment, label: string, isNew: boolean, restored: EditorRestore | null = null) {
    const host = el("span", "dv-comment-slot");
    host.style.display = "contents";
    comments.set(host, { id: ++nextComment, comment, label, isNew, chunkOwned: false, restored });
    return host;
  }

  function forgetCommentsIn(container: Element) {
    for (const host of comments.keys()) if (container.contains(host)) comments.delete(host);
  }

  function drawComments() {
    const portals = [...comments].map(([host, entry]) => createPortal(createElement(DiffComment, {
      comment: entry.comment, label: entry.label, initiallyEditing: entry.isNew || !!entry.restored, isNew: entry.isNew, restored: entry.restored,
      onSave: value => {
        entry.comment.text = value;
        if (entry.isNew) { st.draft.push(entry.comment); entry.isNew = false; }
        entry.restored = null;
        if (orphaned.has(entry.comment)) orphaned.set(entry.comment, null);
        changed();
      },
      onRemove: () => {
        const index = st.draft.indexOf(entry.comment);
        if (index >= 0) st.draft.splice(index, 1);
        comments.delete(host);
        orphaned.delete(entry.comment);
        lostComments.delete(entry.comment);
        if (!entry.chunkOwned) host.remove();
        changed();
      },
      onCancelNew: () => { comments.delete(host); orphaned.delete(entry.comment); lostComments.delete(entry.comment); host.remove(); changed(); },
      onEscape: () => root.focus({ preventScroll: true }),
      onEditEnd: () => {
        entry.restored = null;
        if (orphaned.has(entry.comment)) orphaned.set(entry.comment, null);
        if (!deferredFiles.has(entry.comment.path)) return;
        queueMicrotask(() => {
          if ([...comments].some(([node, item]) => item.comment.path === entry.comment.path && node.querySelector(".dv-cmt.open"))) return;
          deferredFiles.delete(entry.comment.path);
          renderedFiles.delete(entry.comment.path);
          const top = files.scrollTop;
          if (reconcileFiles() && files.scrollTop !== top) files.scrollTop = top;
        });
      },
    }), host, String(entry.id)));
    render("comments", createElement(Fragment, null, ...portals));
  }

  /// A new comment goes where `put` says and enters the draft on Add.
  function addBox(comment: ReviewComment, put: (box: HTMLElement) => void) {
    const host = commentHost(comment, "", true);
    put(host);
    drawComments();
  }

  /// A click on a line's number opens a box under its row (under the pair, side by side).
  function onGutter(e: MouseEvent) {
    if (src.readOnly) return;
    const g = (e.target as Element).closest<HTMLElement>(".dv-g");
    const row = g?.parentElement;
    const path = g?.closest<HTMLElement>(".dv-file")?.dataset.path;
    if (!row || !path || row.matches(".hunk, .gap, .none")) return;
    const pair = row.parentElement?.classList.contains("dv-pair") ? row.parentElement : null;
    const del = row.classList.contains("del");
    // On an unchanged line the right cell has the new number.
    const nums = (!del && pair ? pair.lastElementChild! : row).firstElementChild!.children;
    const line = Number((del ? nums[0] : nums[nums.length - 1]).textContent);
    if (!line) return;
    let at: Element = pair ?? row;
    while (at.nextElementSibling?.matches(".dv-cmt, .dv-comment-slot")) at = at.nextElementSibling;
    addBox({ path, side: del ? "old" : "new", line, text: "" }, box => at.after(box));
  }

  async function sendReview() {
    if (st.sending || !st.draft.length) return;
    const sent = st.draft.slice();
    st.sending = true;
    st.error = "";
    drawReview();
    const why = await src.review.send(reviewText(src.review.heading(), st.view?.files ?? [], sent));
    st.sending = false;

    if (why) {
      st.error = why;
    } else {
      st.draft = st.draft.filter(c => !sent.includes(c));
      saveDraft(src.review.key, st.draft);
    }

    const keep = files.scrollTop;
    drawHead();
    drawFiles();
    files.scrollTop = keep;
    root.focus({ preventScroll: true });
  }

  function updateBodyMode(path: string) {
    if (bodyModes.get(path) === (st.split && !st.narrow)) return;
    const section = sections.get(path), f = renderedFiles.get(path);
    const inner = section?.querySelector(".dv-fold > div");
    if (!f || !inner || isFolded(f, st.folded)) return;
    inner.replaceChildren(fileBody(f));
  }

  /// The drawn files change modes now; the others are laid out again from
  /// estimates and draw in the new mode as they come into view.
  function redrawBodies() {
    keepCurrent(() => {
      for (const path of sections.keys()) updateBodyMode(path);
      pruneChunks();
      drawBodyControls();
      remeasure();
    });
  }

  function setFolded(path: string, folded: boolean, redraw = true) {
    st.folded.set(path, folded);
    if (redraw) remember();
    const section = sections.get(path);
    const f = renderedFiles.get(path) ?? list[indexOf.get(path) ?? -1];
    if (!f) return;

    if (!section) {
      if (redraw) virtual.resizeItem(indexOf.get(path) ?? -1, sizeOf(f));
      return;
    }

    const inner = section.querySelector(".dv-fold > div");
    if (!folded && inner && !inner.childNodes.length) inner.append(fileBody(f));
    if (!folded) updateBodyMode(path);
    section.classList.toggle("closed", folded);
    if (redraw) { drawHeaders(); drawToolbar(); }
  }

  function toggle(path: string) {
    const f = st.view?.files.find(x => x.path === path);
    if (!f) return;
    const folding = !isFolded(f, st.folded);
    // Folding a file scrolled past its header keeps its header in view.
    const item = itemAt(indexOf.get(path));
    if (folding && item && item.start < files.scrollTop) files.scrollTop = item.start - GAP;
    setFolded(path, folding);
  }

  function foldAll(folded: boolean) {
    keepCurrent(() => {
      for (const f of shown()) setFolded(f.path, folded, false);
      remeasure();
    });
    remember();
    drawHeaders();
    drawToolbar();
    if (folded) files.scrollTop = 0;
  }

  /// Where each shown file's header sits, less the gap above it.
  function tops(list: readonly DiffFile[]) {
    virtual.getVirtualItems();
    return list.map(f => (virtual.measurementsCache[indexOf.get(f.path) ?? -1]?.start ?? 0) - GAP);
  }

  let markedPath: string | null = null;
  function revealFolders(path: string | null) {
    if (!path) return;
    for (const closed of [st.closedFolders, filteredClosedFolders]) {
      for (const folder of closed) if (path.startsWith(folder)) closed.delete(folder);
    }
  }

  function markCurrent(reveal = true) {
    const list = shown();
    const held = itemAt(st.cur ? indexOf.get(st.cur) : undefined);
    if (!held || !held.size || !inView(held.start, held.end - GAP, files.scrollTop, files.clientHeight)) {
      st.cur = list[currentFile(tops(list), files.scrollTop)]?.path ?? null;
    }
    if (markedPath === st.cur) return;
    markedPath = st.cur;
    for (const [p, s] of sections) s.classList.toggle("cur", p === st.cur);
    if (!reveal) return;
    revealFolders(st.cur);
    revealTree++;
    drawTree();
  }

  function jump(path: string) {
    const index = indexOf.get(path), item = itemAt(index);
    if (index === undefined || !item) return;

    revealFolders(path);
    st.cur = path;
    markCurrent();
    revealTree++;
    drawTree();
    // A long way is not worth animating; it would draw every file between.
    const near = Math.abs(item.start - files.scrollTop) < files.clientHeight * 3;
    const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
    virtual.scrollToIndex(index, { align: "start", behavior: near && !reduced ? "smooth" : "auto" });
  }

  function onKey(e: KeyboardEvent) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const t = e.target as HTMLElement;

    const find = head.querySelector<HTMLInputElement>(".dv-find");
    if (t === find) {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); find?.blur(); root.focus(); }
      return;
    }

    if (t.closest("input, textarea, [contenteditable='true']")) return;
    const list = shown();
    if (["j", "k", "x", "/"].includes(e.key)) e.stopPropagation();
    const at = list.findIndex(f => f.path === st.cur);

    if (e.key === "j" || e.key === "k") {
      const i = stepFile(at, list.length, e.key === "j" ? 1 : -1);
      e.preventDefault();
      if (i != null) jump(list[i].path);
    } else if (e.key === "x") {
      const f = list[at] ?? list[currentFile(tops(list), files.scrollTop)];
      e.preventDefault();
      if (f) toggle(f.path);
    } else if (e.key === "/") {
      e.preventDefault();
      find?.focus();
    }
  }

  async function copy(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      say("Copied the path");
    } catch (e) {
      say(`Could not copy: ${reasonOf(e)}`);
    }
  }

  async function loadFull(f: DiffFile) {
    if (!st.view || st.pending.has(f.path)) return;
    const seq = st.seq;
    st.pending.add(f.path);
    if ([...comments].some(([node, item]) => item.comment.path === f.path && node.querySelector(".dv-cmt.open"))) deferredFiles.add(f.path);
    else redrawFile(f.path);

    try {
      const got = await src.load([f.path], true, st.uncommitted);
      if (seq !== st.seq) return;
      if (st.view) st.view = withLoaded(st.view, got.files);
    } catch (e) {
      if (seq !== st.seq) return;
      st.error = `Could not read ${f.path}: ${reasonOf(e)}`;
    } finally {
      st.pending.delete(f.path);
    }

    st.folded.set(f.path, false);
    reconcileFiles(new Set([f.path]));
  }

  /// Draws one file's section again, in place, or lays it out again when it
  /// is not drawn.
  function redrawFile(path: string) {
    const f = st.view?.files.find(x => x.path === path), old = sections.get(path), index = indexOf.get(path);
    if (!f || index === undefined) return;
    list[index] = f;

    if (!old) {
      virtual.resizeItem(index, sizeOf(f));
      return;
    }

    const next = fileSection(f);
    forgetCommentsIn(old);
    next.dataset.index = old.dataset.index;
    next.style.top = old.style.top;
    old.replaceWith(next);
    sections.set(path, next);
    renderedFiles.set(path, f);
    virtual.measureElement(next);
    drawHeaders();
    drawComments();
    pruneChunks();
    markedPath = null;
    markCurrent();
  }

  async function load() {
    if (st.view) { remember(); memory = readView(memoryKey); }
    const seq = ++st.seq;
    context.clear();
    st.loading = true;
    st.error = "";
    drawHead();
    if (!st.view) drawFiles();

    try {
      const view = await src.load([], false, st.uncommitted);
      if (seq !== st.seq) return;
      st.view = { ...view, files: filesInTreeOrder(view.files) };
      if (memory && typeof memory === "object" && "context" in memory && Array.isArray(memory.context)) {
        for (const item of memory.context) {
          if (!Array.isArray(item) || typeof item[0] !== "string" || typeof item[1] !== "number" || typeof item[2] !== "number") continue;
          const original = view.files.find(file => file.path === item[0]);
          if (!original) continue;
          const above = Math.max(0, Math.min(49_990, item[1])), below = Math.max(0, Math.min(49_990, item[2]));
          const expanded = await src.load([original.path], true, st.uncommitted, Math.max(above, below) + 3);
          if (seq !== st.seq || !st.view) return;
          const full = expanded.files.find(file => file.path === original.path);
          if (full && !full.left_out) {
            context.set(original.path, { original, above, below });
            st.view = withLoaded(st.view, [contextFile(original, full, above, below)]);
          }
        }
      }
    } catch (e) {
      if (seq !== st.seq) return;
      st.error = reasonOf(e);
    }

    st.loading = false;
    const keep = restoredScroll || files.scrollTop;
    restoredScroll = 0;
    drawHead();
    if (reconcileFiles() && files.scrollTop !== keep) files.scrollTop = keep;
    if (revealOrphan) {
      revealOrphan = false;
      const active = document.activeElement;
      if (active instanceof HTMLElement && lostBox.contains(active)) active.scrollIntoView({ block: "nearest" });
    }
  }

  drawReview();
  load();
  return {
    el: root,
    reload: load,
    focus: () => root.focus({ preventScroll: true }),
    dispose: () => {
      if (disposed) return;
      if (rememberTimer) remember();
      disposed = true; st.seq++;
      clearTimeout(flashTimer);
      if (scrollFrame) cancelAnimationFrame(scrollFrame);
      chunkObserver.disconnect(); rootResize.disconnect(); filesResize.disconnect(); stopTreeResize(); stopVirtual();
      if (watchingSelection) document.removeEventListener("selectionchange", selectionChanged);
      bodyControls.clear();
      feedback.dispose(); src.owner.dispose();
    },
  };
}

function reasonOf(e: unknown) {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
