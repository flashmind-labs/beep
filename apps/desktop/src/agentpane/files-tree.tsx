import { Fragment, type CSSProperties } from "react";
import type { GitChange, TreeRow } from "./files-state.ts";

const marks: Record<GitChange, [string, string]> = { untracked: ["U", "Untracked"], added: ["A", "Added"], modified: ["M", "Changed"], conflicted: ["C", "In conflict"] };
const depth = (value: number): CSSProperties & { "--d": number } => ({ "--d": value });

export function FilesShell({ root, filter, ignored, label, busy, active, status }: { root: string; filter: string; ignored: boolean; label: string; busy: boolean; active: string | null; status: string }) {
  return <>
    <div className="agfbar"><input className="agffind" defaultValue={filter} placeholder="Filter files" aria-label="Filter files" spellCheck={false} /><label className="agfign"><input type="checkbox" defaultChecked={ignored} /> Show ignored</label><button className="agpbtn" data-frefresh="1" title="Read the folders again">↻</button></div>
    <div className="agtree" role="tree" aria-label="Working files" aria-multiselectable="true" aria-busy={busy} aria-activedescendant={active ?? undefined} tabIndex={0} />
    <div className="agpacts"><span className="meta mono" title={root}>{label}</span><span data-fstatus role="status">{status}</span><span className="sp" /><span className="hint">Cmd-click to select, Shift-click for a range. Cmd+Delete asks before deleting.</span></div>
  </>;
}

export function FilesTree({ rows, selected, message, error, more, walking, capped, limit }: { rows: TreeRow[]; selected: ReadonlySet<string>; message: string | null; error: boolean; more: number; walking: boolean; capped: boolean; limit: number }) {
  if (message !== null) return <div className={`agpmsg${error ? " derr" : ""}`}>{walking && <span className="dspin" />}{message}</div>;
  return <>{rows.map(row => <Fragment key={row.path}>
    <div className={`agtr${selected.has(row.path) ? " on" : ""}${row.ignored ? " ign" : ""}`} role="treeitem" aria-selected={selected.has(row.path)} id={`agfile-${encodeURIComponent(row.path)}`} style={depth(row.depth)} draggable data-fpath={row.path} data-ffolder={row.folder ? "1" : undefined} aria-expanded={row.folder ? row.open : undefined} title={row.path}>
      <span className={`tw${row.folder && row.open ? " open" : ""}`}>{row.folder && (row.loading ? <span className="dspin" /> : <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round"><path d="M9 6l6 6-6 6" /></svg>)}</span>
      {row.folder ? <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg> : <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M14 3H6v18h12V7z" /><path d="M14 3v4h4" /></svg>}
      <span className="nm">{row.name}</span>{row.change && <span className={`gm ${row.change}`} title={marks[row.change][1]}>{marks[row.change][0]}</span>}
    </div>{row.error ? <div className="agtr note derr" style={depth(row.depth + 1)}>{row.error}</div> : !!row.more && <div className="agtr note" style={depth(row.depth + 1)}>{row.more} more not shown</div>}
  </Fragment>)}{!!more && <div className="agtr note" style={depth(0)}>{more} more not shown</div>}{walking && <div className="agtr note" style={depth(0)}><span className="dspin" />Reading folders…</div>}{capped && <div className="agtr note" style={depth(0)}>Stopped reading at {limit.toLocaleString()} files</div>}</>;
}
