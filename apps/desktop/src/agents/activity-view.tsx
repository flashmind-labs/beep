// The work rows of a coding agent's log: thoughts, tool calls and runs of
// them, subagents, notes and the live line.
import { Fragment, useSyncExternalStore } from "react";
import { diffBlocks, thoughtLabel, withoutDiffs, KIND_ICONS, type RowParts, type ToolKind, type TurnState } from "./agentturns.ts";
import { liveText } from "./karaoke.ts";
import { Icon } from "../ui/icons.tsx";
import { SubagentsView, type SubagentRow } from "../agentpane/subagents-view.tsx";
import type { MadeFile, MadeFiles } from "../agentpane/artifacts-state.ts";
import type { Answered } from "./agentlog.ts";
import type { ToolDetails } from "./tool-details.ts";
import { EntryTime, Since, StepMarkView, type StepMark } from "./turn-heading-view.tsx";
import { waitedFor, type WaitLine } from "./waiting-state.ts";
import { onShownChange, paneShown } from "../ui/active.ts";
import { Button, DotMatrix } from "../ui/kit.tsx";
import { highlight, languageOf } from "../agentpane/files-state.ts";

/// What an opened tool call shows under its row.
export type ToolBody =
  /// A Beep call: its command or prompt, and its output's tail or all of it.
  | { kind: "beep"; detail: string | null; output: { text: string; whole: boolean } | null; more: string | null }
  /// A command, colored when it is a shell line, then its diffs and output.
  | { kind: "row"; command: { kind: "shell"; html: string } | { kind: "text"; text: string } | null; output: string }
  | { kind: "details"; details: ToolDetails };

/// The head of a call to the session's Beep tools: what it did, the machine,
/// the first line of its command or prompt, and how it stands.
interface BeepHead { tip: string; label: string; machine: string; line: string | null; status: string; since: number | null }

export interface ToolActivity {
  kind: "tool"; id: string; at: number; failed: boolean; open: boolean; artifact: string | null;
  sentAt?: number | null;
  toggle: boolean; keyboard: boolean; mark: StepMark;
  /// Media files the call names, which open in the media pane.
  media: string[];
  body: ToolBody | null;
  /// Picture tiles the call showed, filled in by `fillAgentPics`.
  pictures: string;
  layout: { kind: "beep"; head: BeepHead } | { kind: "standard"; parts: RowParts; tip: string; since: number | null };
}

export type ActivityRow =
  | { kind: "thought"; id: string; key: string; at: number; text: string; content: string | null; ms: number | null; live: boolean; open: boolean }
  | { kind: "live"; id: string; state: TurnState; thinking: boolean; start: number | null; doing: string }
  /// In place of the live line once the agent replied while background work still runs.
  | { kind: "waiting"; id: string; line: WaitLine }
  | ToolActivity
  | { kind: "run"; id: string; at: number; toolKind: ToolKind; label: string; summary: string; open: boolean; tools: ActivityRow[] }
  | { kind: "subagents"; id: string; at: number; heading: string | null; counts: string; rows: SubagentRow[] }
  /// Pictures shown by steps folded away, or by a subagent's call.
  | { kind: "pictures"; id: string; at: number | null; folded: boolean; pictures: string }
  | { kind: "crumb"; id: string; parent: string; title: string }
  | { kind: "older"; id: string; text: string }
  /// Where the chat went on from or to under another harness; opens that session when `open`.
  | { kind: "continued"; id: string; way: "from" | "in"; text: string; open: boolean }
  | { kind: "note"; id: string; at: number; value: { kind: "other"; text: string } | { kind: "compacted"; summary: string | null } | { kind: "answers"; items: Answered[] } | { kind: "made"; files: MadeFiles; open: boolean } | { kind: "collapse" } };

export function ActivityView({ row }: { row: ActivityRow }) {
  switch (row.kind) {
    case "note": return <Note row={row} />;
    case "crumb": return <div className="dcrumb"><button id="agup">{row.parent}</button><span>/</span><b>{row.title}</b><span className="sp" /><Button id="agup2">Open parent<kbd>esc</kbd></Button></div>;
    case "continued": return <div className="dcoderen compacted dcontinued">{row.open ? <button data-continued={row.way}>{row.text}</button> : <span>{row.text}</span>}</div>;
    case "older": return <div className="dmuted dolder">{row.text}</div>;
    case "pictures": return row.folded
      ? <div className="dcoderen dfoldpics" data-row={row.at ?? undefined}><span className="pics" dangerouslySetInnerHTML={{ __html: row.pictures }} /></div>
      : <div className="dcoderen tool"><span className="pics" dangerouslySetInnerHTML={{ __html: row.pictures }} /></div>;
    case "subagents": return <div className="dsubgrp" data-row={row.at}>{row.heading !== null && <div className="h"><SubIcon /><b>{row.heading}</b><span>{row.counts}</span></div>}<SubagentsView rows={row.rows} /></div>;
    case "run": return <div className={`dtrun${row.open ? " open" : ""}`} data-row={row.at}><button data-run={row.at} aria-expanded={row.open}><span className="ki" dangerouslySetInnerHTML={{ __html: KIND_ICONS[row.toolKind] }} /><span className="vb">{row.label}</span><span className="sm">{row.summary}</span><span className="cv"><Icon name="chev" /></span></button><div className="bd"><div><div className="in">{row.tools.map(tool => <ActivityView key={`${tool.kind}:${tool.id}`} row={tool} />)}</div></div></div></div>;
    case "tool": return <Tool row={row} />;
    case "live": return <LiveLine row={row} />;
    case "waiting": return <WaitingLine line={row.line} />;
    case "thought": return <Thought row={row} />;
  }
}

function Thought({ row }: { row: Extract<ActivityRow, { kind: "thought" }> }) {
  const text = row.live ? <div className="tx"><div className="kk" data-kk={row.key}>{liveText(row.text)}</div></div>
    : row.content !== null ? <div className="tx md" dangerouslySetInnerHTML={{ __html: row.content }} />
    : <div className="tx md">{row.text.split("\n").map((line, index) => <Fragment key={index}>{index > 0 && <br />}{line}</Fragment>)}</div>;
  return <div className={`dthink${row.live ? " live" : ""}${row.open ? " open" : ""}`} data-row={row.at}><button data-thought={row.at} aria-expanded={row.open}><span className="lb">{thoughtLabel(row.ms)}</span><span className="cv"><Icon name="chev" /></span></button><div className="bd"><div>{text}</div></div></div>;
}

/// A running turn's one live line: what it does now, or "Thinking" or
/// "Working" until there is one, and how long it has run. Waiting on the
/// user does not sweep.
function LiveLine({ row }: { row: Extract<ActivityRow, { kind: "live" }> }) {
  const waiting = row.state === "waiting", lead = row.doing || (row.thinking ? "Thinking" : "Working");
  return <div className={`dthink dlive${waiting ? "" : " live"}`}>{!waiting && <DotMatrix />}<span className="lb" title={waiting ? undefined : lead}>{waiting ? "Waiting for you" : lead}</span>{row.start !== null && <Since className="since" start={row.start} />}</div>;
}

/// "Waiting for 1 subagent · Upgrade all deps · 12 min", still. A click opens
/// that subagent, or the Running tab for a command.
function WaitingLine({ line }: { line: WaitLine }) {
  useSyncExternalStore(subscribeMinute, currentMinute);
  const waited = waitedFor(Date.now() - line.start);
  const target = line.target;
  return <div className="dthink dlive"><button data-wait={target.kind === "subagent" ? target.at : "running"} title={target.kind === "subagent" ? `Open ${line.title}` : "Show it in the Running tab"}><span className="lb">{line.label}</span><span className="ln">· {line.title}</span>{waited !== null && <span className="since">· {waited}</span>}</button></div>;
}

// The waiting lines move once a minute, on one timer that stops while none
// shows or the pane hides; shown again, they catch up at once.
const minuteListeners = new Set<() => void>();
let minuteTimer: ReturnType<typeof setInterval> | null = null;

function subscribeMinute(listener: () => void) {
  minuteListeners.add(listener);
  tickMinutes();
  return () => { minuteListeners.delete(listener); tickMinutes(); };
}

function tickMinutes() {
  const run = minuteListeners.size > 0 && paneShown();
  if (run && minuteTimer === null) minuteTimer = setInterval(nextMinute, 60_000);
  else if (!run && minuteTimer !== null) { clearInterval(minuteTimer); minuteTimer = null; }
}

const currentMinute = () => Math.floor(Date.now() / 60_000);
function nextMinute() { for (const listener of minuteListeners) listener(); }

onShownChange(shown => { tickMinutes(); if (shown) nextMinute(); }, true);

/// A tool call as one row: its mark, icon, verb, its target in mono, the
/// right side (an edit's +2 -1, an exit code) and, opened, what it did.
function Tool({ row }: { row: ToolActivity }) {
  const cls = `dcoderen tool ${row.layout.kind === "beep" ? "beep" : "row"}${row.failed ? " failed" : ""}${row.open ? " open" : ""}${row.artifact ? " artifact" : ""}`;
  const mark = <span className="mk"><StepMarkView mark={row.mark} /></span>;
  return <div className={cls} data-row={row.at} data-at={row.toggle ? row.at : undefined} tabIndex={row.keyboard ? 0 : undefined} role={row.keyboard ? "button" : undefined} aria-expanded={row.keyboard ? row.open : undefined} data-artifact={row.artifact ?? undefined} title={row.artifact ? "Open in the Artifacts tab" : undefined}>
    {row.layout.kind === "beep" ? <>{mark}<BeepHeadView head={row.layout.head} /></> : <ToolHead parts={row.layout.parts} tip={row.layout.tip} since={row.layout.since} mark={mark} />}
    <EntryTime at={row.sentAt ?? null} />
    <span data-tool-content style={{ display: "contents" }}>
      {row.media.length > 0 && <span className="dmedia">{row.media.map(path => <button key={path} data-media={path} title={`Open ${path}`}><Icon name="image" />{path.slice(path.lastIndexOf("/") + 1)}</button>)}</span>}
      {row.body && <ToolBodyView body={row.body} at={row.at} />}
      {row.pictures && <span className="pics" dangerouslySetInnerHTML={{ __html: row.pictures }} />}
    </span>
  </div>;
}

function ToolHead({ parts, tip, since, mark }: { parts: RowParts; tip: string; since: number | null; mark: React.ReactNode }) {
  const counts = /^\+(\d+) -(\d+)$/.exec(parts.detail);
  return <><span className="ki" dangerouslySetInnerHTML={{ __html: KIND_ICONS[parts.kind] }} /><span className="vb">{parts.verb}</span><code className="tt" title={tip}>{parts.target}</code><span className={`rm${parts.bad ? " bad" : ""}`}>{since !== null ? <ToolClock since={since} /> : counts ? <><span className="pl">+{counts[1]}</span><span className="mi">-{counts[2]}</span></> : parts.detail}</span>{mark}</>;
}

function BeepHeadView({ head }: { head: BeepHead }) {
  return <><span className="tt bp" title={head.tip}><span className="bi" dangerouslySetInnerHTML={{ __html: KIND_ICONS.beep }} /><span className="v">{head.label}</span>{head.machine && <span className="mc">{head.machine}</span>}{head.line !== null && <code>{head.line}</code>}</span>
    <span className="bs">{head.status}{head.since !== null && <> · <ToolClock since={head.since} /></>}</span></>;
}

const ToolClock = ({ since }: { since: number }) => <Since start={since} title="Time since this window saw the tool running" />;

function ToolBodyView({ body, at }: { body: ToolBody; at: number }) {
  if (body.kind === "details") return <Details details={body.details} />;
  if (body.kind === "row") return <>{body.command && (body.command.kind === "shell" ? <pre className="cmd" dangerouslySetInnerHTML={{ __html: body.command.html }} /> : <pre className="cmd">{body.command.text}</pre>)}<ToolOutput output={body.output} /></>;
  return <>{body.detail !== null && <pre className="cmd">{body.detail}</pre>}{body.output && <pre className={`out${body.output.whole ? " all" : ""}`}>{body.output.text}</pre>}{body.more !== null && <button className="more" data-all={at}>{body.more}</button>}</>;
}

function Details({ details }: { details: ToolDetails }) {
  return <div className="call-details"><dl><dt>Tool</dt><dd>{details.tool}</dd></dl><h4>{details.truncated ? "Arguments (truncated)" : "Command"}</h4><pre>{details.command}</pre><h4>Output{details.outputTruncated ? " (truncated)" : ""}</h4>{details.output.kind === "diffs" ? <ToolOutput output={details.output.output} /> : <pre>{details.output.text}</pre>}</div>;
}

/// Each diff in an output as its changed lines, then the rest in a mono box
/// that scrolls.
function ToolOutput({ output }: { output: string }) {
  const diffs = diffBlocks(output), rest = diffs.length ? withoutDiffs(output) : output;
  return <>{diffs.map((diff, index) => <div className="ddiff" key={index}><div className="dh">{diff.path}</div>{diff.lines.map((line, at) => <div key={at} className={line.mark === "+" ? "da" : line.mark === "-" ? "dr" : "dc"}><span className="dk">{line.mark || " "}</span><span dangerouslySetInnerHTML={{ __html: highlight(line.text, languageOf(diff.path)) }} /></div>)}{diff.more > 0 && <div className="dm">{diff.more} more lines</div>}</div>)}{rest && <pre className="out box">{rest}</pre>}</>;
}

function Note({ row }: { row: Extract<ActivityRow, { kind: "note" }> }) {
  const note = row.value;
  if (note.kind === "collapse") return <div><button className="dprogress-collapse" data-turn={row.at} data-collapse aria-label="Collapse activity" title="Collapse activity" aria-expanded="true"><Icon name="chev" /></button></div>;
  if (note.kind === "other") return <div className="dcoderen other" data-row={row.at}>{note.text}</div>;
  if (note.kind === "compacted" && note.summary) return <details className="dcoderen beepctx" data-row={row.at}><summary>Context summarized</summary><pre>{note.summary}</pre></details>;
  if (note.kind === "compacted") return <div className="dcoderen compacted" data-row={row.at}><span>Conversation compacted</span></div>;
  if (note.kind === "answers") return <div className="dcoderen qa" data-row={row.at}>{note.items.map((item, index) => <Fragment key={index}>{item.question && <span className="q">{item.question}</span>}<span className="a">{item.answer}</span></Fragment>)}</div>;
  if (note.files.files.length === 1) return <div className="dcoderen other mf" data-row={row.at}>Made <MadeChip file={note.files.files[0]} cls="mf-name" /></div>;
  return <div className={`dcoderen other mf${note.open ? " open" : ""}`} data-row={row.at}><button className="mf-head" data-madeat={row.at} aria-expanded={note.open}><span className="mf-car">›</span>Made {note.files.files.length} files<small>{note.files.counts}</small></button>{note.files.groups.map(group => <div className="mf-grp" key={group.group}><small>{group.label}</small><div className="mf-chips">{group.files.map(file => <MadeChip key={file.path} file={file} cls="mf-chip" />)}</div></div>)}</div>;
}

function MadeChip({ file, cls }: { file: MadeFile; cls: string }) {
  return <button className={cls} data-made={file.path} title={file.path}>{file.name}</button>;
}

/// The icon of a subagent, which a child's reply without a harness shows too.
export function SubIcon() {
  return <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="6" cy="6" r="2.5" /><circle cx="18" cy="18" r="2.5" /><path d="M6 8.5V13a5 5 0 0 0 5 5h4.5" /></svg>;
}
