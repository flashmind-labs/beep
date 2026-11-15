// A session body's log shell (the question and plan card, notes) and the
// transcript inside it, drawn from the turns `logwindow.ts` picks.
import { memo, useCallback, useSyncExternalStore } from "react";
import { Button, Spinner } from "../ui/kit.tsx";
import type { AgentQuestion } from "./agents.ts";
import { Icon } from "../ui/icons.tsx";
import type { DrawnTurn, KeyedRow, LogWindow, TranscriptRow } from "./logwindow.tsx";
import { AgentTurnHeading } from "./turn-heading-view.tsx";
import { AgentContext, AgentError, AgentPrompt, AgentReply } from "./reply-view.tsx";
import { ActivityView } from "./activity-view.tsx";
import { questionRef } from "./mode-view.tsx";
import { SubagentsView, type SubagentRow } from "../agentpane/subagents-view.tsx";
import { plural } from "../ui/format.ts";

/// A step of the plan: how many subagents ran under it, and their rows once
/// it is opened.
export interface PlanRow { status: string; content: string; count: number; subs: readonly SubagentRow[] | null }

export interface LogShell {
  why: string | null;
  starting: boolean;
  log: boolean;
  question: AgentQuestion | null;
  questionKey: string | null;
  answering: boolean;
  plan: readonly PlanRow[];
  asking: boolean;
  error: string | null;
  external: boolean;
  /// `connecting`: the log is what this device kept, and its machine has not answered yet.
  note: { text: string; tone: "error" | "muted" | "connecting" } | null;
  retry: boolean;
}

export function AgentLogShell({ value, log }: { value: LogShell; log: LogWindow }) {
  const question = value.question;
  const allow = question?.options.findIndex(option => option.allow) ?? -1;
  const deny = question?.options.findIndex(option => !option.allow) ?? -1;
  return <><div className="dagmain">
    {value.why && <div className="dmuted" data-log-part="why">{value.why}</div>}
    {value.log ? <div className="dcoderlog" ref={log.attach}><TranscriptLog log={log} /></div> : value.starting && <div className="dmuted" data-log-part="starting"><Spinner /> Starting the agent…</div>}
    {(question || value.plan.length > 0) && <div className="dcard dcoder" data-log-part="card">
      {question && <div className="q"><b>{question.text}</b>{question.detail && <code>{question.detail}</code>}<div className="dups" ref={questionRef({ key: value.questionKey ?? "", allows: question.options.map(option => option.allow), disabled: value.answering })}>{question.options.map((option, index) => <Button key={index} look={index === allow ? "primary" : "secondary"} danger={index === deny} data-agopt={index} data-agquestion={value.questionKey ?? ""} data-agallow={String(option.allow)} disabled={value.answering}>{option.name}{index === allow ? <kbd>⌘<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M20 5v7a3 3 0 0 1-3 3H5m4-4l-4 4 4 4" /></svg></kbd> : index === deny ? <kbd>⌘D</kbd> : null}</Button>)}</div></div>}
      {value.plan.length > 0 && <><div className="sec">Plan</div><div className="plan">{value.plan.map((item, index) => <PlanStepView key={index} item={item} index={index} asking={value.asking} />)}</div></>}
    </div>}
    {value.error && (value.external ? <div className="derr" data-log-part="error">{value.error}</div> : <div data-log-part="error"><AgentError message={value.error} /></div>)}
    {value.note && <div className={value.note.tone === "error" ? "derr" : value.note.tone === "connecting" ? "dmuted dlate" : "dmuted"} data-log-part="note">{value.note.text}</div>}
    {value.retry && <Button look="quiet" id="agresume" data-log-part="retry">Retry</Button>}
  </div><button className="dagjump" id="agjump" hidden>Jump to latest<Icon name="pull" /></button></>;
}

/// A step of the plan. One that subagents ran under opens to their rows,
/// which open the subagent as they do in the log.
function PlanStepView({ item, index, asking }: { item: PlanRow; index: number; asking: boolean }) {
  const mark = <span className="dmark">{item.status === "completed" ? <span className="dok"><Icon name="check" /></span> : item.status === "in_progress" ? asking ? <span className="ddot hot" /> : <Spinner dot /> : <span className="ddot off" />}</span>;
  const text = <span className={item.status === "pending" ? "dmut" : undefined}>{item.content}</span>;
  if (!item.count) return <div className="row">{mark}{text}</div>;

  return <><button className="row step" data-planstep={index} aria-expanded={item.subs !== null}>{mark}{text}<small>{plural(item.count, "agent", "agents")}</small><Icon name="chev" /></button>{item.subs && <div className="subs"><SubagentsView rows={item.subs} /></div>}</>;
}

/// The log's turns as React draws them; the window decides which.
export function TranscriptLog({ log }: { log: LogWindow }) {
  const view = useSyncExternalStore(log.subscribe, log.snapshot, log.snapshot);
  return <><Spacer height={view.top} />{view.turns.map(turn => <AgentTurn key={turn.key} turn={turn} log={log} />)}<Spacer height={view.bottom} /></>;
}

const Spacer = ({ height }: { height: number | null }) => <div className="agspace" hidden={height === null} style={{ height: `${height ?? 0}px` }} />;

const AgentTurn = memo(function AgentTurn({ turn, log }: { turn: DrawnTurn; log: LogWindow }) {
  const key = turn.key;
  const ref = useCallback((el: HTMLDivElement | null) => {
    if (!el) return;
    log.elements.set(key, el);
    log.watch.observe(el);
    return () => {
      log.watch.unobserve(el);
      if (log.elements.get(key) === el) log.elements.delete(key);
    };
  }, [log, key]);
  return <div ref={ref} className="agturn" data-find-key={key} hidden={!turn.rows.length}><TurnRows rows={turn.rows} /></div>;
});

export function TurnRows({ rows }: { rows: readonly KeyedRow[] }) {
  return rows.map(row => <RowSlot key={row.key} row={row} />);
}

const RowSlot = memo(function RowSlot({ row }: { row: KeyedRow }) {
  return <div style={{ display: "contents" }}><TranscriptRowView row={row.row} /></div>;
});

function TranscriptRowView({ row }: { row: TranscriptRow }) {
  switch (row.kind) {
    case "heading": return <AgentTurnHeading value={row} />;
    case "blocked": return <AgentError message={row.message} />;
    case "reply": return <AgentReply reply={row} />;
    case "prompt": return <AgentPrompt prompt={row} />;
    case "context": return <AgentContext row={row} />;
    default: return <ActivityView row={row} />;
  }
}

