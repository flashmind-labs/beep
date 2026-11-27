// The diff's files: each folding under its sticky header, its lines in a
// unified or side by side view that scrolls sideways on its own. Only the
// files near the viewport have sections, laid out by a TanStack virtualizer;
// within a section, only the chunks of rows near the viewport draw their rows.
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { flushSync } from "react-dom";
import { useStore } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import { writeClipboard } from "../../ui/clipboard.ts";
import { plural } from "../../ui/format.ts";
import { DiffFileHeader } from "./diff-file-header.tsx";
import { DiffContext, DiffOmitted } from "./diff-body-controls.tsx";
import { DiffRows, type Look } from "./diff-chunk.tsx";
import type { Span } from "../../ui/syntax.ts";
import { DiffComment } from "./diff-comment.tsx";
import { DiffMessage, DiffMore } from "./diff-status.tsx";
import {
  type ContextEdit, type DiffFile, type DiffRow, type PlacedFile, type ReviewComment, type SplitRow, commentCounts, diffRows, diffSpans, isFolded, leftOutNote, lineLabel, matchesFilter,
  numberWidth, placeComments, splitHeight, splitRows,
} from "../../stores/review/diff-state.ts";
import { CH, CHUNK, GAP, MARGIN, type Viewer, indexOf, reasonOf, remember, say, selected, shown } from "../../stores/review/diff-viewer.ts";
import { commentKey, openEditor } from "../../stores/review/diff-comments.ts";
import { markCurrent, sizeOf, toggle } from "../../stores/review/diff-keys.ts";
import { editContext, loadFull } from "../../stores/review/diff-load.ts";
import { openMenu, type MenuItem } from "../../ui/menu.tsx";

type CssVars = CSSProperties & Record<`--${string}`, string>;

export const DiffFiles = memo(function DiffFiles({ v }: { v: Viewer }) {
  const s = useStore(v.store, useShallow(s => ({
    view: s.view, list: s.list, loading: s.loading, error: s.error, filter: s.filter, folded: s.folded, cur: s.cur,
    split: s.split && !s.narrow, pending: s.pending, draft: s.draft, orphaned: s.orphaned,
  })));
  const fresh = useStore(v.store, useShallow(s => [...s.editors].flatMap(([comment, isNew]) => isNew ? [comment] : [])));
  const { list, filter, folded } = s;
  const [files, setFiles] = useState<HTMLDivElement | null>(null);
  const win = useRef<HTMLDivElement>(null);
  const [margin, setMargin] = useState(0);

  const placed = useMemo(() => {
    const out = placeComments(list, [...s.draft, ...fresh].filter(comment => !s.orphaned.has(comment)));
    out.lost.push(...s.orphaned);
    return out;
  }, [list, s.draft, fresh, s.orphaned]);
  const counts = useMemo(() => commentCounts(s.draft), [s.draft]);

  // The files near the viewport, and the drawn ones that must stay: the
  // focus, a selection or an open comment would be lost with them.
  const keepHeld = useCallback((range: Range) => {
    const out = defaultRangeExtractor(range);
    const extra: number[] = [];

    for (const [path, section] of v.sections) {
      const index = indexOf(v, path);
      if (index < 0 || (index >= out[0] && index <= out[out.length - 1])) continue;
      if (section.contains(document.activeElement) || section.querySelector(".dv-cmt.open") || selected(section)) extra.push(index);
    }

    return extra.length ? [...new Set([...out, ...extra])].sort((a, b) => a - b) : out;
  }, [v]);
  // Stable while the list and its folds stay, so the laid out sizes are not worked out again on every frame.
  const estimateSize = useCallback((index: number) => sizeOf(list[index], filter, folded), [list, filter, folded]);
  const getItemKey = useCallback((index: number) => list[index]?.path ?? index, [list]);
  const virtual = useVirtualizer<HTMLDivElement, HTMLElement>({
    count: list.length,
    getScrollElement: () => files,
    estimateSize,
    getItemKey,
    measureElement: (node, entry) => node.hidden ? 0 : Math.round(entry?.borderBoxSize?.[0]?.blockSize ?? node.offsetHeight) + GAP,
    rangeExtractor: keepHeld,
    overscan: 2,
    paddingStart: GAP,
    scrollPaddingStart: GAP,
    scrollMargin: margin,
    useAnimationFrameWithResizeObserver: true,
  });
  // Only a file wholly above the viewport moves the scroll as it is measured;
  // the chunks keep the file at the top steady themselves.
  virtual.shouldAdjustScrollPositionOnItemSizeChange = item => item.end <= (files?.scrollTop ?? 0);
  v.virtual = virtual;
  const items = virtual.getVirtualItems();
  const order = useSectionOrder(v, items.map(item => list[item.index]));

  // The list starts below the lost comments and notes, which change with the
  // state read here, as a lost comment's editor grows, or with the width.
  // Not on every render: that would lay the page out on every scroll.
  const lost = useRef<HTMLDivElement>(null);
  const hasList = list.length > 0;
  useLayoutEffect(() => { setMargin(win.current?.offsetTop ?? 0); }, [s.error, s.loading, s.view, filter, placed.lost, hasList]);
  useLayoutEffect(() => {
    const box = lost.current;
    if (!files || !box) return;
    const observer = new ResizeObserver(() => setMargin(win.current?.offsetTop ?? 0));
    observer.observe(files);
    observer.observe(box);
    return () => observer.disconnect();
  }, [files]);

  // A file whose content changed while it was not drawn is laid out by its estimate again.
  const laidOut = useRef(list);
  useLayoutEffect(() => {
    const before = new Map(laidOut.current.map(file => [file.path, file]));
    laidOut.current = list;
    list.forEach((file, index) => {
      const old = before.get(file.path);
      if (old && old !== file && !v.sections.has(file.path)) virtual.resizeItem(index, sizeOf(file, filter, folded));
    });
    markCurrent(v, false);
  }, [list]);

  useFilesEvents(v, files);

  return <div className="dv-files" ref={setFiles} style={{ position: "relative" }}>
    <div style={{ display: "contents" }}>{s.error && <div className="dv-err">{s.error}</div>}</div>
    <div className="dv-lost" ref={lost}>{s.view && <LostComments v={v} lost={placed.lost} />}</div>
    <div style={{ display: "contents" }}><DiffMessage loading={s.loading} view={s.view} /></div>
    {hasList && <>
      <div className="dv-more" hidden={!filter.trim()}>{`${shown(s).length} of ${s.view?.files.length ?? 0} files match`}</div>
      <div className="dv-win" ref={win} style={{ height: `${virtual.getTotalSize()}px` }}>
        {order.map(({ file, key }) => {
          const index = list.indexOf(file), item = items.find(item => item.index === index);
          if (!item) return null;
          return <FileSection key={key} v={v} file={file} index={index} top={item.start - margin} folded={isFolded(file, folded)}
            cur={s.cur === file.path} hidden={!matchesFilter(file, filter)} split={s.split} busy={s.pending.has(file.path)}
            count={counts.get(file.path) ?? 0} placed={placed.files.get(file.path) ?? null} />;
        })}
      </div>
      <div style={{ display: "contents" }}><DiffMore count={s.view?.more ?? 0} /></div>
    </>}
  </div>;
});

/// The comments on files or lines the diff no longer has, above the files.
function LostComments({ v, lost }: { v: Viewer; lost: readonly ReviewComment[] }) {
  if (!lost.length) return null;
  return <>
    <div className="dv-lh">On files or lines no longer in the changes</div>
    {lost.map(comment => <CommentSlot key={commentKey(comment)} v={v} comment={comment} label={`${comment.path} ${lineLabel(comment)}`.trim()} />)}
  </>;
}

/// The sections in the order React keeps them: each new one after the
/// others, so React never moves one that holds the focus or a selection. The
/// layout effect then puts them in list order around such a section, so a
/// selection reads in order.
function useSectionOrder(v: Viewer, wanted: readonly DiffFile[]) {
  const previous = useRef<{ file: DiffFile; key: string }[]>([]);
  const want = new Set(wanted);
  const kept = previous.current.filter(entry => want.has(entry.file));
  const have = new Set(kept.map(entry => entry.file));
  const order = [...kept, ...wanted.filter(file => !have.has(file)).map(file => ({ file, key: sectionKey(file) }))];
  previous.current = order;

  useLayoutEffect(() => {
    const ordered = wanted.flatMap(file => v.sections.get(file.path) ?? []);
    const parent = ordered[0]?.parentElement;
    if (!parent) return;
    const anchor = ordered.findIndex(section => section.contains(document.activeElement) || selected(section));

    if (anchor >= 0) {
      let before: Node = ordered[anchor];
      for (let i = anchor - 1; i >= 0; i--) {
        if (ordered[i].nextSibling !== before) parent.insertBefore(ordered[i], before);
        before = ordered[i];
      }
    }

    let cursor = anchor >= 0 ? ordered[anchor].nextSibling : parent.firstChild;
    for (const section of ordered.slice(anchor + 1)) {
      if (section === cursor) cursor = cursor.nextSibling;
      else parent.insertBefore(section, cursor);
    }
  });

  return order;
}

const versions = new WeakMap<DiffFile, number>();
let nextVersion = 0;

/// A section's key: a file drawn again with other content is a new section.
function sectionKey(file: DiffFile) {
  let version = versions.get(file);
  if (version === undefined) versions.set(file, version = ++nextVersion);
  return `${file.path}\n${version}`;
}

/// The list's scrolling: the position kept, the current file marked, and
/// the chunks that draw their rows as they come near.
function useFilesEvents(v: Viewer, files: HTMLDivElement | null) {
  useLayoutEffect(() => {
    if (!files) return;
    v.files = files;
    let frame = 0;
    const scroll = () => {
      // Scrolling writes the position at most a few times a second.
      if (!v.rememberTimer) v.rememberTimer = window.setTimeout(() => remember(v), 250);
      if (!frame) frame = requestAnimationFrame(() => { frame = 0; markCurrent(v); });
    };
    files.addEventListener("scroll", scroll, { passive: true });
    const resize = new ResizeObserver(() => markCurrent(v));
    resize.observe(files);
    const observer = new IntersectionObserver(entries => onChunks(v, entries), { root: files, rootMargin: `${MARGIN}px 0px` });
    v.chunkObserver = observer;
    for (const chunk of v.chunks.keys()) observer.observe(chunk);

    return () => {
      files.removeEventListener("scroll", scroll);
      if (frame) cancelAnimationFrame(frame);
      resize.disconnect();
      observer.disconnect();
      if (v.chunkObserver === observer) v.chunkObserver = null;
      if (v.files === files) v.files = null;
    };
  }, [v, files]);
}

/// Chunks coming near the viewport draw their rows; those leaving keep only
/// their height. A chunk drawn above the viewport in the file at the top
/// moves the scroll by what it grew, so what shows stays put.
function onChunks(v: Viewer, entries: IntersectionObserverEntry[]) {
  const files = v.files;
  if (!files) return;
  const top = files.getBoundingClientRect().top;
  const changes = entries.flatMap(entry => {
    const chunk = v.chunks.get(entry.target), box = entry.target;
    if (!chunk || !box.isConnected || chunk.live === entry.isIntersecting) return [];
    if (!entry.isIntersecting && (box.querySelector(".dv-cmt.open") || selected(box))) return [];
    // A file wholly above the viewport is kept steady by the virtualizer.
    const spans = (box.closest(".dv-file")?.getBoundingClientRect().bottom ?? 0) > top;
    return [{ chunk, box, want: entry.isIntersecting, rect: box.getBoundingClientRect(), spans }];
  });
  if (!changes.length) return;

  // Read all old heights before changing the DOM, then measure the new ones
  // together. Scrolling itself never walks the offscreen chunks.
  flushSync(() => {
    for (const { chunk, want, rect } of changes) {
      chunk.live = want;
      chunk.set({ live: want, height: rect.height });
    }
  });
  let shift = 0;

  for (const { box, want, rect, spans } of changes) {
    if (want && spans && rect.bottom <= top) shift += box.getBoundingClientRect().height - rect.height;
  }

  // Even assigning the same scrollTop stops WebKit's trackpad momentum.
  if (shift) files.scrollTop += shift;
  markCurrent(v);
}

interface SectionProps {
  v: Viewer;
  file: DiffFile;
  index: number;
  top: number;
  folded: boolean;
  cur: boolean;
  hidden: boolean;
  split: boolean;
  busy: boolean;
  count: number;
  placed: PlacedFile | null;
}

const FileSection = memo(function FileSection({ v, file, index, top, folded, cur, hidden, split, busy, count, placed }: SectionProps) {
  const path = file.path;
  // The lines are drawn the first time the file opens, and kept while it folds.
  const opened = useRef(!folded);
  if (!folded) opened.current = true;
  const ref = useMemo(() => {
    let mounted: HTMLElement | null = null;
    return (section: HTMLElement | null) => {
      if (section) v.sections.set(path, mounted = section);
      else if (mounted && v.sections.get(path) === mounted) v.sections.delete(path);
      v.virtual?.measureElement(section);
    };
  }, [v, path]);

  return <section ref={ref} className={`dv-file${folded ? " closed" : ""}${cur ? " cur" : ""}`} data-path={path} data-index={index}
    style={{ top: `${top}px` }} hidden={hidden} onPointerDown={() => { if (v.state.cur !== path) v.store.setState({ cur: path }); }}>
    <div style={{ display: "contents" }}>
      <DiffFileHeader file={file} count={count ? plural(count, "comment", "comments") : ""} readOnly={!!v.src.readOnly}
        openLabel={v.src.open?.label ?? null} folded={folded}
        onToggle={() => {
          v.store.setState({ cur: path });
          toggle(v, path);
        }}
        onMenu={event => fileMenu(v, event.nativeEvent, file)}
        onComment={() => openEditor(v, { path, side: "new", line: null, text: "" }, true)}
        onCopy={() => void copy(v, path)} onOpen={() => v.src.open?.run(path)} />
    </div>
    {/* Comments on the file itself sit outside the fold, so a folded, deleted or binary file shows them. */}
    <div className="dv-fcs">
      {placed && [...placed.file, ...placed.top].map(comment => <CommentSlot key={commentKey(comment)} v={v} comment={comment} label={lineLabel(comment)} />)}
    </div>
    <div className="dv-fold"><div>
      {opened.current && <FileBody key={split ? "split" : "unified"} v={v} file={file} split={split} busy={busy} comments={placed?.lines ?? NO_COMMENTS} />}
    </div></div>
  </section>;
});

const NO_COMMENTS: ReadonlyMap<string, readonly ReviewComment[]> = new Map();

interface BodyProps { v: Viewer; file: DiffFile; split: boolean; busy: boolean; comments: ReadonlyMap<string, readonly ReviewComment[]> }

const FileBody = memo(function FileBody({ v, file, split, busy, comments }: BodyProps) {
  const layout = useMemo(() => leftOutNote(file) ? null : bodyLayout(v, file, split), [v, file, split]);
  const note = leftOutNote(file);
  const spans = useSpans(file);
  const want = useStore(v.store, s => s.filter.trim().toLowerCase());
  const hit = useStore(v.store, s => s.hit?.path === file.path ? s.hit : null);
  const look = useMemo(() => ({ spans, want, hit }), [spans, want, hit]);

  if (note) {
    return <div style={{ display: "contents" }}>
      <DiffOmitted text={note.text} label={note.load} busy={busy} load={() => void loadFull(v, file)} />
    </div>;
  }

  if (!layout) return null;
  const extents = v.context.get(file.path)?.extents, last = file.hunks.length - 1;
  const edit = (edits: readonly ContextEdit[]) => void editContext(v, file, edits);

  return <div className="dv-scroll">
    <div className={split ? "dv-lines split" : "dv-lines"} style={layout.vars}>
      <div style={{ display: "contents" }}><DiffContext direction="above" hunk={0} expanded={!!extents?.[0]?.above} edit={edit} /></div>
      {layout.chunks.map((chunk, index) => <Chunk key={index} v={v} file={file} rows={chunk.rows} estimate={chunk.height} comments={comments} look={look} />)}
      <div style={{ display: "contents" }}><DiffContext direction="below" hunk={last} expanded={!!extents?.[last]?.below} edit={edit} /></div>
    </div>
  </div>;
});

/// The coloured spans of `file`'s lines, none until its language's parser
/// loads. The parsers load in their own chunks, on the first file shown.
function useSpans(file: DiffFile) {
  const [spans, setSpans] = useState(() => coloured.get(file) ?? NO_SPANS);

  useEffect(() => {
    const kept = coloured.get(file);
    if (kept) { setSpans(kept); return; }
    let live = true;

    void import("../../ui/syntax.ts").then(({ colourFor }) => colourFor(file.path)).then(colour => {
      const found = colour ? diffSpans(file, colour) : NO_SPANS;
      coloured.set(file, found);
      if (live) setSpans(found);
    }, () => {});

    return () => { live = false; };
  }, [file]);

  return spans;
}

const coloured = new WeakMap<DiffFile, ReadonlyMap<string, readonly Span[]>>();
const NO_SPANS: ReadonlyMap<string, readonly Span[]> = new Map();

/// A file's rows in chunks, each with the height it has before it draws.
function bodyLayout(v: Viewer, file: DiffFile, split: boolean) {
  const width = Math.max(2, numberWidth(file));
  const rows = diffRows(file, v.context.get(file.path)?.extents ?? null);
  // The widest line is known up front, so the sideways scroll does not
  // change as rows come and go.
  const len = rows.reduce((m, r) => (r.kind === "line" ? Math.max(m, r.text.length) : m), 0);
  const vars: CssVars = { "--nw": String(width), "--len": String(len) };
  const cols = Math.floor(((v.files?.clientWidth || 900) / 2 - width * CH - 50) / CH);
  const chunks: { rows: readonly (DiffRow | SplitRow)[]; height: number }[] = [];

  if (split) {
    const items = splitRows(rows);
    for (let i = 0; i < items.length; i += CHUNK) {
      const slice = items.slice(i, i + CHUNK);
      chunks.push({ rows: slice, height: splitHeight(slice, cols) });
    }
  } else {
    for (let i = 0; i < rows.length; i += CHUNK) {
      const slice = rows.slice(i, i + CHUNK);
      chunks.push({ rows: slice, height: slice.length * 19 });
    }
  }

  return { vars, chunks };
}

interface ChunkProps { v: Viewer; file: DiffFile; rows: readonly (DiffRow | SplitRow)[]; estimate: number; comments: ReadonlyMap<string, readonly ReviewComment[]>; look: Look }

/// A run of rows, drawn only while it is near the viewport; off screen it is
/// an empty box as tall as it last was.
const Chunk = memo(function Chunk({ v, file, rows, estimate, comments, look }: ChunkProps) {
  const [state, setState] = useState({ live: false, height: estimate });
  const box = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const node = box.current;
    if (!node) return;
    v.chunks.set(node, { live: false, set: setState });
    v.chunkObserver?.observe(node);
    return () => { v.chunkObserver?.unobserve(node); v.chunks.delete(node); };
  }, [v]);

  return <div ref={box} className="dv-chunk" style={state.live ? undefined : { height: `${state.height}px` }}>
    {state.live && <DiffRows rows={rows} comments={comments} look={look}
      gutter={(side, line) => openEditor(v, { path: file.path, side, line, text: "" }, true)}
      edit={edits => void editContext(v, file, edits)} readOnly={!!v.src.readOnly}
      comment={comment => <CommentSlot key={commentKey(comment)} v={v} comment={comment} label="" />} />}
  </div>;
});

function CommentSlot({ v, comment, label }: { v: Viewer; comment: ReviewComment; label: string }) {
  return <span className="dv-comment-slot" style={{ display: "contents" }}><DiffComment v={v} comment={comment} label={label} /></span>;
}

/// The right-click menu of a file, on its header or its row in the list.
export function fileMenu(v: Viewer, e: MouseEvent, f: DiffFile) {
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
    await writeClipboard(path);
    say(v, "Copied the path");
  } catch (e) {
    say(v, `Could not copy: ${reasonOf(e)}`);
  }
}
