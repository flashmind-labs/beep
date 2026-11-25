import { Fragment, useLayoutEffect, useRef, useState, type ReactElement, type ReactNode } from "react";
import { moveLabel } from "../agents/move-store.ts";
import { closingNote } from "../agents/closing.ts";
import { agentName, runningKey, runningWhen, type RunningAgent } from "../agents/running-state.ts";
import { formatAgo } from "../ui/format.ts";
import type { TerminalRow } from "../terminals/terminals.tsx";
import { terminalNote, terminalWhere } from "../terminals/terminal-list.ts";
import type { Freshness } from "../ui/pagecache.ts";
import type { TileData } from "./board.ts";
import type { LabelFilter, SplitGroup, SplitItem } from "./split.tsx";
import { Button, EmptyPanel, LineRow, NO_AUTO, Section, SectionHead, Spinner } from "../ui/kit.tsx";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { ProjectArt } from "../ui/painted-art.tsx";
import { SideToggle } from "../center/sidebar-shell.tsx";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { sessionOffers } from "../agents/sessionmenu-state.ts";
import { closeAgentSession } from "../agents/session.tsx";
import type { AgentRow, AgentSession } from "../agents/agents-state.ts";
import { go } from "../center/nav.ts";
import { FreshNote } from "./page-view.tsx";
import { ProjectSessions } from "./sessions-view.tsx";
import { PersonaTag } from "./customagents-view.tsx";
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

export function Row({ item, picked, filter, nav, openTile, onMiddleClick }: { item: SplitItem; picked: string | null; filter?: LabelFilter; nav: boolean; openTile: (key: string) => void; onMiddleClick?: () => void }) {
  return <button type="button" className={`pj-li${nav ? " nav" : ""}`} title={item.title} aria-current={item.key === picked ? "true" : undefined}
    onClick={() => item.open ? item.open() : openTile(item.key)} onContextMenu={item.menu ? e => item.menu?.(e.nativeEvent) : undefined}
    onMouseDown={onMiddleClick && (event => { if (event.button === 1) event.preventDefault(); })}
    onAuxClick={onMiddleClick && (event => {
      if (event.button !== 1) return;
      event.preventDefault();
      onMiddleClick();
    })}>
    {nav ? <><span className="t"><b>{item.title}</b></span><span className="c">{item.sub}</span></> : <>
      {item.lead && <span className="lead">{item.lead}</span>}<span className="t"><b>{item.title}</b><span>{item.sub}{item.tags?.map(label => filter ? <Label key={label} label={label} filter={filter}/> : <i className="pj-tag" key={label}>{label}</i>)}</span></span>
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
  if (["working", "running", "starting"].includes(row.status)) return <span className="cc-running"><Spinner dot label={row.status === "starting" ? "Starting" : "Working"} />{doing && row.doing ? <span className="doing">{row.doing}</span> : runningWhen(row)}</span>;
  return runningWhen(row);
}

/// A project's sessions in place of the sidebar while one of them shows. Its head mirrors the
/// sidebar's: the project's name, which goes back to its page, and the toggle back to the
/// sidebar. Then its `RailList`. `current` is the running key of the session on screen. At the foot, over the project's
/// painting, a card with its branch, pull requests, issues and machines.
export function ProjectRail({ project, label, running, past, current, card, back }: {
  project: string; label: string; running: RunningAgent[]; past: SessionRow[]; current: string | null; card: RailCard | null; back(): void;
}) {
  return <>
    <ProjectArt project={project} />
    <div className="cc-head" data-tauri-drag-region="" />
    <div className="cc-brand"><Button look="quiet" className="rail-back" title={`Back to ${label}`} onClick={back}>{label}</Button><SideToggle /></div>
    <RailList running={running} past={past} current={current} more={() => go("projects", { project, tile: "agents" })} />
    {card && <ProjectCard project={project} card={card} />}
  </>;
}

/// A rail's filter and its sessions: the running ones, roomier, with what each is doing, then
/// the latest past ones, one line each, and `more` to see them all. A `head`, such as a chat,
/// leads the running ones, which are its subitems.
export function RailList({ head = null, running, past, current, more, noun = "sessions" }: {
  head?: RunningAgent | null; running: readonly RunningAgent[]; past: readonly (AgentRow & Pick<AgentSession, "last_activity">)[]; current: string | null; more: () => void; noun?: string;
}) {
  const [filter, setFilter] = useState("");
  const needle = filter.trim().toLowerCase();
  // The painting shows clearer while no row reaches into it.
  const list = useRef<HTMLDivElement>(null), [clear, setClear] = useState(false);
  useLayoutEffect(() => {
    const box = list.current, art = box?.parentElement?.querySelector<HTMLElement>(":scope > .painted-art");
    if (!box || !art) return;
    const check = () => {
      const end = box.lastElementChild?.getBoundingClientRect().bottom ?? 0, top = art.getBoundingClientRect();
      setClear(box.scrollHeight <= box.clientHeight && end < top.top + top.height * 0.3);
    };
    check();
    const sized = new ResizeObserver(check);
    sized.observe(box);
    return () => sized.disconnect();
  });
  const matches = (row: AgentRow) => !needle || (row.title || agentName(row.agent)).toLowerCase().includes(needle);
  const live = running.filter(matches), ended = past.filter(matches);
  const row = (session: AgentRow, right: ReactNode, sub: ReactNode, close?: () => void) => <Row key={runningKey(session)} picked={current} nav={false} openTile={() => {}} onMiddleClick={close} item={{
    key: runningKey(session), lead: <HarnessIcon agent={session.agent} />, title: session.title || agentName(session.agent), sub, right,
    open: () => go("pane", { agent: session }),
    menu: event => void sessionMenu(event, session, picked => go("pane", { agent: picked })),
  }} />;

  const liveRow = (session: RunningAgent) => row(session, <><RunningStatus row={session} /><LineCounts changes={session.changes} /></>, <><PersonaTag persona={session.persona} />{workLine(session)}</>,
    session.status === "idle" && sessionOffers(session).close ? () => void closeAgentSession(session) : undefined);

  return <>
    <input className="ip-field rail-filter" type="search" placeholder={`Filter ${noun}`} aria-label={`Filter ${noun}`} {...NO_AUTO} value={filter} onChange={e => setFilter(e.currentTarget.value)} />
    <div className="rail-scroll" ref={list} data-clear={clear || undefined}>
    {(!!head || !!live.length) && <h4>Running<span className="n">{live.length + (head ? 1 : 0)}</span></h4>}
    {(!!head || !!live.length) && <div className="rail-live">{head && liveRow(head)}{head ? !!live.length && <div className="rail-live rail-threads">{live.map(liveRow)}</div> : live.map(liveRow)}</div>}
    {!!ended.length && <h4>Past<span className="n">{ended.length}</span></h4>}
    {ended.slice(0, RAIL_PAST).map(session => row(session, formatAgo(session.last_activity, "short"), <PersonaTag persona={session.persona} />))}
    {ended.length > RAIL_PAST && <Button look="quiet" className="rail-all" onClick={more}>All {ended.length} {noun}</Button>}
    </div>
  </>;
}

/// What the rail's card shows of its project, as its page's rows count it; `null` counts are
/// still being read.
export interface RailCard { prs: number | null; issues: number | null; actions: string; failed: boolean; machines: string }

/// The project's rows that are not sessions, each opening that tab of its page.
function ProjectCard({ project, card }: { project: string; card: RailCard }) {
  const open = (tile: string) => () => go("projects", { project, tile });
  const count = (n: number | null) => n === null ? "" : `${n} open`;
  return <div className="rail-card">
    <Button look="quiet" className="rail-card-li" onClick={open("prs")}><Icon name="pr" /><span>Pull requests</span><small>{count(card.prs)}</small></Button>
    <Button look="quiet" className="rail-card-li" onClick={open("issues")}><Icon name="issue" /><span>Issues</span><small>{count(card.issues)}</small></Button>
    <Button look="quiet" className="rail-card-li" onClick={open("actions")}><Icon name="run" /><span>Actions</span><small className={card.failed ? "bad" : undefined}>{card.actions}</small></Button>
    <Button look="quiet" className="rail-card-li" onClick={open("machines")}><Icon name="sync" /><span>Machines</span><small>{card.machines}</small></Button>
  </div>;
}

/// A running row's second line: what it does, else the first line of its last reply, with
/// its emphasis and code as notifications show them.
export function workLine(session: RunningAgent) {
  if (session.doing) return session.doing;
  return session.reply_html ? <i className="rail-reply nt-md" dangerouslySetInnerHTML={{ __html: session.reply_html }} /> : "";
}

/// The lines its checkout changed, green and red, as its last turn left them.
export function LineCounts({ changes }: { changes: RunningAgent["changes"] }) {
  if (!changes?.additions && !changes?.deletions) return null;
  return <span className="rail-diff" title="Lines added and removed in its checkout">
    {!!changes.additions && <i className="dv-plus">+{changes.additions}</i>}
    {!!changes.deletions && <i className="dv-minus">-{changes.deletions}</i>}
  </span>;
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

export function ProjectStatus({ cells, openTile }: { cells: readonly { label: string; text: string | null; key: string; bad?: boolean }[]; openTile: (key: string) => void }) {
  return <>{cells.filter(cell => cell.label && cell.text !== null).map(cell => <button type="button" className="m" key={cell.key} onClick={() => openTile(cell.key)}><b>{cell.label}</b>{cell.text && <span className={cell.bad ? "bad" : ""}>{cell.text}</span>}</button>)}</>;
}

export function SessionTabs({ selected, sessions, agents, select }: { selected: "sessions" | "agents"; sessions: number; agents: number; select: (tab: "sessions" | "agents") => void }) {
  return <>{([["sessions", "Sessions", sessions], ["agents", "Agents", agents]] as const).map(([key, label, count]) =>
    <button key={key} type="button" role="tab" aria-selected={key === selected} onClick={() => select(key)}>{label}{count ? <span className="n">{count}</span> : null}</button>)}</>;
}
