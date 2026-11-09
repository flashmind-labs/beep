import { ICON } from "../ui/icons.ts";

export interface SubagentRow {
  at: number; title: string; agent: string; meta: string; mark: string;
  target: { kind: "child"; id: string; key: string } | { kind: "native" } | { kind: "summary" };
  dismiss: boolean; stoppable: boolean; result: string | null;
}

export function SubagentsView({ rows }: { rows: readonly SubagentRow[] }) {
  return <>{rows.map(row => <div key={row.at} style={{ display: "contents" }}><button className={`dsubrow${row.stoppable ? " stoppable" : ""}`} data-kid={row.target.kind === "child" ? row.target.id : undefined} data-key={row.target.kind === "child" ? row.target.key : undefined} data-native={row.target.kind === "native" ? row.at : undefined} data-subsummary={row.target.kind === "summary" ? row.at : undefined} title={`Open ${row.title}`}><span className="mk" dangerouslySetInnerHTML={{ __html: row.mark }} /><span className="t"><b>{row.title}{row.agent && <span className="kind">{row.agent}</span>}</b><small>{row.meta}</small></span>{row.dismiss && <em className="x" data-subclose={row.at} title="Dismiss from pane" dangerouslySetInnerHTML={{ __html: ICON.x }} />}{row.stoppable && <em className="kstop" data-kidstop title={`Stop ${row.title}`}>Stop</em>}</button>{row.result !== null && <div className="dmuted">{row.result}</div>}</div>)}</>;
}
