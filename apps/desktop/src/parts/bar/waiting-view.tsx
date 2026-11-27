import { useLayoutEffect, useRef } from "react";
import { HintView, type HintViewModel } from "./hints-view.tsx";
import type { WaitingKind } from "../../stores/bar/waiting-state.ts";
import { HarnessIcon } from "../../ui/icons.tsx";
import { Button, Spinner } from "../../ui/kit.tsx";

export interface WaitingRow { key: string; agent: string; title: string; sub: string; kind: WaitingKind; doing: string }

export function WaitingList(p: { rows: WaitingRow[]; selected: number; summary: string; query: string; hints: HintViewModel; open(index: number): void; newAgent(): void }) {
  const active = p.rows[p.selected] ? `wt-${p.selected}` : undefined;
  const selected = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { selected.current?.scrollIntoView({ block: "nearest" }); }, [p.selected, p.rows.length]);
  return <><div className="dhead"><span className="tt"><b>Sessions</b><small>{p.query ? `Filter: ${p.query}` : p.summary}</small></span><Button id="wtNew" onClick={p.newAgent}>New<kbd>⌘N</kbd></Button></div>
    <div className="dlist top" id="wtlist" role="listbox" aria-label="Sessions" tabIndex={0} aria-activedescendant={active}>
      {p.rows.map((row, index) => <button key={row.key} id={`wt-${index}`} className="dchat" role="option" tabIndex={-1} aria-selected={index === p.selected} data-wti={index} ref={index === p.selected ? selected : undefined} onClick={() => p.open(index)}>
        <span style={{ display: "contents" }}><HarnessIcon agent={row.agent} className="dapp dcoderav" /></span>
        <span className="t"><span>{row.title}</span><small>{row.sub}</small></span>
        {row.kind === "asking" ? <span className="dstate ask">Needs you</span>
          : row.kind === "done" ? <span className="dstate run">Done</span>
          : row.kind === "running" ? <span className="run" title={row.doing}><Spinner dot /><span className="tx">{row.doing}</span></span>
          : <span className="w">Idle</span>}
      </button>)}
      {!p.rows.length && <div className="dmuted dpadx">Nothing matches.</div>}
    </div>
    <HintView value={p.hints} />
  </>;
}
