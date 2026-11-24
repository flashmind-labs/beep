import { invoke, listen } from "../ui/tauri.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { Changes } from "../ui/pagecache.ts";
import type { AgentRow, AgentSession } from "./agents.ts";
import { retitled, sessionKey, type Retitled } from "./conversation-state.ts";
import { parentRow } from "./nesting-state.ts";
import { refreshMoves, withoutCopies } from "./move-store.ts";

/// Every session on the user's machines, as the last `loadSessions` read it.
export let sessions: AgentSession[] = [];
const nativeSessions = new Map<string, AgentSession>();
const nativeParents = new Map<string, AgentRow>();

export function keepNativeSession(row: AgentSession, parent?: AgentRow) {
  const key = sessionKey(row);
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

export const parentSession = (row: AgentRow) => parentRow(sessions, row) ?? nativeParents.get(sessionKey(row));

let loading: Promise<AgentSession[]> | null = null;
/// Called after each read of the sessions, with the new list in `sessions`.
export const sessionsRead = new Set<() => void>();

/// Home's topic changes with every session's state and moves, and the
/// window hears of titles and sessions on other devices: until one of those,
/// the last read stands.
const changes = new Changes();
/// Called when the sessions may have changed since the last read.
export const sessionsChanged = new Set<() => void>();
const changed = () => { changes.changed(); for (const f of sessionsChanged) f(); };
subscribeEach(topic("home"), changed);
for (const name of ["sessions-changed", "reconnected"]) listen(name, changed);
// A title saved comes with its value: the kept rows take it, and only a cleared title reads.
listen<Retitled>("agent-title-changed", ({ payload }) => {
  const next = retitled(sessions, payload);
  if (!next) { if (payload.title === null) changed(); return; }
  sessions = next;
  for (const f of sessionsRead) f();
});

/// The sessions every page shares: the last read while nothing changed
/// since, otherwise a read.
export function ensureSessions(): Promise<AgentSession[]> {
  return loading ?? (changes.dirty("") ? loadSessions() : Promise.resolve(sessions));
}

/// Reads the sessions again into `sessions` and returns them. A call made
/// while another read is out waits for that one, so pages that ask together
/// cost one request. A failed read rejects and leaves `sessions` as it was.
export function loadSessions() {
  if (loading) return loading;
  changes.read("");
  loading = Promise.all([invoke<AgentSession[]>("agent_sessions"), refreshMoves()])
    // A session that moved is on two machines for a while: it shows once.
    .then(([list]) => {
      sessions = withoutCopies([...list.filter(s => !s.reviewer), ...nativeSessions.values()]);
      for (const f of sessionsRead) f();
      return sessions;
    })
    .catch(e => { changes.changed(); throw e; })
    .finally(() => { loading = null; });
  return loading;
}
