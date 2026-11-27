// Which sessions the bar's default list shows, and in what order.
import type { AgentSession } from "../agents/agents-state.ts";
import { sessionRuns } from "../agents/running-state.ts";
import { sessionKey } from "../agents/conversation-state.ts";

/// Why a session is on the list, in the order the groups show.
export type WaitingKind = "asking" | "done" | "running" | "idle";

export interface WaitingSession {
  session: AgentSession;
  kind: WaitingKind;
}

/// The most rows the list shows.
const WAITING_MAX = 10;

const ORDER: Record<WaitingKind, number> = { asking: 0, done: 1, running: 2, idle: 3 };

function waitingKind(s: AgentSession, unread: ReadonlySet<string>): WaitingKind | null {
  if (s.status === "asking" || s.question) return "asking";
  if (s.status === "ended" || s.status === "closing") return null;
  if (sessionRuns(s)) return "running";
  return unread.has(sessionKey(s)) ? "done" : "idle";
}

/// Sessions asking first, then those with an unread finished turn, then
/// running ones, then other idle ones; newest first in each. Children
/// open from the parent agent panel.
/// `query` matches the title or what `project` names.
export function waitingSessions(list: readonly AgentSession[], unread: ReadonlySet<string>, query: string, project: (s: AgentSession) => string, max = WAITING_MAX): WaitingSession[] {
  const q = query.trim().toLowerCase();
  const rows: WaitingSession[] = [];

  for (const session of list) {
    const kind = waitingKind(session, unread);
    if (!kind || session.parent) continue;
    if (q && !`${session.title ?? ""} ${project(session)}`.toLowerCase().includes(q)) continue;
    rows.push({ session, kind });
  }

  rows.sort((a, b) => ORDER[a.kind] - ORDER[b.kind] || b.session.last_activity - a.session.last_activity);
  return rows.slice(0, max);
}
