import { Fragment, useLayoutEffect, useRef, type ReactNode } from "react";
import { harnessInner, harnessMark } from "../ui/harnessicon.ts";
import { moveLabel } from "../agents/move-store.ts";
import { closingNote } from "../agents/closing.ts";
import { runningWhen, type RunningAgent } from "../agents/running-state.ts";
import { ago } from "../ui/format.ts";
import { terminalNote, terminalWhere, type TerminalRow } from "../terminals/terminals.ts";
import type { Freshness } from "../ui/pagecache.ts";
import type { TileData } from "./board.ts";
import type { LabelFilter, SplitGroup, SplitItem } from "./split.ts";

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

export function Harness({ agent }: { agent: string }) {
  const safe = /^[a-z0-9-]+$/.test(agent) ? ` ${agent}` : "";
  return <span className={`cc-ag${safe}${harnessMark(agent) ? " mark" : ""}`} dangerouslySetInnerHTML={{ __html: harnessInner(agent) }}/>;
}

export function RunningStatus({ row }: { row: RunningAgent }) {
  const moving = moveLabel(row), closing = closingNote(row);
  if (moving) return <span className={`chip ${moving.chip === "Moving" ? "run" : "bad"}`}>{moving.chip}</span>;
  if (closing) return closing;
  if (row.status === "asking") return <span className="chip wait">needs you</span>;
  if (["working", "running", "starting"].includes(row.status)) return <span className="cc-running"><span className="dspin" role="img" aria-label={row.status === "starting" ? "Starting" : "Working"}/>{runningWhen(row)}</span>;
  return runningWhen(row);
}

export function Chip({ cls, text, spin }: { cls: string; text: string; spin: boolean }) {
  return <span className={cls ? `chip ${cls}` : "chip"}>{spin && <span className="dspin"/>}{text}</span>;
}

export function RetainedHost({ host, contents = true }: { host: HTMLElement; contents?: boolean }) {
  const slot = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const target = slot.current;
    if (!target) return;
    if (host.parentElement !== target) target.append(host);
    return () => { if (host.parentElement === target) host.remove(); };
  }, [host]);
  return <div ref={slot} style={contents ? { display: "contents" } : undefined}/>;
}

export interface RecentSessions { host: HTMLElement | null; empty: string | null; count: number; freshness: Freshness | null }
export function RecentSessionsSection({ session }: { session: RecentSessions }) {
  return <div id="sec-sessions"><div className="cc-sec"><h4>Past sessions{!!session.count && <span className="n">{session.count}</span>}{session.freshness?.spin ? <span className="dspin sw-spin ss-updating" title="Updating"/> : session.freshness?.text ? <small className="sw-age ss-updating">{session.freshness.text}</small> : null}</h4></div>{session.host ? <RetainedHost host={session.host}/> : <div className="cc-panel cc-empty"><span>{session.empty}</span></div>}</div>;
}

export function ProjectFallback({ session, machines, running, agents }: { session: RecentSessions; machines: HTMLElement; running: HTMLElement; agents: HTMLElement }) {
  return <><RecentSessionsSection session={session}/><div className="ca-col"><RetainedHost host={machines}/><RetainedHost host={running}/><RetainedHost host={agents} contents={false}/></div></>;
}

function OverviewRow({ item, openTile }: { item: SplitItem; openTile: (key: string) => void }) {
  return <div className={`cc-row cc-line${item.lead ? "" : " bare"}`} onClick={() => item.open ? item.open() : openTile(item.key)} onContextMenu={item.menu ? event => item.menu?.(event.nativeEvent) : undefined}>{item.lead}<button type="button" className="t" title={item.title}><b>{item.title}</b><span>{item.sub}</span></button><span className="r">{item.right}{item.tags?.slice(0, 3).map(label => <i className="pj-tag" key={label}>{label}</i>)}</span></div>;
}

function OverviewBox({ title, count, open, children }: { title: string; count: number | string; open: () => void; children: ReactNode }) {
  return <div id=""><div className="cc-sec"><h4><button type="button" className="dbtn lk" onClick={open}>{title}</button>{!!count && <span className="n">{count}</span>}</h4></div><div className="cc-panel">{children}</div></div>;
}

export function ProjectOverview({ run, prs, issues, prNote, issueNote, machines, terminals, session, openTile, openTerminal }: {
  run: SplitItem[]; prs: SplitItem[]; issues: SplitItem[]; prNote: string | undefined; issueNote: string | undefined;
  machines: TileData; terminals: TerminalRow[]; session: RecentSessions;
  openTile: (key: string) => void; openTerminal: (terminal: TerminalRow) => void;
}) {
  const rows = (items: SplitItem[]) => items.map(item => <OverviewRow key={item.key} item={item} openTile={openTile}/>);
  const empty = (title: string, note: string | undefined) => <div id=""><div className="cc-sec"><h4>{title}</h4></div><div className="cc-panel cc-empty"><span>{note}</span></div></div>;
  return <><div>
    {!!run.length && <OverviewBox title="Running" count={run.length} open={() => openTile("running")}>{rows(run.slice(0, 5))}</OverviewBox>}
    <RecentSessionsSection session={session}/>
    {!!machines.rows.length && <OverviewBox title="Machines" count={machines.count} open={() => openTile("machines")}>{machines.rows.slice(0, 5).map((row, index) => <div key={row.id ?? index} className="cc-row cc-line" title={row.title || row.text || ""} onClick={() => openTile("machines")}><span className={`ddot${row.dot ? " off" : ""}`}/><button type="button" className="t" title={row.text || ""}><b>{row.text}</b><span>{row.note}</span></button><span className="r"/></div>)}</OverviewBox>}
    {!!terminals.length && <OverviewBox title="Terminals" count={terminals.length} open={() => openTile("machines")}>{terminals.slice(0, 5).map(terminal => <div key={`${terminal.machine_id}:${terminal.id}`} className="cc-row cc-line" title={`${terminal.machine_name}: ${terminal.command} in ${terminal.cwd}`} onClick={() => openTerminal(terminal)}><span className={`ddot${terminal.state === "running" ? "" : " off"}`}/><button type="button" className="t" title={terminal.name}><b>{terminal.name}</b><span>{terminal.machine_name} · {terminalWhere(terminal)}</span></button><span className="r">{terminalNote(terminal, ago)}</span></div>)}</OverviewBox>}
  </div><div>
    {prs.length ? <OverviewBox title="Pull requests" count={prs.length} open={() => openTile(prs[0].key)}>{rows(prs.slice(0, 5))}</OverviewBox> : empty("Pull requests", prNote)}
    {issues.length ? <div id=""><div className="cc-sec"><h4><button type="button" className="dbtn lk" onClick={() => openTile("issues")}>Issues</button><span className="n">{issues.length}</span></h4></div><div className="cc-panel pj-scroll pj-ov-issues">{rows(issues)}</div></div> : empty("Issues", issueNote)}
  </div></>;
}

export function ProjectStatus({ cells, openTile }: { cells: readonly { label: string; text: string; key: string; bad?: boolean }[]; openTile: (key: string) => void }) {
  return <>{cells.filter(cell => cell.label && cell.text).map(cell => <button type="button" className="m" key={cell.key} onClick={() => openTile(cell.key)}><b>{cell.label}</b><span className={cell.bad ? "bad" : ""}>{cell.text}</span></button>)}</>;
}

export function SessionTabs({ selected, sessions, agents, select }: { selected: "sessions" | "agents"; sessions: number; agents: number; select: (tab: "sessions" | "agents") => void }) {
  return <>{([["sessions", "Sessions", sessions], ["agents", "Agents", agents]] as const).map(([key, label, count]) =>
    <button key={key} type="button" role="tab" aria-selected={key === selected} onClick={() => select(key)}>{label}{count ? <span className="n">{count}</span> : null}</button>)}</>;
}
