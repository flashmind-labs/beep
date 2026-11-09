import { useLayoutEffect, useMemo, type MouseEvent } from "react";
import { compactFolders, matchesFilter, splitPath, STATUS_LETTER, type DiffFile } from "./diff-state.ts";

interface Folder {
  kind: "folder";
  path: string;
  name: string;
  children: TreeItem[];
}
interface FileItem { kind: "file"; file: DiffFile; name: string }
type TreeItem = Folder | FileItem;

interface DiffTreeProps {
  files: readonly DiffFile[];
  filter: string;
  current: string | null;
  closedFolders: ReadonlySet<string>;
  filteredClosedFolders: ReadonlySet<string>;
  onFolder: (path: string, open: boolean) => void;
  onFile: (path: string) => void;
  onMenu: (event: MouseEvent<HTMLButtonElement>, file: DiffFile) => void;
  onCommit: () => void;
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

function matches(item: TreeItem, filter: string): boolean {
  return item.kind === "file" ? matchesFilter(item.file, filter) : item.children.some(child => matches(child, filter));
}

export function DiffTree(props: DiffTreeProps) {
  const items = useMemo(() => groupFiles(props.files), [props.files]);
  useLayoutEffect(() => { props.onCommit(); });

  function draw(item: TreeItem) {
    if (item.kind === "folder") {
      const open = props.filter.trim() ? !props.filteredClosedFolders.has(item.path) : !props.closedFolders.has(item.path);
      return <details key={item.path} className="dv-dir" data-folder={item.path} open={open} hidden={!matches(item, props.filter)}>
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
        <div className="dv-dir-children">{item.children.map(draw)}</div>
      </details>;
    }
    const f = item.file;
    return <button key={f.path} type="button" className={`dv-tr${props.current === f.path ? " on" : ""}`} data-path={f.path}
      title={f.old_path ? `${f.old_path} -> ${f.path}` : f.path} hidden={!matchesFilter(f, props.filter)}
      onClick={() => props.onFile(f.path)} onContextMenu={event => props.onMenu(event, f)}>
      <span className={`dv-st ${STATUS_LETTER[f.status]}`}>{STATUS_LETTER[f.status]}</span>
      <span className="nm"><b>{item.name}</b></span><span className="ct">
        {!!f.additions && <i className="dv-plus">+{f.additions.toLocaleString("en-US")}</i>}
        {!!f.deletions && <i className="dv-minus">-{f.deletions.toLocaleString("en-US")}</i>}
      </span>
    </button>;
  }
  return <>{items.map(draw)}</>;
}
