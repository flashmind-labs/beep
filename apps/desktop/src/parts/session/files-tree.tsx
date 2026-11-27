import { Icon } from "../../ui/icons.tsx";
import { Checkbox, NO_AUTO, Spinner } from "../../ui/kit.tsx";
import { lazy, memo, Suspense, useCallback, useLayoutEffect, useMemo, useRef, type CSSProperties } from "react";
import { observeElementRect, useVirtualizer, type Rect, type Virtualizer } from "@tanstack/react-virtual";
import { useStore } from "zustand";
import type { GitChange, TreeRow } from "../../stores/agentpane/files-state.ts";
import { type TreeView, filesMenuRef, filesModel, filesRef, keepScroll, keptScroll, refreshFiles, scrollAgain, scrolledHere, restoreScroll, revealed, rowId, setFilter, setIgnored, treeActions, treeRef, WALK_ENTRIES } from "./files.ts";

// The menu's library loads with the first right-click, as other menus do.
const FilesMenu = lazy(() => import("./files-menu.tsx").then(module => ({ default: module.FilesMenu })));

const marks: Record<GitChange, [string, string]> = { untracked: ["U", "Untracked"], added: ["A", "Added"], modified: ["M", "Changed"], conflicted: ["C", "In conflict"] };
const depth = (value: number): CSSProperties & { "--d": number } => ({ "--d": value });

/// The Files tab: the filter, the tree and its menu, and the footer.
export const FilesView = memo(function FilesView({ hidden }: { hidden: boolean }) {
  const m = useStore(filesModel);

  // A tree drawn again, in the other dock or with its session, goes back where it was left.
  useLayoutEffect(() => scrollAgain, []);

  return <div className="agpview files" hidden={hidden} ref={filesRef}>
    <div className="agfbar"><input className="agffind" value={m.filter} placeholder="Filter files" aria-label="Filter files" {...NO_AUTO}
      onChange={event => setFilter(event.currentTarget.value)}
      onKeyDown={event => { if (event.key === "Escape" && event.currentTarget.value) { event.stopPropagation(); event.preventDefault(); setFilter(""); } }} />
      <label className="agfign"><Checkbox checked={m.ignored} change={setIgnored} /> Show ignored</label>
      <button className="agpbtn" data-frefresh="1" title="Read the folders again" onClick={refreshFiles}>↻</button></div>
    <div ref={treeRef} className={`agtree${m.drop === "" ? " dropping" : ""}`} role="tree" aria-label="Working files" aria-multiselectable="true" aria-busy={m.busy} aria-activedescendant={m.active ?? undefined} tabIndex={0}
      onScroll={keepScroll} onWheel={scrolledHere} onPointerDown={scrolledHere} onKeyDownCapture={scrolledHere} onClick={treeActions.click} onContextMenu={treeActions.contextMenu} onKeyDown={treeActions.keyDown}
      onDragStart={treeActions.dragStart} onDragOver={treeActions.dragOver} onDragLeave={treeActions.dragLeave} onDrop={treeActions.drop}>
      {m.tree && <FilesTree tree={m.tree} selected={m.selected} drop={m.drop} reveal={m.reveal} />}
    </div>
    <div className="agpacts"><span className="meta mono" title={m.root}>{m.label || m.root}</span><span data-fstatus role="status">{m.status}</span><span className="sp" /><span className="hint">Cmd-click to select, Shift-click for a range. Cmd+Delete asks before deleting.</span></div>
    {m.menu && <Suspense fallback={null}><FilesMenu {...m.menu} menuRef={filesMenuRef} /></Suspense>}
  </div>;
});

function FilesTree({ tree, selected, drop, reveal }: { tree: TreeView; selected: ReadonlySet<string>; drop: string | null; reveal: string | null }) {
  if (tree.kind === "message") return <div className={`agpmsg${tree.error ? " derr" : ""}`}>{tree.loading && <Spinner />}{tree.text}</div>;
  return <TreeLines tree={tree} selected={selected} drop={drop} reveal={reveal} />;
}

/// A line of the tree: a file or folder, or a note under one or under all.
type Line = { kind: "row"; row: TreeRow } | { kind: "note"; key: string; depth: number; text: string; error: boolean; loading: boolean };

function treeLines(tree: Extract<TreeView, { kind: "rows" }>): Line[] {
  const lines: Line[] = [];

  for (const row of tree.rows) {
    lines.push({ kind: "row", row });
    if (row.error) lines.push({ kind: "note", key: `error:${row.path}`, depth: row.depth + 1, text: row.error, error: true, loading: false });
    else if (row.more) lines.push({ kind: "note", key: `more:${row.path}`, depth: row.depth + 1, text: `${row.more} more not shown`, error: false, loading: false });
  }

  if (tree.more) lines.push({ kind: "note", key: "more", depth: 0, text: `${tree.more} more not shown`, error: false, loading: false });
  if (tree.walking) lines.push({ kind: "note", key: "walking", depth: 0, text: "Reading folders…", error: false, loading: true });
  if (tree.capped) lines.push({ kind: "note", key: "capped", depth: 0, text: `Stopped reading at ${WALK_ENTRIES.toLocaleString()} files`, error: false, loading: false });
  return lines;
}

/// The tree's lines, only those in view and a few around them drawn.
function TreeLines({ tree, selected, drop, reveal }: { tree: Extract<TreeView, { kind: "rows" }>; selected: ReadonlySet<string>; drop: string | null; reveal: string | null }) {
  const lines = useMemo(() => treeLines(tree), [tree]);
  const rows = useRef<HTMLDivElement>(null);

  // The tree goes back where it was left before it paints: as it draws another
  // folder, and once a tree drawn hidden has a size and its rows. The
  // virtualizer hears of the move before the frame paints.
  useLayoutEffect(restore, [lines]);
  useLayoutEffect(() => {
    const watch = new ResizeObserver(restore);
    for (const element of [treeRef.current, rows.current]) if (element) watch.observe(element);
    return () => watch.disconnect();
  }, []);
  // A key function kept while the lines stay, so the virtualizer keeps its measurements.
  const getItemKey = useCallback((index: number) => {
    const line = lines[index];
    return line.kind === "row" ? line.row.path : line.key;
  }, [lines]);
  const virtual = useVirtualizer({
    count: lines.length, getScrollElement: () => treeRef.current, estimateSize: () => ROW_HEIGHT, overscan: 12, getItemKey, initialRect: shownRect, initialOffset: keptScroll, observeElementRect: shownSize,
    // Rows take the size the ResizeObserver reports before they paint; a
    // hidden row reports none and keeps the size it had.
    measureElement: (node, entry, instance) => entry?.borderBoxSize?.[0]?.blockSize || instance.measurementsCache[instance.indexFromElement(node)]?.size || ROW_HEIGHT,
  });

  // A row asked for comes into view once the tree has it.
  useLayoutEffect(() => {
    if (!reveal) return;
    const index = lines.findIndex(line => line.kind === "row" && line.row.path === reveal);
    if (index < 0) return;
    virtual.scrollToIndex(index, { align: "auto" });
    revealed();
  }, [reveal, lines]);

  return <div ref={rows} className="agtrows" style={{ height: virtual.getTotalSize() }}>{virtual.getVirtualItems().map(item => {
    const line = lines[item.index];
    if (line.kind === "row") return <FileRow key={item.key} row={line.row} index={item.index} start={item.start} on={selected.has(line.row.path)} drop={drop === line.row.path} measure={virtual.measureElement} />;
    return <div key={item.key} ref={virtual.measureElement} data-index={item.index} className={`agtr note${line.error ? " derr" : ""}`} style={place(line.depth, item.start)}>{line.loading && <Spinner />}{line.text}</div>;
  })}</div>;
}

/// The tree's size when it last showed. A tree drawn hidden keeps drawing
/// the rows it would show, so they are there the frame it shows.
let shownRect: Rect = { width: 0, height: 600 };
const shownSize = (instance: Virtualizer<HTMLDivElement, Element>, report: (rect: Rect) => void) => observeElementRect(instance, rect => {
  if (!rect.height) return;
  shownRect = rect;
  report(rect);
});

function restore() {
  const tree = treeRef.current;
  if (tree && restoreScroll(tree)) queueMicrotask(() => tree.dispatchEvent(new Event("scroll")));
}

/// The height a line is taken to have until it is measured.
const ROW_HEIGHT = 21;

const place = (d: number, start: number) => ({ ...depth(d), transform: `translateY(${start}px)` });

interface FileRowProps { row: TreeRow; index: number; start: number; on: boolean; drop: boolean; measure: (node: HTMLElement | null) => void }

/// One file or folder. A refresh draws again only the rows it changed.
const FileRow = memo(function FileRow({ row, index, start, on, drop, measure }: FileRowProps) {
  return <div ref={measure} data-index={index} className={`agtr${on ? " on" : ""}${row.ignored ? " ign" : ""}${drop ? " drop" : ""}`} role="treeitem" aria-selected={on} id={rowId(row.path)} style={place(row.depth, start)} draggable data-fpath={row.path} data-ffolder={row.folder ? "1" : undefined} aria-expanded={row.folder ? row.open : undefined} title={row.path}>
    <span className={`tw${row.folder && row.open ? " open" : ""}`}>{row.folder && (row.loading ? <Spinner /> : <Icon name="twist" />)}</span>
    {row.folder ? <Icon name="paneFiles" /> : <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M14 3H6v18h12V7z" /><path d="M14 3v4h4" /></svg>}
    <span className="nm">{row.name}</span>{row.change && <span className={`gm ${row.change}`} title={marks[row.change][1]}>{marks[row.change][0]}</span>}
  </div>;
}, (a, b) => a.index === b.index && a.start === b.start && a.on === b.on && a.drop === b.drop && a.measure === b.measure && sameRow(a.row, b.row));

/// Whether two reads give a row the same look: each read makes new rows.
const sameRow = (a: TreeRow, b: TreeRow) => (Object.keys(a) as (keyof TreeRow)[]).every(key => a[key] === b[key]);
