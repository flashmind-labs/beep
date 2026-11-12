import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { useLayoutEffect, useRef } from "react";
import { chipClass, researchState, researchSummary, STATE_LABEL, type ResearchRow } from "./research-list.ts";
import { researchLine } from "./researches.ts";
import { ICON } from "../ui/icons.tsx";

export function ResearchesView(p: { list: ResearchRow[]; loaded: boolean; selected: number; mode: "list" | "actions"; error: string; initial: string; typed: boolean; hints: HintViewModel; edit(): void; choose(index: number, action: "open" | "pdf"): void }) {
  const input = useRef<HTMLInputElement>(null), list = useRef<HTMLDivElement>(null), last = useRef("");
  useLayoutEffect(() => { input.current?.focus(); }, []);
  const selected = !p.typed ? p.list[p.selected]?.session ?? "" : "";
  useLayoutEffect(() => {
    if (selected && selected !== last.current) list.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
    last.current = selected;
  }, [selected]);
  return <><div className="dinput"><input ref={input} id="rtopic" placeholder="What should the team research?" autoComplete="off" spellCheck={false} defaultValue={p.initial} onChange={p.edit} /><kbd>⌘R</kbd></div>
    <div className="dhead"><span className="tt"><b>Research</b><small>{p.loaded ? researchSummary(p.list) : ""}</small></span></div>
    {p.error && <div className="derr dpadx">{p.error}</div>}
    {p.loaded && !p.list.length ? <div className="dempty"><span className="dicon" dangerouslySetInnerHTML={{ __html: ICON.doc }} /><b className="dbig">No research yet.</b><span className="dmuted dblock">A team of agents digs into a question and writes it up as a paper.</span></div> : <div ref={list} className="dlist top" role="listbox" aria-label="Research">{p.list.map((row, i) => {
      const current = i === p.selected && !p.typed, state = researchState(row.status);
      return <div key={row.session} className={`drt${state === "failed" || state === "stopped" ? " off" : ""}`} role="option" aria-selected={current} data-i={i} onClick={() => p.choose(i, "open")}><span className="ic" dangerouslySetInnerHTML={{ __html: ICON.doc }} /><span className="tt"><b>{row.topic}</b><small>{researchLine(row)}</small></span>{current && p.mode === "actions" && !!row.sources ? <span className="acts"><button className="dbtn" data-rl="pdf" onClick={event => { event.stopPropagation(); p.choose(i, "pdf"); }}>Export PDF</button></span> : <span className={`chip ${chipClass(row.status)}`}>{state === "running" && <span className="drun" />}{STATE_LABEL[state]}</span>}</div>;
    })}</div>}
    <HintView value={p.hints} />
  </>;
}
