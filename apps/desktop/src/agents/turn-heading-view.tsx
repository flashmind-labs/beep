import { memo } from "react";
import { clock, heading, type TurnState } from "./agentturns.ts";

export interface TurnHeading {
  kind: "heading";
  id: string;
  html: string;
  state: TurnState;
  start: number | null;
  ms: number | null;
  stats: string;
  turn: string | number | null;
  opened: boolean;
}

export const AgentTurnHeading = memo(function AgentTurnHeading({ value }: { value: TurnHeading }) {
  const done = value.state === "done";
  const label = done ? <><span>{heading(value.state, value.ms)}</span>{value.stats && <span className="stats">· {value.stats}</span>}</> : <span>{value.stats || "Steps"}</span>;
  return <div className="dcoderturn">{value.turn === null ? <span className="hd">{label}</span> : <button className={`hd${value.opened ? " open" : ""}`} data-turn={value.turn} aria-expanded={value.opened} title={value.opened ? "Fold the steps" : "Show the steps"}>{label}<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M9 6l6 6-6 6" /></svg></button>}{value.start !== null && <span className="when">{clock(value.start)}</span>}</div>;
});
