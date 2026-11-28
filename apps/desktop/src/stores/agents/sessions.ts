import { invoke, listen } from "../../ui/tauri.ts";
import { subscribeEach, topic } from "../../ui/mesh-events.ts";
import { pushedList } from "../../ui/snapshot-state.ts";
import { AMBIENT_MS, ambient, nextChange } from "../../ui/active.ts";
import type { AgentRow, AgentSession } from "./agents-state.ts";
import { replaceStartedSession, type AgentStarted } from "./starting-state.ts";
import type { MoveRecord } from "./move-state.ts";
import { retitled, sessionKey, type Retitled } from "./conversation-state.ts";
import { parentRow } from "./nesting-state.ts";
import { takeSessionPlaces } from "./place.ts";
import { moveStore, refreshMoves, takePushedSessions, withoutCopies } from "./move-store.ts";

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

  show();
}

export const parentSession = (row: AgentRow) => parentRow(sessions, row) ?? nativeParents.get(sessionKey(row));

let loading: Promise<AgentSession[]> | null = null;
/// The list the `home` push or a read last gave, before the moves and native rows join it.
let listed: AgentSession[] | null = null;
/// Counts the lists taken, so a read that was out when a push arrived is dropped.
let taken = 0;
/// Called after each change of the sessions, with the new list in `sessions`.
export const sessionsRead = new Set<() => void>();

/// Takes `list` as the sessions: a session that moved is on two machines for a while, it shows once.
function take(list: AgentSession[]) {
  takeSessionPlaces(list);
  listed = list;
  taken++;
  show();
}

function show() {
  if (!listed) return;
  sessions = withoutCopies([...listed.filter(s => !s.reviewer), ...nativeSessions.values()]);
  for (const f of sessionsRead) f();
}

// Home's topic pushes the list with every change of a session's state or moves.
subscribeEach(topic("home"), event => {
  const pushed = pushedList<AgentSession>(event.data, "agent_sessions");
  if (!pushed || "error" in pushed) return;

  take(pushed.list);

  const moves = pushedList<MoveRecord>(event.data, "session_moves");
  if (moves && !("error" in moves)) takePushedSessions(moves.list);
});

// A move that ends or starts changes which copy shows.
let moves = moveStore.getState().records;
moveStore.subscribe(state => { if (state.records !== moves) { moves = state.records; show(); } });

// A reconnect says a push may have been missed: the sessions are read again, once a second at most.
const reread = ambient(() => loadSessions().catch(e => console.warn("sessions:", e)), AMBIENT_MS);
listen("reconnected", () => reread());
listen<AgentStarted>("agent-started", ({ payload: { start, session } }) => {
  if (!listed) return;
  take(replaceStartedSession(listed, start, session));
});
// A title comes with its value, a cleared one with the agent's own: the kept rows take it.
listen<Retitled>("agent-title-changed", ({ payload }) => {
  if (!retitled(sessions, payload)) return;
  if (listed) listed = retitled(listed, payload) ?? listed;
  for (const [key, row] of nativeSessions) {
    const next = retitled([row], payload);
    if (next) nativeSessions.set(key, next[0]);
  }
  show();
});

/// Resolves at the next change of the sessions, or after `ms`: for a page that looks for a row
/// the machine has not listed yet.
export const nextSessions = (ms: number) => nextChange(sessionsRead, ms);

/// The sessions every page shares: the list held from the pushes, or one read before the first.
export function ensureSessions(): Promise<AgentSession[]> {
  return loading ?? (listed ? Promise.resolve(sessions) : loadSessions());
}

/// Reads the sessions into `sessions` and returns them, for the first draw and after events that
/// carry no list. The moves are read beside it and join the list when they arrive. A call made while another read is out waits for that one. A list pushed while
/// the read is out is newer, and the read's answer is dropped. A failed read rejects and leaves
/// `sessions` as it was.
export function loadSessions() {
  if (loading) return loading;
  const at = taken;
  void refreshMoves();
  loading = invoke<AgentSession[]>("agent_sessions")
    .then(list => {
      if (at === taken) take(list);
      return sessions;
    })
    .finally(() => { loading = null; });
  return loading;
}
