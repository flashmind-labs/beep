import type { ReactNode, Ref } from "react";
import { baseTitle, type DiffView } from "./diff-state.ts";
import { DiffSummary } from "./diff-summary.tsx";
import { Button } from "../ui/kit.tsx";

interface DiffToolbarProps {
  view: DiffView | null;
  loading: boolean;
  filter: string;
  split: boolean;
  uncommitted: boolean;
  showUncommitted: boolean;
  allFolded: boolean;
  flash: string;
  comments: number;
  sending: boolean;
  readOnly: boolean;
  /// Review with agent, or an empty span where a diff has none.
  feedback: ReactNode;
  findRef: Ref<HTMLInputElement>;
  onFilter: (value: string) => void;
  onSplit: () => void;
  onUncommitted: () => void;
  onFold: () => void;
  onRefresh: () => void;
  onSend: () => void;
}

export function DiffToolbar(props: DiffToolbarProps) {
  const none = !props.view?.files.length;
  const foldTitle = props.allFolded ? "Expand all" : "Collapse all";
  const sendTitle = `Send ${props.comments} ${props.comments === 1 ? "comment" : "comments"} to the agent as one message`;
  return <>
    <span className="dv-sum" title={props.view ? baseTitle(props.view, props.uncommitted) : ""}>
      <DiffSummary view={props.view} loading={props.loading} uncommitted={props.uncommitted} />
    </span>
    <input ref={props.findRef} className="dv-find" placeholder="Filter" aria-label="Filter files" spellCheck={false} disabled={none}
      value={props.filter} onChange={event => props.onFilter(event.currentTarget.value)} />
    <span className="dv-tools">
      {props.showUncommitted && <button type="button" className="dv-btn icon"
        title="Uncommitted only: hide the branch's commits" aria-label="Uncommitted only" aria-pressed={props.uncommitted}
        onClick={event => { event.stopPropagation(); props.onUncommitted(); }}>
        <span style={{ display: "grid" }}><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
          <circle cx="12" cy="12" r="3"/><path d="M12 3v6M12 15v6"/>
        </svg></span>
      </button>}
      <button type="button" className="dv-btn icon dv-splitbtn" title={props.split ? "Unified" : "Side by side"}
        aria-label="Side by side" aria-pressed={props.split} onClick={event => { event.stopPropagation(); props.onSplit(); }}>
        <span style={{ display: "grid" }}><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
          <rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>
        </svg></span>
      </button>
      <button type="button" className="dv-btn icon" title={foldTitle} aria-label={foldTitle} disabled={none}
        onClick={event => { event.stopPropagation(); props.onFold(); }}><span className="dv-pm">{props.allFolded ? "+" : "−"}</span></button>
      <button type="button" className="dv-btn icon" title={props.loading ? "Reading…" : "Read the changes again"}
        aria-label="Refresh" disabled={props.loading} onClick={event => { event.stopPropagation(); props.onRefresh(); }}>
        <span style={{ display: "grid" }}><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
          <path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>
        </svg></span>
      </button>
    </span>
    <span className="dv-sp"/>
    <span className="dv-flash" role="status">{props.flash}</span>
    <span style={{ display: "contents" }}>{props.feedback}</span>
    <Button look="primary" small title={sendTitle} hidden={props.readOnly || !props.comments}
      disabled={props.sending} onClick={props.onSend}>Send review<span className="dv-n">{props.comments}</span></Button>
  </>;
}
