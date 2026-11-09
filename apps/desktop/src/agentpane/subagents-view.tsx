import { Fragment } from "react";
import { ICON } from "../ui/icons.ts";

export interface SubagentRow {
  at: number; title: string; agent: string; meta: string; mark: string;
  target: { kind: "child"; id: string; key: string } | { kind: "native" } | { kind: "summary" };
  dismiss: boolean; stoppable: boolean; result: string | null;
  prompt: { text: string; open: boolean } | null;
}

export function SubagentsView({ rows }: { rows: readonly SubagentRow[] }) {
  return <>{rows.map(row => <Fragment key={row.at}><Subagent row={row} /></Fragment>)}</>;
}

function Subagent({ row }: { row: SubagentRow }) {
  const button = <button className={`dsubrow${row.stoppable ? " stoppable" : ""}`} data-kid={row.target.kind === "child" ? row.target.id : undefined} data-key={row.target.kind === "child" ? row.target.key : undefined} data-native={row.target.kind === "native" ? row.at : undefined} data-subsummary={row.target.kind === "summary" ? row.at : undefined} title={`Open ${row.title}`}><span className="mk" dangerouslySetInnerHTML={{ __html: row.mark }} /><span className="t"><b>{row.title}{row.agent && <span className="kind">{row.agent}</span>}</b><small>{row.meta}</small></span>{row.dismiss && <em className="x" data-subclose={row.at} title="Dismiss from pane" dangerouslySetInnerHTML={{ __html: ICON.x }} />}{row.stoppable && <em className="kstop" data-kidstop title={`Stop ${row.title}`}>Stop</em>}</button>;
  if (row.result !== null) return <>{button}<div className="dmuted">{row.result}</div></>;
  if (!row.prompt) return button;
  return <div className="dsubprompt" data-row={row.at} data-at={row.at}>{button}<button className="prompt-toggle" data-prompt={row.at} aria-expanded={row.prompt.open}><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.chev }} />Prompt</button>{row.prompt.open && <pre>{row.prompt.text}</pre>}</div>;
}
