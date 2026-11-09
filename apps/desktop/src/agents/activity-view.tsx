import { thoughtLabel, duration, type RowParts, type ToolKind, type TurnState } from "./agentturns.ts";
import { liveText } from "./karaoke.ts";
import { ICON } from "../ui/icons.ts";
import { esc } from "../ui/format.ts";
import { KIND_ICONS } from "./turnrows.ts";

export type ToolActivity = { kind: "tool"; id: string; html: string; at: number; failed: boolean; open: boolean; artifact: string | null; toggle: boolean; keyboard: boolean; mark: string; after: string; layout: { kind: "beep"; head: string } | { kind: "standard"; parts: RowParts; tip: string; since: number | null } };
export type ActivityRow =
  | { kind: "thought"; id: string; html: string; key: string; at: number; text: string; content: string | null; ms: number | null; live: boolean; open: boolean }
  | { kind: "live"; id: string; html: string; state: TurnState; thinking: boolean; start: number | null; doing: string }
  | ToolActivity
  | { kind: "run"; id: string; html: string; at: number; toolKind: ToolKind; label: string; summary: string; open: boolean; tools: ToolActivity[] };

export function ActivityView({ row }: { row: ActivityRow }) {
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

function ToolHead({ parts, tip, since, mark }: { parts: RowParts; tip: string; since: number | null; mark: React.ReactNode }) {
  const counts = /^\+(\d+) -(\d+)$/.exec(parts.detail);
  return <><span className="ki" dangerouslySetInnerHTML={{ __html: KIND_ICONS[parts.kind] }} /><span className="vb">{parts.verb}</span><code className="tt" title={tip}>{parts.target}</code><span className={`rm${parts.bad ? " bad" : ""}`}>{since !== null ? <span data-since={since} title="Time since this window saw the tool running">{duration(Date.now() - since)}</span> : counts ? <><span className="pl">+{counts[1]}</span><span className="mi">-{counts[2]}</span></> : parts.detail}</span>{mark}</>;
}
