// Continuing a chat under another harness: who can, and how sessions link.
import type { AgentRow } from "./agents-state.ts";
import { isNativeSession } from "./native-session-state.ts";

/// What a chat continues with: the harness, model and thinking picked for it.
export interface ContinuePick {
  agent: string;
  model: string;
  effort: string | null;
  fast: boolean | null;
}

/// Whether the chat `s` can go on under another harness: not while it works or
/// asks, and not a session Beep only follows, a subagent or the Home assistant.
export const canContinueElsewhere = (s: AgentRow) => !s.external && !s.assistant && !s.reviewer && !isNativeSession(s.id)
  && !s.id.startsWith("subagent:") && s.status !== "working" && s.status !== "asking" && s.status !== "running" && s.status !== "starting";

type Linked = Pick<AgentRow, "machine_id" | "id" | "agent" | "title" | "continues">;

/// The session `s` carries on, when `rows` has it.
export const continuedFrom = <R extends Linked>(s: Linked, rows: readonly R[]): R | null =>
  s.continues ? rows.find(r => r.machine_id === s.machine_id && r.id === s.continues) ?? null : null;

/// The session that carries `s` on, when `rows` has one.
export const continuedIn = <R extends Linked>(s: Linked, rows: readonly R[]): R | null =>
  rows.find(r => r.machine_id === s.machine_id && r.continues === s.id) ?? null;

/// The line the log shows for a session it links to: where the chat went on
/// from or to, as `Continued from <title> · <harness>`.
export function continuedLine(way: "from" | "in", other: { title?: string | null; agent: string } | null, harness: (agent: string) => string) {
  if (way === "in") return `Continued in ${other ? harness(other.agent) : "another harness"}`;
  return other ? `Continued from ${other.title || "earlier chat"} · ${harness(other.agent)}` : "Continued from an earlier chat";
}
