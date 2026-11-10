import { HintView, type HintViewModel } from "./hints-view.tsx";
import type { WaitingKind } from "./waiting-state.ts";

export interface WaitingRow { key: string; avatar: string; title: string; sub: string; kind: WaitingKind; doing: string }

export function WaitingList(p: { rows: WaitingRow[]; selected: number; summary: string; query: string; hints: HintViewModel }) {
  const active = p.rows[p.selected] ? `wt-${p.selected}` : undefined;
  return <><div className="dhead"><span className="tt"><b>Sessions</b><small>{p.query ? `Filter: ${p.query}` : p.summary}</small></span><button className="dbtn" id="wtNew">New<kbd>⌘N</kbd></button></div>
    <div className="dlist top" id="wtlist" role="listbox" aria-label="Sessions" tabIndex={0} aria-activedescendant={active}>
      {p.rows.map((row, index) => <button key={row.key} id={`wt-${index}`} className="dchat" role="option" tabIndex={-1} aria-selected={index === p.selected} data-wti={index}>
        <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: row.avatar }} />
        <span className="t"><span>{row.title}</span><small>{row.sub}</small></span>
        {row.kind === "asking" ? <span className="dstate ask">Needs you</span>
          : row.kind === "done" ? <span className="dstate run">Done</span>
          : row.kind === "running" ? <span className="run" title={row.doing}><span className="dspin" /><span className="tx">{row.doing}</span></span>
          : <span className="w">Idle</span>}
      </button>)}
      {!p.rows.length && <div className="dmuted dpadx">Nothing matches.</div>}
    </div>
    <HintView value={p.hints} />
  </>;
}
