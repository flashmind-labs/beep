import { Fragment, type ReactElement } from "react";
import { moveLabel } from "../agents/move-store.ts";
import { closingNote } from "../agents/closing.ts";
import { runningWhen, type RunningAgent } from "../agents/running-state.ts";
import type { Freshness } from "../ui/pagecache.ts";
import type { LabelFilter, SplitGroup, SplitItem } from "./split.ts";
import { EmptyPanel } from "../ui/kit.tsx";
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

export function RunningStatus({ row }: { row: RunningAgent }) {
  const moving = moveLabel(row), closing = closingNote(row);
  if (moving) return <span className={`chip ${moving.chip === "Moving" ? "run" : "bad"}`}>{moving.chip}</span>;
  if (closing) return closing;
  if (row.status === "asking") return <span className="chip wait">needs you</span>;
  if (["working", "running", "starting"].includes(row.status)) return <span className="cc-running"><span className="drun" role="img" aria-label={row.status === "starting" ? "Starting" : "Working"}/>{runningWhen(row)}</span>;
  return runningWhen(row);
}

export interface RecentSessions { rows: SessionRow[] | null; empty: string | null; count: number; freshness: Freshness | null }
function RecentSessionsSection({ session }: { session: RecentSessions }) {
  return <div id="sec-sessions"><div className="cc-sec"><h4>Past sessions{!!session.count && <span className="n">{session.count}</span>}{session.freshness && <FreshNote freshness={session.freshness} className=" ss-updating"/>}</h4></div>{session.rows ? <div style={{ display: "contents" }}><div className="cc-panel"><ProjectSessions rows={session.rows} all={false}/></div></div> : <EmptyPanel text={session.empty} />}</div>;
}

export function ProjectFallback({ session, machines, running, agents }: { session: RecentSessions; machines: ReactElement; running: ReactElement; agents: ReactElement }) {
  return <><RecentSessionsSection session={session}/><div className="ca-col">{machines}{running}{agents}</div></>;
}

export function ProjectStatus({ cells, openTile }: { cells: readonly { label: string; text: string; key: string; bad?: boolean }[]; openTile: (key: string) => void }) {
  return <>{cells.filter(cell => cell.label && cell.text).map(cell => <button type="button" className="m" key={cell.key} onClick={() => openTile(cell.key)}><b>{cell.label}</b><span className={cell.bad ? "bad" : ""}>{cell.text}</span></button>)}</>;
}

export function SessionTabs({ selected, sessions, agents, select }: { selected: "sessions" | "agents"; sessions: number; agents: number; select: (tab: "sessions" | "agents") => void }) {
  return <>{([["sessions", "Sessions", sessions], ["agents", "Agents", agents]] as const).map(([key, label, count]) =>
    <button key={key} type="button" role="tab" aria-selected={key === selected} onClick={() => select(key)}>{label}{count ? <span className="n">{count}</span> : null}</button>)}</>;
}
