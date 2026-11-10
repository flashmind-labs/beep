import { Fragment, useLayoutEffect, useRef } from "react";
import { formatAgo } from "../ui/format.ts";
import { chipClass, phaseStep, PHASES, progressLine, researchState, STATE_LABEL, type ResearchRow } from "./research-list.ts";

interface Props {
  list: readonly ResearchRow[] | null;
  error: string;
  starting: boolean;
  draft: string;
  onDraft: (value: string) => void;
  onStart: () => void;
  onOpen: (session: string) => void;
}

export function ResearchPage({ list, error, starting, draft, onDraft, onStart, onOpen }: Props) {
  const field = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    if (field.current && field.current.value !== draft) field.current.value = draft;
  }, [draft]);
  return <div className="cc-view">
    <div className="cc-rnew"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>
      <input type="text" placeholder="What should the team research?" autoComplete="off" spellCheck={false} aria-label="What should the team research?" ref={field} defaultValue={draft} disabled={starting} onChange={e => onDraft(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); onStart(); } }}/>
      <button type="button" className="dbtn pri small" disabled={starting} onClick={onStart}>{starting ? "Starting…" : "Research"}</button>
    </div>
    {error && <div className="cc-note"><span className="ddot bad"/><span className="cc-err">{error}</span></div>}
    {list === null ? !error && <div className="cc-panel">{[70, 55, 62].map(width => <div className="cc-skel" key={width} style={{ width: `${width}%` }}/>)}</div> : list.length === 0 ?
      <div className="cc-panel cc-empty"><b>No research yet</b><span>Type a question above to start one.</span></div> :
      <div><div className="cc-sec"><h4>Research</h4></div><ResearchTable list={list} onOpen={onOpen}/></div>}
  </div>;
}

function ResearchTable({ list, onOpen }: { list: readonly ResearchRow[]; onOpen: (session: string) => void }) {
  return <div className="cc-mt cc-rt" role="table">
    {["Research", "Status", "Progress", "Sources", "Started"].map(title => <span className="th" key={title}>{title}</span>)}
    {list.map(row => <ResearchCells key={row.session} row={row} onOpen={onOpen}/>)}
  </div>;
}

function ResearchCells({ row, onOpen }: { row: ResearchRow; onOpen: (session: string) => void }) {
  const state = researchState(row.status);
  const off = state === "failed" || state === "stopped" ? " off" : "";
  const step = phaseStep(row.status);
  const open = () => onOpen(row.session);
  const chip = chipClass(row.status);
  return <Fragment>
    <span className={`nm${off} r`} onClick={open}><b><button type="button" title={row.topic}><span>{row.topic}</span></button></b></span>
    <span className={`${off.trim()} r`.trim()} onClick={open}><span className={`chip${chip ? ` ${chip}` : ""}`}>{state === "running" && <span className="dspin"/>}{STATE_LABEL[state]}</span></span>
    <span className={`${off.trim()} r`.trim()} onClick={open}>{step > 0 && <span className="cc-rsteps">{PHASES.map((phase, i) => <i key={phase} className={i + 1 < step ? "done" : i + 1 === step ? "now" : ""}/>)}</span>}<span className="num">{progressLine(row)}</span></span>
    <span className={`num${off} r`} onClick={open}>{row.sources ? String(row.sources) : "–"}</span>
    <span className={`num${off} r`} title={new Date(row.started_at).toLocaleString()} onClick={open}>{formatAgo(row.started_at, "since")}</span>
  </Fragment>;
}
