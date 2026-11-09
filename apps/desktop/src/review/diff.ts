import { createElement, Fragment, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { DiffToolbar } from "./diff-toolbar.tsx";
import { DiffTree } from "./diff-tree.tsx";
import { DiffFileHeader } from "./diff-file-header.tsx";
import { DiffChunk } from "./diff-chunk.tsx";
import { DiffComment } from "./diff-comment.tsx";
import { DiffKeys, DiffMessage, DiffMore } from "./diff-status.tsx";
import { readView, writeView } from "../agentpane/view-memory.ts";
// The diff viewer: a one-line header with the counts, a filter, icon toggles
// and the review buttons; a list of the files at wide widths; each file folding under
// its sticky header, its lines in a unified view that scrolls sideways on its
// own.
import {
  type DiffFile, type DiffRow, type DiffView, type ReviewComment, type SplitRow, allFolded, currentFile, contextFile, diffRows, isFolded, leftOutNote, sameDiffFile,
  filesInTreeOrder, inView, lineLabel, matchesFilter, numberWidth, placeComments, reviewText, splitHeight, splitRows, stepFile, withLoaded,
} from "./diff-state.ts";
import { initTreeResize } from "./treewidth.ts";
import { loadDraft, saveDraft } from "./draftstore.ts";
import { feedbackButton } from "./feedback.ts";
import { mergeComments } from "./feedback-state.ts";
import { el, type Kid } from "../ui/dom.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";

/// Where a diff comes from and what its files can open in.
export type DiffSlot = "toolbar" | "status" | "tree" | "chunks" | "headers" | "comments" | "feedback";
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

const SVG = {
  to: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>',
};

/// An icon from the fixed markup above, never from a diff.
function icon(svg: string) {
  const s = el("span");
  s.style.display = "grid";
  s.innerHTML = svg;
  return s;
}

function btn(label: Kid, cls: string, title: string, run: () => void) {
  const b = el("button", `dv-btn${cls ? ` ${cls}` : ""}`, label);
  b.type = "button";
  if (title) b.title = title;
  b.addEventListener("click", e => { e.stopPropagation(); run(); });
  return b;
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
  /// Each file's section, by path, as last drawn.
  let sections = new Map<string, HTMLElement>();
  let renderedFiles = new Map<string, DiffFile>();
  let hasRenderedView = false;
  /// The chunks of rows of every open file.
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
  const visibleFiles = new Set<string>();
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
  let revealTree = false;
  const filteredClosedFolders = new Set<string>();
  const files = el("div", "dv-files");
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
  const remember = () => writeView(memoryKey, { scroll: files.scrollTop, filter: st.filter, folded: [...st.folded], context: [...context].map(([path, value]) => [path, value.above, value.below]) });
  files.addEventListener("scroll", remember);

  files.style.position = "relative";
  const fileObserver = new IntersectionObserver(entries => {
    const near: string[] = [];

    for (const entry of entries) {
      const path = (entry.target as HTMLElement).dataset.path!;

      if (entry.isIntersecting) {
        visibleFiles.add(path);
        near.push(path);
      } else visibleFiles.delete(path);
    }

    updateBodyModes(near);
  }, { root: files, rootMargin: `${MARGIN}px 0px` });
  const chunkObserver = new IntersectionObserver(entries => {
    const top = files.getBoundingClientRect().top;
    const changes = entries.flatMap(entry => {
      const c = chunks.get(entry.target);
      if (!c || !c.box.isConnected || c.live === entry.isIntersecting) return [];
      if (!entry.isIntersecting && (c.box.querySelector(".dv-cmt.open") || selected(c.box))) return [];
      return [{ c, want: entry.isIntersecting, rect: c.box.getBoundingClientRect() }];
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

    for (const { c, want, rect } of changes) {
      if (want && rect.bottom <= top) shift += c.box.getBoundingClientRect().height - rect.height;
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
    renderedFiles = new Map((v?.files ?? []).map(file => [file.path, file]));
    chunkObserver.disconnect();
    fileObserver.disconnect();
    comments.clear();
    lostComments.clear();
    drawComments();
    lostBox.replaceChildren();
    chunks.clear();
    liveChunks.clear();
    drawChunks();
    bodyModes.clear();
    visibleFiles.clear();
    markedPath = null;
    headers.clear();
    drawHeaders();
    drawTree();
    files.replaceChildren();
    sections = new Map();
    placedCache = null;
    files.append(errorHost, lostBox, messageHost);
    drawStatus();

    if (!v) {
      return;
    }

    if (!v.files.length) {
      drawReview();
      return;
    }

    files.append(matchNote);

    for (const f of v.files) {
      const section = fileSection(f);
      sections.set(f.path, section);
      files.append(section);
      fileObserver.observe(section);
    }

    files.append(moreHost);
    applyFilter();
    drawReview();
  }

  function reconcileFiles(force = new Set<string>()): boolean {
    const view = st.view;
    if (!view || !sections.size) {
      if (view && !view.files.length && !sections.size && hasRenderedView) { drawStatus(); return false; }
      drawFiles();
      return true;
    }
    const incoming = new Map(view.files.map(file => [file.path, file]));
    const changed = view.files.some(file => {
      const old = renderedFiles.get(file.path);
      return force.has(file.path) || !old || !sameDiffFile(old, file);
    }) || [...sections.keys()].some(path => !incoming.has(path)) || [...sections.keys()].some((path, index) => view.files[index]?.path !== path);
    if (!changed) { drawStatus(); return false; }

    placedCache = null;
    for (const [path, section] of sections) {
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
      fileObserver.unobserve(section);
      visibleFiles.delete(path);
      forgetCommentsIn(section);
      headers.delete(path);
      if (next) {
        const replacement = fileSection(next);
        section.replaceWith(replacement);
        sections.set(path, replacement);
        fileObserver.observe(replacement);
      } else {
        section.remove();
        sections.delete(path);
        bodyModes.delete(path);
      }
    }
    for (const file of view.files) {
      let section = sections.get(file.path);
      if (!section) {
        section = fileSection(file);
        sections.set(file.path, section);
        fileObserver.observe(section);
      }
    }
    if (!view.files.length && !sections.size) { matchNote.remove(); moreHost.remove(); }
    const selection = document.getSelection();
    const anchor = view.files.findIndex(file => {
      const section = sections.get(file.path);
      return !!section && (section.contains(document.activeElement) || !!selection && !selection.isCollapsed && selection.containsNode(section, true));
    });
    if (anchor >= 0) {
      let before: Node = sections.get(view.files[anchor].path)!;
      for (let i = anchor - 1; i >= 0; i--) {
        const section = sections.get(view.files[i].path)!;
        if (section.nextSibling !== before) files.insertBefore(section, before);
        before = section;
      }
    }
    let cursor = anchor >= 0 ? sections.get(view.files[anchor].path)!.nextSibling : matchNote.nextSibling;
    for (const file of view.files.slice(anchor + 1)) {
      const section = sections.get(file.path)!;
      if (section === cursor) cursor = cursor.nextSibling;
      else files.insertBefore(section, cursor);
    }
    const pendingRemoved = [...sections].filter(([path]) => !incoming.has(path) && deferredFiles.has(path));
    sections = new Map([...view.files.map(file => [file.path, sections.get(file.path)!] as const), ...pendingRemoved]);
    renderedFiles = new Map([...view.files.map(file => [file.path, deferredFiles.has(file.path) ? renderedFiles.get(file.path) ?? file : file] as const), ...pendingRemoved.flatMap(([path]) => { const file = renderedFiles.get(path); return file ? [[path, file] as const] : []; })]);
    pruneChunks();
    markedPath = null;
    drawTree();
    applyFilter();
    drawReview();
    drawStatus();
    return true;
  }

  function drawTree(_synchronous = false) {
    const view = createElement(DiffTree, {
      files: st.view?.files ?? [], filter: st.filter, current: st.cur, closedFolders: st.closedFolders, filteredClosedFolders,
      onFolder: (path, open) => {
        const closed = st.filter.trim() ? filteredClosedFolders : st.closedFolders;
        if (open) closed.add(path); else closed.delete(path);
        drawTree(true);
      },
      onFile: jump, onMenu: (event, file) => fileMenu(event.nativeEvent, file),
      onCommit: () => {
        if (!revealTree) return;
        revealTree = false;
        const row = [...tree.querySelectorAll<HTMLElement>(".dv-tr[data-path]")].find(item => item.dataset.path === st.cur);
        if (!row || !tree.clientHeight) return;
        const bounds = tree.getBoundingClientRect(), item = row.getBoundingClientRect();
        if (item.top < bounds.top) tree.scrollTop += item.top - bounds.top;
        else if (item.bottom > bounds.bottom) tree.scrollTop += item.bottom - bounds.bottom;
      },
    });
    render("tree", createPortal(view, tree, "tree"));
  }

  /// Hides the files that do not match, without drawing any again.
  function applyFilter() {
    const v = st.view;
    if (!v) return;
    filteredClosedFolders.clear();
    v.files.forEach(f => { sections.get(f.path)!.hidden = !matchesFilter(f, st.filter); });

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
    const section = el("section", `dv-file${folded ? " closed" : ""}`);
    section.dataset.path = f.path;
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
      const box = el("div", "dv-note", note.text);

      if (note.load) {
        const busy = st.pending.has(f.path);
        const b = btn(busy ? "Loading…" : note.load, "", "", () => loadFull(f));
        b.disabled = busy;
        box.append(b);
      }

      return box;
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
      const label = `Show 10 lines ${direction}`;
      const control = btn(el("span", "dv-context-g", icon(SVG.to)), `dv-context ${direction}`, label, () => expandContext(f, direction));
      control.setAttribute("aria-label", label);
      return control;
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
    const section = sections.get(path), f = st.view?.files.find(x => x.path === path);
    const inner = section?.querySelector(".dv-fold > div");
    if (!f || !inner || isFolded(f, st.folded)) return;
    inner.replaceChildren(fileBody(f));
  }

  function updateBodyModes(paths: Iterable<string>) {
    const stale = [...paths].filter(path => bodyModes.has(path) && bodyModes.get(path) !== (st.split && !st.narrow));
    if (!stale.length) return;
    const top = files.scrollTop;
    const anchor = sections.get(shown()[currentFile(tops(), top)]?.path ?? "");
    const before = anchor?.offsetTop ?? 0;
    for (const path of stale) updateBodyMode(path);
    const next = top + (anchor?.offsetTop ?? 0) - before;
    if (files.scrollTop !== next) files.scrollTop = next;
    pruneChunks();
  }

  /// Only the files near the viewport change modes now. Others update
  /// when they scroll into view or open, so resizing stays cheap.
  function redrawBodies() {
    updateBodyModes(visibleFiles);
  }

  function setFolded(path: string, folded: boolean, redraw = true) {
    st.folded.set(path, folded);
    if (redraw) remember();
    const section = sections.get(path);
    const f = st.view?.files.find(x => x.path === path);
    if (!section || !f) return;
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
    const section = sections.get(path);
    if (folding && section && section.offsetTop < files.scrollTop) files.scrollTop = section.offsetTop - 10;
    setFolded(path, folding);
  }

  function foldAll(folded: boolean) {
    for (const f of shown()) setFolded(f.path, folded, false);
    remember();
    drawHeaders();
    drawToolbar();
    if (folded) files.scrollTop = 0;
  }

  function tops() {
    return shown().map(f => (sections.get(f.path)?.offsetTop ?? 0) - 10);
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
    const held = st.cur ? sections.get(st.cur) : null;
    if (!held || !inView(held.offsetTop, held.offsetTop + held.offsetHeight, files.scrollTop, files.clientHeight)) {
      st.cur = list[currentFile(tops(), files.scrollTop)]?.path ?? null;
    }
    if (markedPath === st.cur) return;
    markedPath = st.cur;
    for (const [p, s] of sections) s.classList.toggle("cur", p === st.cur);
    if (!reveal) return;
    revealFolders(st.cur);
    revealTree = true;
    drawTree();
  }

  function jump(path: string) {
    const section = sections.get(path);
    if (!section) return;

    revealFolders(path);
    st.cur = path;
    markCurrent();
    revealTree = true;
    drawTree();
    files.scrollTo({ top: section.offsetTop - 10, behavior: typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
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
      const f = list[at] ?? list[currentFile(tops(), files.scrollTop)];
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

  /// Draws one file's section again, in place.
  function redrawFile(path: string) {
    const f = st.view?.files.find(x => x.path === path), old = sections.get(path);
    if (!f || !old) return;
    const next = fileSection(f);
    forgetCommentsIn(old);
    old.replaceWith(next);
    sections.set(path, next);
    renderedFiles.set(path, f);
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
      disposed = true; st.seq++;
      clearTimeout(flashTimer);
      if (scrollFrame) cancelAnimationFrame(scrollFrame);
      fileObserver.disconnect(); chunkObserver.disconnect(); rootResize.disconnect(); filesResize.disconnect(); stopTreeResize();
      if (watchingSelection) document.removeEventListener("selectionchange", selectionChanged);
      feedback.dispose(); src.owner.dispose();
    },
  };
}

function reasonOf(e: unknown) {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
