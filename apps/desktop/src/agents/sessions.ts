import { invoke } from "../ui/tauri.ts";
import type { AgentSession } from "./agents.ts";
import { refreshMoves, withoutCopies } from "./move-store.ts";

/// Every session on the user's machines, as the last `loadSessions` read it.
export let sessions: AgentSession[] = [];

let loading: Promise<AgentSession[]> | null = null;

/// Reads the sessions again into `sessions` and returns them. A call made
/// while another read is out waits for that one, so pages that ask together
/// cost one request. A failed read rejects and leaves `sessions` as it was.
export function loadSessions() {
  loading ??= Promise.all([invoke<AgentSession[]>("agent_sessions"), refreshMoves()])
    // A session that moved is on two machines for a while: it shows once.
    .then(([list]) => { sessions = withoutCopies(list); return sessions; })
    .finally(() => { loading = null; });
  return loading;
}
