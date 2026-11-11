import { Spinner } from "../ui/kit.tsx";
import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { AGENT_ICON } from "./agents.ts";
import { actOnCopy, type CopiesModel } from "./copies.ts";

export interface AgentListRow { key: string; depth: number; title: string; sub: string; tail: { kind: "review" } | { kind: "move"; label: string; why: string | null } | { kind: "asking" } | { kind: "running"; doing: string } | { kind: "plain"; label: string } | { kind: "connecting" } }
export function AgentsList(p: { rows: AgentListRow[]; showSessions: boolean; selected: number; summary: string; error: string; copies: CopiesModel | null; loading: boolean; hints: HintViewModel }) {
  return <><div className="dhead"><span className="tt"><b>Coding agents</b><small>{p.summary}</small></span><button className="dbtn" id="agNew"><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: AGENT_ICON }} />New<kbd>⌘⇧A</kbd></button></div>
    {p.error && <div className="derr dpadx">{p.error}</div>}<div className="dagscroll">{p.showSessions && <div className="dlist top" role="listbox">{p.rows.map((row, index) => <button key={row.key} className={`dchat${row.depth ? " kid" : ""}${row.tail.kind === "connecting" ? " wait" : ""}`} style={row.depth ? { "--depth": row.depth } as React.CSSProperties : undefined} role="option" aria-selected={index === p.selected} data-agi={index}><span className="t"><span>{row.title}</span><small>{row.sub}</small></span>{row.tail.kind === "review" ? <span className="w">Reviewing</span> : row.tail.kind === "move" ? <span className={`dstate ${row.tail.label === "Moving" ? "run" : "ask"}`} title={row.tail.why ?? undefined}>{row.tail.label}</span> : row.tail.kind === "asking" ? <span className="dstate ask">Needs you</span> : row.tail.kind === "connecting" ? <span className="w" title="Shown as last seen until its machine answers">Connecting</span> : row.tail.kind === "running" ? <span className="run" title={row.tail.doing}><Spinner /><span className="tx">{row.tail.doing}</span></span> : <span className="w">{row.tail.label}</span>}</button>)}</div>}{p.copies && <Copies value={p.copies} />}</div>
    {!p.showSessions && !p.copies && !p.loading && <div className="dempty"><span className="dicon"><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: AGENT_ICON }} /></span><b className="dbig">No sessions running.</b><span className="dmuted dblock">Start one here, ask Beep in a chat, or run Claude Code in a terminal on a linked machine.</span></div>}
    <HintView value={p.hints} />
  </>;
}

/// The copies this computer keeps of other machines' sessions.
function Copies({ value }: { value: CopiesModel }) {
  return <div className="dlist dcopies">{value.error && <div className="derr dpadx">{value.error}</div>}{value.rows.map(row => <div className="dchat copy" key={row.key} data-cpk={row.key}><span className="t"><span>{row.title}</span><small>{row.sub}</small></span>{row.buttons.map(b => <button key={b.act} className={`dbtn ${b.pri ? "pri" : "quiet"} small`} data-cpact={b.act} disabled={b.busy} onClick={() => actOnCopy(row.key, b.act)}>{b.label}</button>)}</div>)}</div>;
}
