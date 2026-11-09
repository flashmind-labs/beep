import { invoke } from "../ui/tauri.ts";
import type { AgentRow, AgentSession } from "./agents.ts";
import { parentRow } from "./nesting-state.ts";
import { loadCopies } from "./copies.ts";
import { refreshMoves, withoutCopies } from "./move-store.ts";

/// Every session on the user's machines, as the last `loadSessions` read it.
export let sessions: AgentSession[] = [];
const nativeSessions = new Map<string, AgentSession>();
const nativeParents = new Map<string, AgentRow>();

export function keepNativeSession(row: AgentSession, parent?: AgentRow) {
  const key = `${row.machine_id}\n${row.id}`;
  if (parent) nativeParents.set(key, parent);
  nativeSessions.set(key, row);

  if (nativeSessions.size > 500) {
    const oldest = nativeSessions.keys().next().value!;
    nativeSessions.delete(oldest);
    nativeParents.delete(oldest);
  }

  const at = sessions.findIndex(s => s.machine_id === row.machine_id && s.id === row.id);
  if (at < 0) sessions.push(row); else sessions[at] = row;
}

export const parentSession = (row: AgentRow) => parentRow(sessions, row) ?? nativeParents.get(`${row.machine_id}\n${row.id}`);
/// The reviewers running as a child, kept apart: they list only under their parent.
export let reviewers: AgentSession[] = [];

let loading: Promise<AgentSession[]> | null = null;
/// Called after each read of the sessions, with the new list in `sessions`.
export const sessionsRead = new Set<() => void>();

/// Reads the sessions again into `sessions` and returns them. A call made
/// while another read is out waits for that one, so pages that ask together
/// cost one request. A failed read rejects and leaves `sessions` as it was.
export function loadSessions() {
  // The copies are read with the sessions: whether a machine is off changes.
  void loadCopies();
  loading ??= Promise.all([invoke<AgentSession[]>("agent_sessions"), refreshMoves()])
    // A session that moved is on two machines for a while: it shows once.
    .then(([list]) => {
      sessions = withoutCopies([...list.filter(s => !s.reviewer), ...nativeSessions.values()]);
      reviewers = list.filter(s => s.reviewer);
      for (const f of sessionsRead) f();
      return sessions;
    })
    .finally(() => { loading = null; });
  return loading;
}
