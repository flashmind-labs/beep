import { Spinner } from "../ui/kit.tsx";
import { Fragment, useLayoutEffect, useRef, type CSSProperties, type RefObject } from "react";
import { useStore } from "zustand";
import type { GitChange, TreeRow } from "./files-state.ts";
import { FilesMenu } from "./files-menu.tsx";
import { type TreeView, filesMenuRef, filesModel, filesRef, keepScroll, refreshFiles, restoreScroll, revealed, setFilter, setIgnored, treeActions, treeRef, WALK_ENTRIES } from "./files.ts";

const marks: Record<GitChange, [string, string]> = { untracked: ["U", "Untracked"], added: ["A", "Added"], modified: ["M", "Changed"], conflicted: ["C", "In conflict"] };
const depth = (value: number): CSSProperties & { "--d": number } => ({ "--d": value });

/// The Files tab: the filter, the tree and its menu, and the footer.
export function FilesView({ hidden }: { hidden: boolean }) {
  const m = useStore(filesModel);
  const picked = useRef<HTMLDivElement>(null);

  // A tree that moved to the other dock, or came back with its session, scrolls to where it was.
  useLayoutEffect(() => {
    const tree = treeRef.current;
    if (tree) restoreScroll(tree);
    return keepScroll;
  }, []);

  return <div className="agpview files" hidden={hidden} ref={filesRef}>
    <div className="agfbar"><input className="agffind" value={m.filter} placeholder="Filter files" aria-label="Filter files" spellCheck={false}
      onChange={event => setFilter(event.currentTarget.value)}
      onKeyDown={event => { if (event.key === "Escape" && event.currentTarget.value) { event.stopPropagation(); event.preventDefault(); setFilter(""); } }} />
      <label className="agfign"><input type="checkbox" checked={m.ignored} onChange={event => setIgnored(event.currentTarget.checked)} /> Show ignored</label>
      <button className="agpbtn" data-frefresh="1" title="Read the folders again" onClick={refreshFiles}>↻</button></div>
    <div ref={treeRef} className={`agtree${m.drop === "" ? " dropping" : ""}`} role="tree" aria-label="Working files" aria-multiselectable="true" aria-busy={m.busy} aria-activedescendant={m.active ?? undefined} tabIndex={0}
      onClick={treeActions.click} onContextMenu={treeActions.contextMenu} onKeyDown={event => treeActions.keyDown(event, picked.current)}
      onDragStart={treeActions.dragStart} onDragOver={treeActions.dragOver} onDragLeave={treeActions.dragLeave} onDrop={treeActions.drop}>
      {m.tree && <FilesTree tree={m.tree} selected={m.selected} drop={m.drop} reveal={m.reveal} picked={picked} />}
    </div>
    <div className="agpacts"><span className="meta mono" title={m.root}>{m.label || m.root}</span><span data-fstatus role="status">{m.status}</span><span className="sp" /><span className="hint">Cmd-click to select, Shift-click for a range. Cmd+Delete asks before deleting.</span></div>
    {m.menu && <FilesMenu {...m.menu} menuRef={filesMenuRef} />}
  </div>;
}

function FilesTree({ tree, selected, drop, reveal, picked }: { tree: TreeView; selected: ReadonlySet<string>; drop: string | null; reveal: string | null; picked: RefObject<HTMLDivElement | null> }) {
  const shown = useRef<HTMLDivElement>(null);

  // A row asked for comes into view once it is drawn.
  useLayoutEffect(() => {
    if (!reveal || !shown.current) return;
    shown.current.scrollIntoView({ block: "center" });
    revealed();
  }, [reveal, tree]);

  if (tree.kind === "message") return <div className={`agpmsg${tree.error ? " derr" : ""}`}>{tree.loading && <Spinner />}{tree.text}</div>;
  // The first selected row is where the keyboard opens the menu.
  const first = tree.rows.find(row => selected.has(row.path))?.path ?? null;
  const rowRef = (row: TreeRow) => (node: HTMLDivElement | null) => {
    if (row.path === reveal) shown.current = node;
    if (row.path === first) picked.current = node;
  };
  return <>{tree.rows.map(row => <Fragment key={row.path}>
    <div ref={rowRef(row)} className={`agtr${selected.has(row.path) ? " on" : ""}${row.ignored ? " ign" : ""}${drop === row.path ? " drop" : ""}`} role="treeitem" aria-selected={selected.has(row.path)} id={`agfile-${encodeURIComponent(row.path)}`} style={depth(row.depth)} draggable data-fpath={row.path} data-ffolder={row.folder ? "1" : undefined} aria-expanded={row.folder ? row.open : undefined} title={row.path}>
      <span className={`tw${row.folder && row.open ? " open" : ""}`}>{row.folder && (row.loading ? <Spinner /> : <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round"><path d="M9 6l6 6-6 6" /></svg>)}</span>
      {row.folder ? <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg> : <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M14 3H6v18h12V7z" /><path d="M14 3v4h4" /></svg>}
      <span className="nm">{row.name}</span>{row.change && <span className={`gm ${row.change}`} title={marks[row.change][1]}>{marks[row.change][0]}</span>}
    </div>{row.error ? <div className="agtr note derr" style={depth(row.depth + 1)}>{row.error}</div> : !!row.more && <div className="agtr note" style={depth(row.depth + 1)}>{row.more} more not shown</div>}
  </Fragment>)}{!!tree.more && <div className="agtr note" style={depth(0)}>{tree.more} more not shown</div>}{tree.walking && <div className="agtr note" style={depth(0)}><Spinner />Reading folders…</div>}{tree.capped && <div className="agtr note" style={depth(0)}>Stopped reading at {WALK_ENTRIES.toLocaleString()} files</div>}</>;
}
