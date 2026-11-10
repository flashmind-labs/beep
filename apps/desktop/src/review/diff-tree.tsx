import { useCallback, useLayoutEffect, useMemo, type MouseEvent } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { compactFolders, grouped, matchesFilter, splitPath, STATUS_LETTER, type DiffFile } from "./diff-state.ts";

interface Folder {
  kind: "folder";
  path: string;
  name: string;
  children: TreeItem[];
}
interface FileItem { kind: "file"; file: DiffFile; name: string }
type TreeItem = Folder | FileItem;

/// One line of the list: a folder or a file, indented by its depth.
type Row = { kind: "folder"; folder: Folder; open: boolean; depth: number } | { kind: "file"; item: FileItem; depth: number };

interface DiffTreeProps {
  files: readonly DiffFile[];
  filter: string;
  current: string | null;
  closedFolders: ReadonlySet<string>;
  filteredClosedFolders: ReadonlySet<string>;
  /// The element the list scrolls in.
  scroller: HTMLElement;
  /// Changes when the current file's row should be scrolled into view.
  reveal: number;
  onFolder: (path: string, open: boolean) => void;
  onFile: (path: string) => void;
  onMenu: (event: MouseEvent<HTMLButtonElement>, file: DiffFile) => void;
}

function groupFiles(files: readonly DiffFile[]): TreeItem[] {
  const root: TreeItem[] = [];
  const folders = new Map<string, Folder>();
  const labels = compactFolders(files.map(file => file.path));
  function folder(path: string): TreeItem[] {
    if (!path) return root;
    const existing = folders.get(path);
    if (existing) return existing.children;
    const { dir, name } = labels.get(path) ?? splitPath(path.slice(0, -1));
    const group: Folder = { kind: "folder", path, name, children: [] };
    folder(dir).push(group);
    folders.set(path, group);
    return group.children;
  }
  for (const file of files) {
    const { dir, name } = splitPath(file.path);
    folder(dir).push({ kind: "file", file, name });
  }
  return root;
}

/// The rows that show: matching files, and folders holding one, with the
/// children of closed folders left out. Says whether any file matched.
function flatten(items: readonly TreeItem[], filter: string, open: (path: string) => boolean, depth: number, out: Row[]): boolean {
  let any = false;

  for (const item of items) {
    if (item.kind === "file") {
      if (!matchesFilter(item.file, filter)) continue;
      out.push({ kind: "file", item, depth });
      any = true;
      continue;
    }

    const isOpen = open(item.path);
    const children: Row[] = [];
    if (!flatten(item.children, filter, open, depth + 1, children)) continue;
    out.push({ kind: "folder", folder: item, open: isOpen, depth });
    if (isOpen) for (const child of children) out.push(child);
    any = true;
  }

  return any;
}

export function DiffTree(props: DiffTreeProps) {
  const items = useMemo(() => groupFiles(props.files), [props.files]);
  const closed = props.filter.trim() ? props.filteredClosedFolders : props.closedFolders;
  // Worked out again only when they change: scrolling the files marks the
  // current one, which draws the list again.
  const rows = useMemo(() => {
    const out: Row[] = [];
    flatten(items, props.filter, path => !closed.has(path), 0, out);
    return out;
  }, [items, props.filter, closed]);
  // A key function kept while the rows stay, so the virtualizer keeps its measurements.
  const getItemKey = useCallback((index: number) => {
    const row = rows[index];
    return row.kind === "folder" ? `d:${row.folder.path}` : `f:${row.item.file.path}`;
  }, [rows]);
  const virtual = useVirtualizer({
    count: rows.length, getScrollElement: () => props.scroller, estimateSize: () => 22, overscan: 12, useFlushSync: false, getItemKey,
    // Rows take the size the ResizeObserver reports before they paint;
    // measuring each one as it mounts would lay the page out once per row.
    measureElement: (node, entry, instance) => entry?.borderBoxSize?.[0]?.blockSize ?? instance.measurementsCache[instance.indexFromElement(node)]?.size ?? 22,
  });

  useLayoutEffect(() => {
    if (!props.reveal) return;
    const index = rows.findIndex(row => row.kind === "file" && row.item.file.path === props.current);
    if (index >= 0) virtual.scrollToIndex(index, { align: "auto" });
  }, [props.reveal]);

  function draw(row: Row) {
    if (row.kind === "folder") {
      const item = row.folder;
      return <details className="dv-dir" data-folder={item.path} open={row.open}>
        <summary className="dv-tr" title={item.path} onClick={event => {
          const group = event.currentTarget.parentElement;
          if (group instanceof HTMLDetailsElement) {
            event.preventDefault();
            props.onFolder(item.path, group.open);
          }
        }}>
          <span className="dv-dir-chevron" style={{ display: "grid" }}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round"><path d="M9 6l6 6-6 6"/></svg></span>
          <span className="nm"><b>{item.name}</b></span>
        </summary>
      </details>;
    }
    const f = row.item.file;
    return <button type="button" className={`dv-tr${props.current === f.path ? " on" : ""}`} data-path={f.path}
      title={f.old_path ? `${f.old_path} -> ${f.path}` : f.path}
      onClick={() => props.onFile(f.path)} onContextMenu={event => props.onMenu(event, f)}>
      <span className={`dv-st ${STATUS_LETTER[f.status]}`}>{STATUS_LETTER[f.status]}</span>
      <span className="nm"><b>{row.item.name}</b></span><span className="ct">
        {!!f.additions && <i className="dv-plus">+{grouped(f.additions)}</i>}
        {!!f.deletions && <i className="dv-minus">-{grouped(f.deletions)}</i>}
      </span>
    </button>;
  }

  return <div className="dv-tree-rows" style={{ height: virtual.getTotalSize() }}>
    {virtual.getVirtualItems().map(item => <div key={item.key} className="dv-tree-row" data-index={item.index} ref={virtual.measureElement}
      style={{ transform: `translateY(${item.start}px)`, paddingLeft: rows[item.index].depth * 16 }}>{draw(rows[item.index])}</div>)}
  </div>;
}
