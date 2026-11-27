import { Icon } from "../../ui/icons.tsx";
import { StepMarkView, type StepMark } from "./turn-heading-view.tsx";

export interface SubagentRow {
  at: number; title: string; agent: string; meta: string; mark: StepMark;
  target: { kind: "child"; id: string; key: string } | { kind: "native" } | { kind: "summary" };
  dismiss: boolean; stoppable: boolean; result: string | null;
  prompt: { text: string; open: boolean } | null;
}

export function SubagentsView({ rows }: { rows: readonly SubagentRow[] }) {
  return <>{rows.map(row => <Subagent key={row.at} row={row} />)}</>;
}

function Subagent({ row }: { row: SubagentRow }) {
  const button = <button className={`dsubrow${row.stoppable ? " stoppable" : ""}`} data-kid={row.target.kind === "child" ? row.target.id : undefined} data-key={row.target.kind === "child" ? row.target.key : undefined} data-native={row.target.kind === "native" ? row.at : undefined} data-subsummary={row.target.kind === "summary" ? row.at : undefined} title={`Open ${row.title}`}><span className="mk"><StepMarkView mark={row.mark} /></span><span className="t"><b>{row.title}{row.agent && <span className="kind">{row.agent}</span>}</b><small>{row.meta}</small></span>{row.dismiss && <em className="x" data-subclose={row.at} title="Dismiss from pane"><Icon name="x" /></em>}{row.stoppable && <em className="kstop" data-kidstop title={`Stop ${row.title}`}>Stop</em>}</button>;
  if (row.result !== null) return <>{button}<div className="dmuted res">{row.result}</div></>;
  if (!row.prompt) return button;
  return <div className="dsubprompt" data-row={row.at} data-at={row.at}>{button}<button className="prompt-toggle" data-prompt={row.at} aria-expanded={row.prompt.open}><span style={{ display: "contents" }}><Icon name="chev" /></span>Prompt</button>{row.prompt.open && <pre>{row.prompt.text}</pre>}</div>;
}
