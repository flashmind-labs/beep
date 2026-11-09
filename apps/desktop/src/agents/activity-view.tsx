import { thoughtLabel, duration, type TurnState } from "./agentturns.ts";
import { liveText } from "./karaoke.ts";
import { ICON } from "../ui/icons.ts";
import { esc } from "../ui/format.ts";

export type ActivityRow =
  | { kind: "thought"; id: string; html: string; key: string; at: number; text: string; content: string | null; ms: number | null; live: boolean; open: boolean }
  | { kind: "live"; id: string; html: string; state: TurnState; thinking: boolean; start: number | null; doing: string };

export function ActivityView({ row }: { row: ActivityRow }) {
  if (row.kind === "live") {
    const waiting = row.state === "waiting", lead = row.doing || (row.thinking ? "Thinking" : "Working");
    return <div className={`dthink dlive${waiting ? "" : " live"}`}><span className="lb" title={waiting ? undefined : lead}>{waiting ? "Waiting for you" : lead}</span>{row.start !== null && <span className="since" data-since={row.start}>{duration(Date.now() - row.start)}</span>}</div>;
  }
  return <div className={`dthink${row.live ? " live" : ""}${row.open ? " open" : ""}`} data-row={row.at}><button data-thought={row.at} aria-expanded={row.open}><span className="lb">{thoughtLabel(row.ms)}</span><span className="cv" dangerouslySetInnerHTML={{ __html: ICON.chev }} /></button><div className="bd"><div>{row.live ? <div className="tx"><div className="kk" data-kk={row.key}>{liveText(row.text)}</div></div> : <div className="tx md" dangerouslySetInnerHTML={{ __html: row.content ?? esc(row.text).replaceAll("\n", "<br>") }} />}</div></div></div>;
}
