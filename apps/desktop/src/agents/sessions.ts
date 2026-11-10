import { invoke, listen } from "../ui/tauri.ts";
import { subscribeTopics, topic } from "../ui/mesh-events.ts";
import { Changes } from "../ui/pagecache.ts";
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

/// Home's topic changes with every session's state and moves, and the
/// window hears of titles and sessions on other devices: until one of those,
/// the last read stands.
const changes = new Changes();
const changed = () => changes.changed();
subscribeTopics(topic("home"), changed);
for (const name of ["sessions-changed", "agent-title-changed", "reconnected"]) listen(name, changed);

/// The sessions every page shares: the last read while nothing changed
/// since, otherwise a read.
export function ensureSessions(): Promise<AgentSession[]> {
  return loading ?? (changes.dirty("") ? loadSessions() : Promise.resolve(sessions));
}

/// Reads the sessions again into `sessions` and returns them. A call made
/// while another read is out waits for that one, so pages that ask together
/// cost one request. A failed read rejects and leaves `sessions` as it was.
export function loadSessions() {
  // The copies are read with the sessions: whether a machine is off changes.
  void loadCopies();
  if (loading) return loading;
  changes.read("");
  loading = Promise.all([invoke<AgentSession[]>("agent_sessions"), refreshMoves()])
    // A session that moved is on two machines for a while: it shows once.
    .then(([list]) => {
      sessions = withoutCopies([...list.filter(s => !s.reviewer), ...nativeSessions.values()]);
      reviewers = list.filter(s => s.reviewer);
      for (const f of sessionsRead) f();
      return sessions;
    })
    .catch(e => { changes.changed(); throw e; })
    .finally(() => { loading = null; });
  return loading;
}
