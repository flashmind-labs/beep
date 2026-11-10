// The diff viewer's drawing: a one-line header with the counts, a filter,
// icon toggles and the review buttons; a list of the files at wide widths;
// each file folding under its sticky header, its lines in a unified view that
// scrolls sideways on its own. Only the files near the viewport have
// sections, laid out by a TanStack virtualizer; within a section, only nearby
// chunks of rows are drawn.
import { createElement, Fragment, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { defaultRangeExtractor, elementScroll, observeElementOffset, observeElementRect, type Range, type VirtualizerOptions } from "@tanstack/react-virtual";
import { DiffToolbar } from "./diff-toolbar.tsx";
import { DiffTree } from "./diff-tree.tsx";
import { DiffFileHeader } from "./diff-file-header.tsx";
import { DiffContext, DiffOmitted } from "./diff-body-controls.tsx";
import { DiffChunk } from "./diff-chunk.tsx";
import { DiffKeys, DiffMessage, DiffMore } from "./diff-status.tsx";
import {
  type DiffFile, type DiffRow, type SplitRow, allFolded, diffRows, estimateHeight, isFolded, leftOutNote, lineLabel, matchesFilter,
  numberWidth, placeComments, sameDiffFile, splitHeight, splitRows,
} from "./diff-state.ts";
import { CH, CHUNK, GAP, type Viewer, noComments, placed, reasonOf, remember, render, saveSplit, selected, shown } from "./diff-viewer.ts";
import { addBox, commentHost, drawComments, drawLostComments, forgetCommentsIn, keepOrphans } from "./diff-comments.ts";
import { jump, markCurrent } from "./diff-keys.ts";
import { expandContext, load, loadFull, sendReview } from "./diff.ts";
import { el } from "../ui/dom.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";

/// Draws the files of the view from scratch.
export function drawFiles(v: Viewer) {
  const view = v.view;
  keepOrphans(v);
  v.hasRenderedView = !!view;
  v.chunkObserver.disconnect();
  for (const [path, section] of [...v.sections]) dropSection(v, path, section);
  v.comments.clear();
  v.lostComments.clear();
  drawComments(v);
  v.lostBox.replaceChildren();
  v.chunks.clear();
  v.liveChunks.clear();
  drawChunks(v);
  v.markedPath = null;
  v.bodyControls.clear();
  v.bodyControlsDirty = false;
  render(v, "body-controls", null);
  drawHeaders(v);
  v.files.replaceChildren();
  v.placedCache = null;
  v.files.append(v.errorHost, v.lostBox, v.messageHost);
  drawStatus(v);
  relist(v, []);
  v.laidOutFilter = v.filter;

  if (!view || !view.files.length) {
    drawTree(v);
    if (view) drawReview(v);
    return;
  }

  v.files.append(v.matchNote, v.win, v.moreHost);
  relist(v, view.files);
  remeasure(v);
  applyFilter(v);
  drawReview(v);
}

/// Brings the drawn files up to the view, redrawing only the files that
/// changed or `force` names. Whether anything changed.
export function reconcileFiles(v: Viewer, force = new Set<string>()): boolean {
  const view = v.view;

  if (!view || !v.hasRenderedView || !v.list.length) {
    if (view && !view.files.length && !v.list.length && v.hasRenderedView) { drawStatus(v); return false; }
    drawFiles(v);
    return true;
  }

  const incoming = new Map(view.files.map(file => [file.path, file]));
  const laidOut = new Map(v.list.map(file => [file.path, file]));
  const before = (path: string) => v.sections.has(path) ? v.renderedFiles.get(path) : laidOut.get(path);
  const differs = (file: DiffFile) => {
    const old = before(file.path);
    return force.has(file.path) || !old || !sameDiffFile(old, file);
  };
  const kept = v.list.filter(file => incoming.has(file.path));
  const changed = view.files.some(differs) || kept.length !== v.list.length
    || kept.length !== view.files.length || kept.some((file, index) => view.files[index].path !== file.path);
  if (!changed) { drawStatus(v); return false; }
  v.placedCache = null;

  for (const [path, section] of [...v.sections]) {
    const next = incoming.get(path), previous = v.renderedFiles.get(path);
    if (next && previous && !force.has(path) && sameDiffFile(previous, next)) continue;

    if (selected(section)) {
      v.deferredFiles.add(path);
      v.selectionDeferred.add(path);
      if (!v.watchingSelection) { document.addEventListener("selectionchange", v.onSelection); v.watchingSelection = true; }
      continue;
    }

    const editing = [...v.comments].flatMap(([host, entry]) => {
      const input = host.querySelector<HTMLTextAreaElement>(".dv-cmt.open textarea");
      return section.contains(host) && input ? [{ entry, input }] : [];
    });
    const stillAnchored = editing.filter(({ entry }) => {
      if (!next) return false;
      if (entry.comment.line === null) return true;
      return !!placeComments([next], [entry.comment]).files.get(path)?.lines.size;
    });
    if (stillAnchored.length) { v.deferredFiles.add(path); continue; }
    v.deferredFiles.delete(path);

    for (const { entry, input } of editing) {
      if (document.activeElement === input) v.revealOrphan = true;
      v.orphaned.set(entry.comment, { text: input.value, focused: document.activeElement === input, start: input.selectionStart, end: input.selectionEnd, isNew: entry.isNew });
    }

    dropSection(v, path, section);
  }

  // A file the view dropped keeps its place while its section is held.
  const next = view.files.slice();
  v.list.forEach((file, index) => {
    if (!incoming.has(file.path) && v.sections.has(file.path) && v.deferredFiles.has(file.path)) next.splice(Math.min(index, next.length), 0, file);
  });
  const stale = view.files.filter(file => !v.sections.has(file.path) && differs(file));
  const top = v.files.scrollTop;
  relist(v, next);
  for (const file of stale) v.virtual.resizeItem(v.indexOf.get(file.path) ?? -1, sizeOf(v, file));
  if (!next.length) { v.matchNote.remove(); v.win.remove(); v.moreHost.remove(); }
  placeFiles(v);
  if (v.files.scrollTop !== top) v.files.scrollTop = top;
  v.markedPath = null;
  applyFilter(v);
  drawReview(v);
  drawStatus(v);
  return true;
}

/// A deferred file whose selection went is drawn again.
export function selectionChanged(v: Viewer) {
  if (!v.root.isConnected) {
    v.selectionDeferred.clear();
    document.removeEventListener("selectionchange", v.onSelection);
    v.watchingSelection = false;
    return;
  }

  let ready = false;

  for (const path of v.selectionDeferred) {
    const section = v.sections.get(path);
    if (section && selected(section)) continue;
    v.selectionDeferred.delete(path);
    v.deferredFiles.delete(path);
    v.renderedFiles.delete(path);
    ready = true;
  }

  if (!v.selectionDeferred.size) {
    document.removeEventListener("selectionchange", v.onSelection);
    v.watchingSelection = false;
  }

  if (ready) {
    const top = v.files.scrollTop;
    if (reconcileFiles(v) && v.files.scrollTop !== top) v.files.scrollTop = top;
  }
}

/// Draws one file's section again, in place, or lays it out again when it
/// is not drawn.
export function redrawFile(v: Viewer, path: string) {
  const f = v.view?.files.find(x => x.path === path), old = v.sections.get(path), index = v.indexOf.get(path);
  if (!f || index === undefined) return;
  v.list[index] = f;

  if (!old) {
    v.virtual.resizeItem(index, sizeOf(v, f));
    return;
  }

  const next = fileSection(v, f);
  forgetCommentsIn(v, old);
  next.dataset.index = old.dataset.index;
  next.style.top = old.style.top;
  old.replaceWith(next);
  v.sections.set(path, next);
  v.renderedFiles.set(path, f);
  v.virtual.measureElement(next);
  drawHeaders(v);
  drawComments(v);
  pruneChunks(v);
  v.markedPath = null;
  markCurrent(v);
}

export function layoutOptions(v: Viewer): VirtualizerOptions<HTMLElement, HTMLElement> {
  const laidOut = v.list;
  return {
    count: laidOut.length,
    getScrollElement: () => v.files,
    estimateSize: index => sizeOf(v, laidOut[index]),
    getItemKey: index => laidOut[index]?.path ?? index,
    measureElement: (node, entry) => node.hidden ? 0 : Math.round(entry?.borderBoxSize?.[0]?.blockSize ?? node.offsetHeight) + GAP,
    rangeExtractor: v.holdRange,
    overscan: 2,
    paddingStart: GAP,
    scrollPaddingStart: GAP,
    scrollMargin: v.win.offsetTop,
    useAnimationFrameWithResizeObserver: true,
    observeElementRect, observeElementOffset, scrollToFn: elementScroll,
    onChange: () => placeFiles(v),
  };
}

/// A file's height before it is measured, with the gap after it.
function sizeOf(v: Viewer, f: DiffFile | undefined) {
  return f && matchesFilter(f, v.filter) ? estimateHeight(f, isFolded(f, v.folded)) + GAP : 0;
}

/// The files near the viewport, and the drawn ones that must stay: the
/// focus, a selection or an open comment would be lost with them.
export function keepHeld(v: Viewer, range: Range) {
  const out = defaultRangeExtractor(range);
  const extra: number[] = [];

  for (const [path, section] of v.sections) {
    const index = v.indexOf.get(path);
    if (index === undefined || (index >= out[0] && index <= out[out.length - 1])) continue;
    if (v.deferredFiles.has(path) || section.contains(document.activeElement) || section.querySelector(".dv-cmt.open") || selected(section)) extra.push(index);
  }

  return extra.length ? [...new Set([...out, ...extra])].sort((a, b) => a - b) : out;
}

/// Sets what the list lays out, keeping measured sizes by path.
function relist(v: Viewer, next: DiffFile[]) {
  v.list = next;
  v.indexOf = new Map(next.map((f, i) => [f.path, i]));
  v.virtual.setOptions(layoutOptions(v));
}

/// Positions the drawn sections and adds or drops sections as files come
/// near the viewport or leave it.
function placeFiles(v: Viewer) {
  if (v.placing) { v.placeAgain = true; return; }
  v.placing = true;

  try {
    for (let pass = 0; pass < 4; pass++) {
      v.placeAgain = false;
      placeOnce(v);
      v.virtual._willUpdate();
      if (!v.placeAgain) break;
    }
  } finally { v.placing = false; }
}

function placeOnce(v: Viewer) {
  const { virtual, win, sections } = v;
  if (win.isConnected && virtual.options.scrollMargin !== win.offsetTop) virtual.setOptions(layoutOptions(v));
  const items = virtual.getVirtualItems();
  const height = `${virtual.getTotalSize()}px`;
  if (win.style.height !== height) win.style.height = height;
  const wanted = new Set(items.map(item => v.list[item.index].path));
  let changed = false;

  for (const [path, section] of sections) {
    if (wanted.has(path)) continue;
    dropSection(v, path, section);
    changed = true;
  }

  const added: HTMLElement[] = [];
  const ordered: HTMLElement[] = [];

  for (const item of items) {
    const f = v.list[item.index];
    let section = sections.get(f.path);

    if (!section) {
      section = fileSection(v, f);
      sections.set(f.path, section);
      v.renderedFiles.set(f.path, f);
      added.push(section);
    }

    section.dataset.index = String(item.index);
    const top = `${item.start - virtual.options.scrollMargin}px`;
    if (section.style.top !== top) section.style.top = top;
    ordered.push(section);
  }

  orderSections(v, ordered);

  if (added.length || changed) {
    pruneChunks(v);
    drawHeaders(v);
    drawComments(v);
    v.markedPath = null;
    markCurrent(v, false);
  }

  for (const section of added) virtual.measureElement(section);
}

/// Puts the sections in list order, so a selection reads in order, without
/// moving the one holding the focus or a selection.
function orderSections(v: Viewer, ordered: HTMLElement[]) {
  const win = v.win;
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

function dropSection(v: Viewer, path: string, section: HTMLElement) {
  forgetCommentsIn(v, section);
  v.headers.delete(path);
  section.remove();
  v.sections.delete(path);
  v.renderedFiles.delete(path);
  v.bodyModes.delete(path);
}

/// Lays out every file again from estimates, measuring the drawn ones now.
function remeasure(v: Viewer) {
  v.virtual.measure();
  for (const section of v.sections.values()) v.virtual.measureElement(section);
  placeFiles(v);
}

/// The laid out box of the file at `index`.
export function itemAt(v: Viewer, index: number | undefined) {
  if (index === undefined) return null;
  v.virtual.getVirtualItems();
  return v.virtual.measurementsCache[index] ?? null;
}

/// Keeps the current file where it is on screen through `run`.
function keepCurrent(v: Viewer, run: () => void) {
  const before = itemAt(v, v.cur ? v.indexOf.get(v.cur) : undefined);
  const offset = before ? v.files.scrollTop - before.start : 0;
  run();
  const after = itemAt(v, v.cur ? v.indexOf.get(v.cur) : undefined);
  if (before && after && after.start !== before.start) v.files.scrollTop = after.start + offset;
}

/// Chunks coming near the viewport draw their rows; those leaving keep
/// only their height.
export function onChunks(v: Viewer, entries: IntersectionObserverEntry[]) {
  const top = v.files.getBoundingClientRect().top;
  const changes = entries.flatMap(entry => {
    const c = v.chunks.get(entry.target);
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
    if (want) { c.comments = placed(v).files.get(c.path)?.lines ?? noComments; v.liveChunks.add(c); }
    else v.liveChunks.delete(c);
  }

  for (const { c, want } of changes) if (!want) forgetCommentsIn(v, c.box);
  if (changes.length) drawChunks(v);
  for (const { c, want } of changes) if (!want) c.box.replaceChildren();
  let shift = 0;

  for (const { c, want, rect, spans } of changes) {
    if (want && spans && rect.bottom <= top) shift += c.box.getBoundingClientRect().height - rect.height;
  }

  // Even assigning the same scrollTop stops WebKit's trackpad momentum.
  if (shift) v.files.scrollTop += shift;
  markCurrent(v);
}

function drawToolbar(v: Viewer) {
  render(v, "toolbar", createPortal(createElement(DiffToolbar, {
    view: v.view, loading: v.loading, filter: v.filter, split: v.split, uncommitted: v.uncommitted,
    showUncommitted: !!v.src.uncommitted, allFolded: allFolded(shown(v), v.folded), flash: v.flashText,
    comments: v.draft.length, sending: v.sending, readOnly: !!v.src.readOnly, feedback: v.feedback.el,
    onFilter: value => { v.filter = value; applyFilter(v); v.files.scrollTop = 0; remember(v); },
    onSplit: () => { v.split = !v.split; saveSplit(v.split); drawToolbar(v); redrawBodies(v); },
    onUncommitted: () => { v.uncommitted = !v.uncommitted; void load(v); },
    onFold: () => foldAll(v, !allFolded(shown(v), v.folded)), onRefresh: () => void load(v), onSend: () => void sendReview(v),
  }), v.head, "toolbar"));
}

export function drawHead(v: Viewer) {
  drawToolbar(v);
  v.feedback.redraw();
}

/// Shows `text` in the header for two seconds.
export function say(v: Viewer, text: string) {
  if (v.disposed) return;
  v.flashText = text;
  drawToolbar(v);
  clearTimeout(v.flashTimer);
  v.flashTimer = window.setTimeout(() => { v.flashText = ""; drawToolbar(v); }, 2000);
}

function drawStatus(v: Viewer) {
  render(v, "status", createElement(Fragment, null,
    createPortal(v.error ? createElement("div", { className: "dv-err" }, v.error) : null, v.errorHost, "error"),
    createPortal(createElement(DiffMessage, { loading: v.loading, view: v.view }), v.messageHost, "message"),
    createPortal(createElement(DiffMore, { count: v.view?.more ?? 0 }), v.moreHost, "more"),
    createPortal(createElement(DiffKeys), v.keys, "keys"),
  ));
}

export function drawTree(v: Viewer) {
  const view = createElement(DiffTree, {
    files: v.view?.files ?? [], filter: v.filter, current: v.cur, closedFolders: v.closedFolders, filteredClosedFolders: v.filteredClosedFolders,
    scroller: v.tree, reveal: v.revealTree,
    onFolder: (path, open) => {
      const closed = v.filter.trim() ? v.filteredClosedFolders : v.closedFolders;
      if (open) closed.add(path); else closed.delete(path);
      drawTree(v);
    },
    onFile: path => jump(v, path), onMenu: (event, file) => fileMenu(v, event.nativeEvent, file),
  });
  render(v, "tree", createPortal(view, v.tree, "tree"));
}

/// Hides the files that do not match, without drawing any again.
function applyFilter(v: Viewer) {
  const view = v.view;
  if (!view) return;

  for (const [path, section] of v.sections) {
    const f = v.list[v.indexOf.get(path) ?? -1];
    section.hidden = !!f && !matchesFilter(f, v.filter);
  }

  if (v.laidOutFilter !== v.filter) {
    v.laidOutFilter = v.filter;
    v.filteredClosedFolders.clear();
    remeasure(v);
  }

  v.matchNote.hidden = !v.filter.trim();
  v.matchNote.textContent = `${shown(v).length} of ${view.files.length} files match`;
  markCurrent(v, false);
  drawTree(v);
  drawToolbar(v);
}

/// The right-click menu of a file, on its header or its row in the list.
function fileMenu(v: Viewer, e: MouseEvent, f: DiffFile) {
  const { src } = v;
  const items: MenuItem[] = [{ label: "Copy path", on: () => void copy(v, f.path) }];

  if (src.open && f.status !== "deleted") {
    const open = src.open;
    items.unshift({ label: open.label, on: () => open.run(f.path) });
  }

  const fuller = openMenu(e, f.path, items);
  if (!src.menu || f.status === "deleted") return;
  src.menu(f.path, { x: e.clientX, y: e.clientY }).then(more => { if (more.length) fuller([...items, ...more]); }, () => {});
}

async function copy(v: Viewer, path: string) {
  try {
    await navigator.clipboard.writeText(path);
    say(v, "Copied the path");
  } catch (e) {
    say(v, `Could not copy: ${reasonOf(e)}`);
  }
}

function fileSection(v: Viewer, f: DiffFile) {
  const folded = isFolded(f, v.folded);
  const section = el("section", `dv-file${folded ? " closed" : ""}${v.cur === f.path ? " cur" : ""}`);
  section.dataset.path = f.path;
  section.hidden = !matchesFilter(f, v.filter);
  // Comments on the file itself sit outside the fold, so a folded, deleted or binary file shows them.
  const fcs = el("div", "dv-fcs");
  const mine = placed(v).files.get(f.path);
  if (mine) fcs.append(...[...mine.file, ...mine.top].map(c => commentHost(v, c, lineLabel(c), false)));
  const fh = el("div");
  fh.style.display = "contents";
  v.headers.set(f.path, { host: fh, file: f, comments: fcs });
  const inner = el("div");
  // The lines are drawn the first time the file opens.
  if (!folded) inner.append(fileBody(v, f));
  section.append(fh, fcs, el("div", "dv-fold", inner));
  return section;
}

function fileBody(v: Viewer, f: DiffFile): HTMLElement {
  const split = v.split && !v.narrow;
  v.bodyModes.set(f.path, split);
  const note = leftOutNote(f);

  if (note) {
    return bodyControl(v, createElement(DiffOmitted, { text: note.text, label: note.load, busy: v.pending.has(f.path), load: () => void loadFull(v, f) }));
  }

  const lines = el("div", split ? "dv-lines split" : "dv-lines");
  lines.style.setProperty("--nw", String(Math.max(2, numberWidth(f))));
  const rows = diffRows(f);
  // The widest line is known up front, so the sideways scroll does not
  // change as rows come and go.
  lines.style.setProperty("--len", String(rows.reduce((m, r) => (r.kind === "line" ? Math.max(m, r.text.length) : m), 0)));
  // Rows are drawn in chunks, only while they are near the viewport; a
  // chunk off screen is an empty box as tall as it last was.
  const cols = Math.floor(((v.files.clientWidth || 900) / 2 - Math.max(2, numberWidth(f)) * CH - 50) / CH);
  const items: (DiffRow | SplitRow)[] = split ? splitRows(rows) : rows;

  for (let i = 0; i < items.length; i += CHUNK) {
    const slice = items.slice(i, i + CHUNK);
    const box = el("div", "dv-chunk");
    box.style.height = `${split ? splitHeight(slice as SplitRow[], cols) : slice.reduce((h, r) => h + (r.kind === "gap" ? 22 : 19), 0)}px`;
    v.chunks.set(box, { box, id: ++v.nextChunk, path: f.path, rows: slice, comments: noComments, live: false });
    v.chunkObserver.observe(box);
    lines.append(box);
  }

  const more = (direction: "above" | "below") => bodyControl(v, createElement(DiffContext, { direction, expand: () => void expandContext(v, f, direction) }));
  lines.prepend(more("above"));
  lines.append(more("below"));

  return el("div", "dv-scroll", lines);
}

function drawChunks(v: Viewer) {
  const portals = [...v.liveChunks].map(chunk => createPortal(createElement(DiffChunk, {
    rows: chunk.rows, comments: chunk.comments, mountComment: v.mountComment, unmountComment: v.unmountComment,
  }), chunk.box, String(chunk.id)));
  render(v, "chunks", createElement(Fragment, null, ...portals));
  drawComments(v);
}

/// Stop observing chunks removed by a file or mode change.
function pruneChunks(v: Viewer) {
  let removed = false;

  for (const [box, chunk] of v.chunks) {
    if (v.root.contains(box)) continue;
    v.chunkObserver.unobserve(box);
    forgetCommentsIn(v, box);
    if (v.liveChunks.delete(chunk)) removed = true;
    v.chunks.delete(box);
  }

  if (removed) drawChunks(v);
}

function countLabel(v: Viewer, path: string) {
  const n = v.draft.filter(c => c.path === path).length;
  return n ? `${n} ${n === 1 ? "comment" : "comments"}` : "";
}

function drawHeaders(v: Viewer) {
  const portals = [...v.headers.entries()].map(([path, item]) => createPortal(createElement(DiffFileHeader, {
    file: item.file, count: countLabel(v, path), readOnly: !!v.src.readOnly, openLabel: v.src.open?.label ?? null,
    folded: isFolded(item.file, v.folded),
    onToggle: select => { if (select) v.cur = path; toggle(v, path); if (select) markCurrent(v); },
    onMenu: event => fileMenu(v, event.nativeEvent, item.file),
    onComment: () => addBox(v, { path, side: "new", line: null, text: "" }, box => item.comments.append(box)),
    onCopy: () => void copy(v, path), onOpen: () => v.src.open?.run(path),
  }), item.host, path));
  render(v, "headers", createElement(Fragment, null, ...portals));
  drawBodyControls(v);
}

function bodyControl(v: Viewer, view: ReactNode) {
  const host = el("div");
  host.style.display = "contents";
  v.bodyControls.set(host, { id: ++v.nextBodyControl, view });
  v.bodyControlsDirty = true;
  return host;
}

function drawBodyControls(v: Viewer) {
  for (const host of v.bodyControls.keys()) if (!v.root.contains(host)) { v.bodyControls.delete(host); v.bodyControlsDirty = true; }
  if (!v.bodyControlsDirty) return;
  v.bodyControlsDirty = false;
  render(v, "body-controls", createElement(Fragment, null, ...[...v.bodyControls].map(([host, item]) => createPortal(item.view, host, String(item.id)))));
}

/// The header's count and Send button, each folded file's count and the
/// comments on files the diff no longer has.
export function drawReview(v: Viewer) {
  drawToolbar(v);
  drawHeaders(v);
  drawLostComments(v);
  // The lost comments sit above the list, which then starts lower.
  if (v.win.isConnected) placeFiles(v);
}

function updateBodyMode(v: Viewer, path: string) {
  if (v.bodyModes.get(path) === (v.split && !v.narrow)) return;
  const section = v.sections.get(path), f = v.renderedFiles.get(path);
  const inner = section?.querySelector(".dv-fold > div");
  if (!f || !inner || isFolded(f, v.folded)) return;
  inner.replaceChildren(fileBody(v, f));
}

/// The drawn files change modes now; the others are laid out again from
/// estimates and draw in the new mode as they come into view.
export function redrawBodies(v: Viewer) {
  keepCurrent(v, () => {
    for (const path of v.sections.keys()) updateBodyMode(v, path);
    pruneChunks(v);
    drawBodyControls(v);
    remeasure(v);
  });
}

function setFolded(v: Viewer, path: string, folded: boolean, redraw = true) {
  v.folded.set(path, folded);
  if (redraw) remember(v);
  const section = v.sections.get(path);
  const f = v.renderedFiles.get(path) ?? v.list[v.indexOf.get(path) ?? -1];
  if (!f) return;

  if (!section) {
    if (redraw) v.virtual.resizeItem(v.indexOf.get(path) ?? -1, sizeOf(v, f));
    return;
  }

  const inner = section.querySelector(".dv-fold > div");
  if (!folded && inner && !inner.childNodes.length) inner.append(fileBody(v, f));
  if (!folded) updateBodyMode(v, path);
  section.classList.toggle("closed", folded);
  if (redraw) { drawHeaders(v); drawToolbar(v); }
}

export function toggle(v: Viewer, path: string) {
  const f = v.view?.files.find(x => x.path === path);
  if (!f) return;
  const folding = !isFolded(f, v.folded);
  // Folding a file scrolled past its header keeps its header in view.
  const item = itemAt(v, v.indexOf.get(path));
  if (folding && item && item.start < v.files.scrollTop) v.files.scrollTop = item.start - GAP;
  setFolded(v, path, folding);
}

function foldAll(v: Viewer, folded: boolean) {
  keepCurrent(v, () => {
    for (const f of shown(v)) setFolded(v, f.path, folded, false);
    remeasure(v);
  });
  remember(v);
  drawHeaders(v);
  drawToolbar(v);
  if (folded) v.files.scrollTop = 0;
}
