import { Icon } from "../../ui/icons.tsx";
import { grouped, type DiffFile } from "../../stores/review/diff-state.ts";

interface DiffFileHeaderProps {
  file: DiffFile;
  count: string;
  readOnly: boolean;
  openLabel: string | null;
  onComment: () => void;
  onCopy: () => void;
  onOpen: () => void;
  folded: boolean;
  onToggle: () => void;
  onMenu: (event: React.MouseEvent<HTMLDivElement>) => void;
}

export function DiffFileHeader({ file, count, readOnly, openLabel, onComment, onCopy, onOpen, folded, onToggle, onMenu }: DiffFileHeaderProps) {
  return <div className="dv-fh" tabIndex={0} role="button" aria-expanded={!folded} title="Click or press x to fold" onContextMenu={onMenu}
    onClick={event => { if (!(event.target instanceof Element) || !event.target.closest(".dv-facts")) onToggle(); }}
    onKeyDown={event => { if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) { event.preventDefault(); onToggle(); } }}>
    <span className="dv-chev"><span style={{ display: "grid" }}>
      <Icon name="twist" size={12} />
    </span></span>
    <span className="dv-path">
      {file.old_path && <>
        <span className="old" title={file.old_path}>{file.old_path}</span>
        <span className="to" title="renamed to" style={{ display: "grid" }}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>
        </span>
      </>}
      <span title={file.path}>{file.path}</span>
    </span>
    <span className={`dv-chip ${file.status}`}>{file.status}</span>
    <span className="dv-fct">
      {!!file.additions && <i className="dv-plus">+{grouped(file.additions)}</i>}
      {!!file.deletions && <i className="dv-minus">-{grouped(file.deletions)}</i>}
    </span>
    <span className="dv-fcm">{count}</span>
    <span className="dv-facts">
      {!readOnly && <button type="button" className="dv-btn" title="Comment on this file"
        onClick={event => { event.stopPropagation(); onComment(); }}>Comment</button>}
      <button type="button" className="dv-btn icon" title="Copy path" onClick={event => { event.stopPropagation(); onCopy(); }}>
        <span style={{ display: "grid" }}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round">
            <rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/>
          </svg>
        </span>
      </button>
      {openLabel !== null && file.status !== "deleted" && <button type="button" className="dv-btn icon" title={openLabel}
        onClick={event => { event.stopPropagation(); onOpen(); }}>
        <span style={{ display: "grid" }}>
          <Icon name="panePdf" />
        </span>
      </button>}
    </span>
  </div>;
}
