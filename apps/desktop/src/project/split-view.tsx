import { Fragment, useState, type ReactElement, type ReactNode } from "react";
import { moveLabel } from "../agents/move-store.ts";
import { closingNote } from "../agents/closing.ts";
import { agentName, runningKey, runningWhen, type RunningAgent } from "../agents/running-state.ts";
import { formatAgo } from "../ui/format.ts";
import { terminalNote, terminalWhere, type TerminalRow } from "../terminals/terminals.tsx";
import type { Freshness } from "../ui/pagecache.ts";
import type { TileData } from "./board.ts";
import type { LabelFilter, SplitGroup, SplitItem } from "./split.tsx";
import { Button, EmptyPanel, LineRow, Section, SectionHead } from "../ui/kit.tsx";
import { HarnessIcon } from "../ui/icons.tsx";
import { SideToggle } from "../center/sidebar-shell.tsx";
import { sessionMenu } from "../agents/sessionmenu.ts";
import type { AgentRow } from "../agents/agents.ts";
import { go } from "../center/views.tsx";
import { FreshNote } from "./page-view.tsx";
import { ProjectSessions } from "./sessions-view.tsx";
import type { SessionRow } from "./projects-state.ts";

export function ProjectNavigation({ groups, picked, openTile }: { groups: readonly SplitGroup[]; picked: string; openTile: (key: string) => void }) {
  return <>{groups.map((group, index) => <Fragment key={`${index}:${group.title}`}>
    {!group.title && index > 0 && <hr/>}
    {group.title && <h4>{group.title}{group.items.length > 0 && <span className="n">{group.items.length}</span>}</h4>}
    {group.note && <div className="note">{group.note}</div>}
    {group.controls}
    {group.labels && group.labels.all.length > 0 && <LabelGroup filter={group.labels}/>}
    {group.items.map(item => <Row key={item.key} item={item} picked={picked} filter={group.labels} nav={!group.title} openTile={openTile}/>)}
  </Fragment>)}</>;
}

export function LabelGroup({ filter }: { filter: LabelFilter }) {
  return <div className="pj-labels">{filter.all.map(label => <Label key={label} label={label} filter={filter} button/>)}</div>;
}

export function Row({ item, picked, filter, nav, openTile }: { item: SplitItem; picked: string | null; filter?: LabelFilter; nav: boolean; openTile: (key: string) => void }) {
  return <button type="button" className={`pj-li${nav ? " nav" : ""}`} title={item.title} aria-current={item.key === picked ? "true" : undefined}
    onClick={() => item.open ? item.open() : openTile(item.key)} onContextMenu={item.menu ? e => item.menu?.(e.nativeEvent) : undefined}>
    {nav ? <><span className="t"><b>{item.title}</b></span><span className="c">{item.sub}</span></> : <>
      <span className="lead">{item.lead}</span><span className="t"><b>{item.title}</b><span>{item.sub}{item.tags?.map(label => filter ? <Label key={label} label={label} filter={filter}/> : <i className="pj-tag" key={label}>{label}</i>)}</span></span>
      <span className="r">{item.right}</span>
    </>}
  </button>;
}

function Label({ label, filter, button = false }: { label: string; filter: LabelFilter; button?: boolean }) {
  const cls = `pj-tag lab${label === filter.on ? " on" : ""}`;
  const pick = (event: React.MouseEvent) => { event.stopPropagation(); filter.pick(label); };
  return button ? <button type="button" className={cls} onClick={pick}>{label}</button> : <i className={cls} onClick={pick}>{label}</i>;
}

/// `doing` names what a working agent is doing, when it says, in place of the time.
export function RunningStatus({ row, doing = false }: { row: RunningAgent; doing?: boolean }) {
  const moving = moveLabel(row), closing = closingNote(row);
  if (moving) return <span className={`chip ${moving.chip === "Moving" ? "run" : "bad"}`}>{moving.chip}</span>;
  if (closing) return closing;
  if (row.status === "asking") return <span className="chip wait">needs you</span>;
  if (["working", "running", "starting"].includes(row.status)) return <span className="cc-running"><span className="drun" role="img" aria-label={row.status === "starting" ? "Starting" : "Working"}/>{doing && row.doing ? <span className="doing">{row.doing}</span> : runningWhen(row)}</span>;
  return runningWhen(row);
}

/// A project's sessions in place of the sidebar while one of them shows. Its head mirrors the
/// sidebar's: the project's name, which goes back to its page, and the toggle back to the
/// sidebar. Then a filter, the running sessions, roomier, with what each is doing, and the
/// latest past ones, one line
/// each. `current` is the running key of the session on screen.
export function ProjectRail({ project, label, running, past, current, back }: {
  project: string; label: string; running: RunningAgent[]; past: SessionRow[]; current: string | null; back(): void;
}) {
  const [filter, setFilter] = useState("");
  const needle = filter.trim().toLowerCase();
  const matches = (row: AgentRow) => !needle || (row.title || agentName(row.agent)).toLowerCase().includes(needle);
  const live = running.filter(matches), ended = past.filter(matches);
  const row = (session: AgentRow, right: ReactNode, sub: ReactNode = "") => <Row key={runningKey(session)} picked={current} nav={false} openTile={() => {}} item={{
    key: runningKey(session), lead: <HarnessIcon agent={session.agent} />, title: session.title || agentName(session.agent), sub, right,
    open: () => go("pane", { agent: session }),
    menu: event => void sessionMenu(event, session, picked => go("pane", { agent: picked })),
  }} />;

  return <>
    <div className="cc-head" data-tauri-drag-region="" />
    <div className="cc-brand"><Button look="quiet" className="rail-back" title={`Back to ${label}`} onClick={back}>{label}</Button><SideToggle /></div>
    <input className="ip-field rail-filter" type="search" placeholder="Filter sessions" aria-label="Filter sessions" spellCheck={false} value={filter} onChange={e => setFilter(e.currentTarget.value)} />
    <div className="rail-scroll">
    {!!live.length && <h4>Running<span className="n">{live.length}</span></h4>}
    {!!live.length && <div className="rail-live">{live.map(session => row(session, <RunningStatus row={session} />, runningLine(session)))}</div>}
    {!!ended.length && <h4>Past<span className="n">{ended.length}</span></h4>}
    {ended.slice(0, RAIL_PAST).map(session => row(session, formatAgo(session.last_activity, "short")))}
    {ended.length > RAIL_PAST && <Button look="quiet" className="rail-all" onClick={() => go("projects", { project, tile: "agents" })}>All {ended.length} sessions</Button>}
    </div>
  </>;
}

/// A running row's second line: what it does, else the first line of its last reply.
function runningLine(session: RunningAgent) {
  if (session.doing) return session.doing;
  const reply = session.reply?.split("\n").find(line => line.trim())?.trim();
  return reply ? <i className="rail-reply">{reply}</i> : "";
}

/// The past sessions the rail lists; the project's page has them all.
const RAIL_PAST = 40;

export interface RecentSessions { rows: SessionRow[] | null; empty: string | null; count: number; freshness: Freshness | null }
function RecentSessionsSection({ session }: { session: RecentSessions }) {
  return <div id="sec-sessions"><div className="cc-sec"><h4>Past sessions{!!session.count && <span className="n">{session.count}</span>}{session.freshness && <FreshNote freshness={session.freshness} className=" ss-updating"/>}</h4></div>{session.rows ? <div style={{ display: "contents" }}><div className="cc-panel"><ProjectSessions rows={session.rows} all={false}/></div></div> : <EmptyPanel text={session.empty} />}</div>;
}

export function ProjectFallback({ session, machines, running, agents }: { session: RecentSessions; machines: ReactElement; running: ReactElement; agents: ReactElement }) {
  return <><RecentSessionsSection session={session}/><div className="ca-col">{machines}{running}{agents}</div></>;
}

function OverviewRow({ item, openTile }: { item: SplitItem; openTile: (key: string) => void }) {
  return <LineRow lead={item.lead} title={item.title} sub={item.sub} right={<>{item.right}{item.tags?.slice(0, 3).map(label => <i className="pj-tag" key={label}>{label}</i>)}</>} onOpen={() => item.open ? item.open() : openTile(item.key)} onContext={item.menu} />;
}

function OverviewBox({ title, count, open, children }: { title: string; count: number | string; open: () => void; children: ReactNode }) {
  return <Section id="" title={<Button className="lk" onClick={open}>{title}</Button>} count={count}>{children}</Section>;
}

export function ProjectOverview({ run, prs, issues, prNote, issueNote, machines, terminals, session, openTile, openTerminal }: {
  run: SplitItem[]; prs: SplitItem[]; issues: SplitItem[]; prNote: string | undefined; issueNote: string | undefined;
  machines: TileData; terminals: TerminalRow[]; session: RecentSessions;
  openTile: (key: string) => void; openTerminal: (terminal: TerminalRow) => void;
}) {
  const rows = (items: SplitItem[]) => items.map(item => <OverviewRow key={item.key} item={item} openTile={openTile}/>);
  const empty = (title: string, note: string | undefined) => <div id=""><SectionHead title={title} /><EmptyPanel text={note ?? ""} /></div>;
  return <><div>
    {!!run.length && <OverviewBox title="Running" count={run.length} open={() => openTile("running")}>{rows(run.slice(0, 5))}</OverviewBox>}
    <RecentSessionsSection session={session}/>
    {!!machines.rows.length && <OverviewBox title="Machines" count={machines.count} open={() => openTile("machines")}>{machines.rows.slice(0, 5).map((row, index) => <LineRow key={row.id ?? index} lead={<span className={`ddot${row.dot ? " off" : ""}`}/>} title={row.text ?? ""} sub={row.note} rowTitle={row.title || row.text || ""} textTitle={row.text || ""} onOpen={() => openTile("machines")} />)}</OverviewBox>}
    {!!terminals.length && <OverviewBox title="Terminals" count={terminals.length} open={() => openTile("machines")}>{terminals.slice(0, 5).map(terminal => <LineRow key={`${terminal.machine_id}:${terminal.id}`} lead={<span className={`ddot${terminal.state === "running" ? "" : " off"}`}/>} title={terminal.name} sub={<>{terminal.machine_name} · {terminalWhere(terminal)}</>} right={terminalNote(terminal, ms => formatAgo(ms, "short"))} rowTitle={`${terminal.machine_name}: ${terminal.command} in ${terminal.cwd}`} onOpen={() => openTerminal(terminal)} />)}</OverviewBox>}
  </div><div>
    {prs.length ? <OverviewBox title="Pull requests" count={prs.length} open={() => openTile(prs[0].key)}>{rows(prs.slice(0, 5))}</OverviewBox> : empty("Pull requests", prNote)}
    {issues.length ? <div id=""><SectionHead title={<Button className="lk" onClick={() => openTile("issues")}>Issues</Button>} count={issues.length} /><div className="cc-panel pj-scroll pj-ov-issues">{rows(issues)}</div></div> : empty("Issues", issueNote)}
  </div></>;
}

export function ProjectStatus({ cells, openTile }: { cells: readonly { label: string; text: string; key: string; bad?: boolean }[]; openTile: (key: string) => void }) {
  return <>{cells.filter(cell => cell.label && cell.text).map(cell => <button type="button" className="m" key={cell.key} onClick={() => openTile(cell.key)}><b>{cell.label}</b><span className={cell.bad ? "bad" : ""}>{cell.text}</span></button>)}</>;
}

export function SessionTabs({ selected, sessions, agents, select }: { selected: "sessions" | "agents"; sessions: number; agents: number; select: (tab: "sessions" | "agents") => void }) {
  return <>{([["sessions", "Sessions", sessions], ["agents", "Agents", agents]] as const).map(([key, label, count]) =>
    <button key={key} type="button" role="tab" aria-selected={key === selected} onClick={() => select(key)}>{label}{count ? <span className="n">{count}</span> : null}</button>)}</>;
}
