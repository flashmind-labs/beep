import { Fragment } from "react";
import { thoughtLabel, duration, type RowParts, type ToolKind, type TurnState } from "./agentturns.ts";
import { liveText } from "./karaoke.ts";
import { ICON } from "../ui/icons.ts";
import { esc } from "../ui/format.ts";
import { KIND_ICONS } from "./turnrows.ts";
import { SubagentsView, type SubagentRow } from "../agentpane/subagents-view.tsx";
import type { MadeFile, MadeFiles } from "../agentpane/artifacts-state.ts";
import type { Answered } from "./agentlog.ts";

export type ToolActivity = { kind: "tool"; id: string; html: string; at: number; failed: boolean; open: boolean; artifact: string | null; toggle: boolean; keyboard: boolean; mark: string; after: string; layout: { kind: "beep"; head: string } | { kind: "standard"; parts: RowParts; tip: string; since: number | null } };
export type ActivityRow =
  | { kind: "thought"; id: string; html: string; key: string; at: number; text: string; content: string | null; ms: number | null; live: boolean; open: boolean }
  | { kind: "live"; id: string; html: string; state: TurnState; thinking: boolean; start: number | null; doing: string }
  | ToolActivity
  | { kind: "run"; id: string; html: string; at: number; toolKind: ToolKind; label: string; summary: string; open: boolean; tools: ToolActivity[] }
  | { kind: "subagents"; id: string; html: string; at: number; heading: string | null; counts: string; rows: SubagentRow[] }
  | { kind: "folded-pictures"; id: string; html: string; at: number; pictures: string }
  | { kind: "crumb"; id: string; html: string; parent: string; title: string }
  | { kind: "older"; id: string; html: string; text: string }
  | { kind: "note"; id: string; html: string; at: number; value: { kind: "other"; text: string } | { kind: "compacted" } | { kind: "answers"; items: Answered[] } | { kind: "made"; files: MadeFiles; open: boolean } | { kind: "collapse" } };

export function ActivityView({ row }: { row: ActivityRow }) {
  if (row.kind === "note") return <Note row={row} />;
  if (row.kind === "crumb") return <div className="dcrumb"><button id="agup">{row.parent}</button><span>/</span><b>{row.title}</b><span className="sp" /><button className="dbtn" id="agup2">Open parent<kbd>esc</kbd></button></div>;
  if (row.kind === "older") return <div className="dmuted">{row.text}</div>;
  if (row.kind === "folded-pictures") return <div className="dcoderen dfoldpics" data-row={row.at}><span className="pics" dangerouslySetInnerHTML={{ __html: row.pictures }} /></div>;
  if (row.kind === "subagents") return <div className="dsubgrp" data-row={row.at}>{row.heading !== null && <div className="h"><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: SUB_ICON }} /><b>{row.heading}</b><span>{row.counts}</span></div>}<SubagentsView rows={row.rows} /></div>;
  if (row.kind === "run") return <div className={`dtrun${row.open ? " open" : ""}`} data-row={row.at}><button data-run={row.at} aria-expanded={row.open}><span className="ki" dangerouslySetInnerHTML={{ __html: KIND_ICONS[row.toolKind] }} /><span className="vb">{row.label}</span><span className="sm">{row.summary}</span><span className="cv" dangerouslySetInnerHTML={{ __html: ICON.chev }} /></button><div className="bd"><div><div className="in">{row.tools.map(tool => <ActivityView key={tool.id} row={tool} />)}</div></div></div></div>;
  if (row.kind === "tool") {
    const cls = `dcoderen tool ${row.layout.kind === "beep" ? "beep" : "row"}${row.failed ? " failed" : ""}${row.open ? " open" : ""}${row.artifact ? " artifact" : ""}`;
    const mark = <span className="mk" dangerouslySetInnerHTML={{ __html: row.mark }} />;
    return <div className={cls} data-row={row.at} data-at={row.toggle ? row.at : undefined} tabIndex={row.keyboard ? 0 : undefined} role={row.keyboard ? "button" : undefined} aria-expanded={row.keyboard ? row.open : undefined} data-artifact={row.artifact ?? undefined} title={row.artifact ? "Open in the Artifacts tab" : undefined}>{row.layout.kind === "beep" ? <>{mark}<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: row.layout.head }} /></> : <ToolHead parts={row.layout.parts} tip={row.layout.tip} since={row.layout.since} mark={mark} />}<span data-tool-content style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: row.after }} /></div>;
  }
  if (row.kind === "live") {
    const waiting = row.state === "waiting", lead = row.doing || (row.thinking ? "Thinking" : "Working");
    return <div className={`dthink dlive${waiting ? "" : " live"}`}><span className="lb" title={waiting ? undefined : lead}>{waiting ? "Waiting for you" : lead}</span>{row.start !== null && <span className="since" data-since={row.start}>{duration(Date.now() - row.start)}</span>}</div>;
  }
  return <div className={`dthink${row.live ? " live" : ""}${row.open ? " open" : ""}`} data-row={row.at}><button data-thought={row.at} aria-expanded={row.open}><span className="lb">{thoughtLabel(row.ms)}</span><span className="cv" dangerouslySetInnerHTML={{ __html: ICON.chev }} /></button><div className="bd"><div>{row.live ? <div className="tx"><div className="kk" data-kk={row.key}>{liveText(row.text)}</div></div> : <div className="tx md" dangerouslySetInnerHTML={{ __html: row.content ?? esc(row.text).replaceAll("\n", "<br>") }} />}</div></div></div>;
}

function Note({ row }: { row: Extract<ActivityRow, { kind: "note" }> }) {
  const note = row.value;
  if (note.kind === "collapse") return <div><button className="dprogress-collapse" data-turn={row.at} data-collapse aria-label="Collapse activity" title="Collapse activity" aria-expanded="true" dangerouslySetInnerHTML={{ __html: ICON.chev }} /></div>;
  if (note.kind === "other") return <div className="dcoderen other" data-row={row.at}>{note.text}</div>;
  if (note.kind === "compacted") return <div className="dcoderen compacted" data-row={row.at}><span>Conversation compacted</span></div>;
  if (note.kind === "answers") return <div className="dcoderen qa" data-row={row.at}>{note.items.map((item, index) => <Fragment key={index}>{item.question && <span className="q">{item.question}</span>}<span className="a">{item.answer}</span></Fragment>)}</div>;
  if (note.files.files.length === 1) return <div className="dcoderen other mf" data-row={row.at}>Made <MadeChip file={note.files.files[0]} cls="mf-name" /></div>;
  return <div className={`dcoderen other mf${note.open ? " open" : ""}`} data-row={row.at}><button className="mf-head" data-madeat={row.at} aria-expanded={note.open}><span className="mf-car">›</span>Made {note.files.files.length} files<small>{note.files.counts}</small></button>{note.files.groups.map(group => <div className="mf-grp" key={group.group}><small>{group.label}</small><div className="mf-chips">{group.files.map(file => <MadeChip key={file.path} file={file} cls="mf-chip" />)}</div></div>)}</div>;
}

function MadeChip({ file, cls }: { file: MadeFile; cls: string }) {
  return <button className={cls} data-made={file.path} title={file.path}>{file.name}</button>;
}

export const SUB_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V13a5 5 0 0 0 5 5h4.5"/></svg>';

function ToolHead({ parts, tip, since, mark }: { parts: RowParts; tip: string; since: number | null; mark: React.ReactNode }) {
  const counts = /^\+(\d+) -(\d+)$/.exec(parts.detail);
  return <><span className="ki" dangerouslySetInnerHTML={{ __html: KIND_ICONS[parts.kind] }} /><span className="vb">{parts.verb}</span><code className="tt" title={tip}>{parts.target}</code><span className={`rm${parts.bad ? " bad" : ""}`}>{since !== null ? <span data-since={since} title="Time since this window saw the tool running">{duration(Date.now() - since)}</span> : counts ? <><span className="pl">+{counts[1]}</span><span className="mi">-{counts[2]}</span></> : parts.detail}</span>{mark}</>;
}
