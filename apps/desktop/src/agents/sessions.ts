import { invoke } from "../ui/tauri.ts";
import type { AgentSession } from "./agents.ts";

/// Every session on the user's machines, as the last `loadSessions` read it.
export let sessions: AgentSession[] = [];

let loading: Promise<AgentSession[]> | null = null;

/// Reads the sessions again into `sessions` and returns them. A call made
/// while another read is out waits for that one, so pages that ask together
/// cost one request. A failed read rejects and leaves `sessions` as it was.
export function loadSessions() {
  loading ??= invoke<AgentSession[]>("agent_sessions")
    .then(list => { sessions = list; return list; })
    .finally(() => { loading = null; });
  return loading;
}
