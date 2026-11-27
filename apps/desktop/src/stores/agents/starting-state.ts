import type { AgentRow, AgentSession } from "./agents-state.ts";

/// A session this device keeps until its machine names it: the desktop answers for it, not the machine.
export const isStarting = (id: string) => id.startsWith("start:");

/// The session each start kept on this device became, by the start's id: a page or list that still
/// holds the start uses the named session instead.
export const became = new Map<string, AgentRow>();

/// What `agent-started` says once the machine named the session of start `start`.
export interface AgentStarted {
  start: string;
  session: AgentSession;
  first_message: string;
}

/// Replaces `start` with `session` in `list`, without leaving both ids or two
/// copies of `(machine_id, id)`.
export function replaceStartedSession(list: readonly AgentSession[], start: string, session: AgentSession): AgentSession[] {
  const keep = !session.reviewer;
  const out: AgentSession[] = [];
  let placed = false;

  for (const row of list) {
    if (row.id === start) {
      if (keep && !placed) { out.push(session); placed = true; }
      continue;
    }
    if (row.machine_id === session.machine_id && row.id === session.id) {
      if (keep && !placed) { out.push(session); placed = true; }
      continue;
    }
    out.push(row);
  }

  if (keep && !placed) out.unshift(session);
  return out;
}
