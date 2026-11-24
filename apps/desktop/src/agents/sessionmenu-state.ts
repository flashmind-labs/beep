// What a coding agent session's right-click menu offers, apart from the DOM.

/// A turn is under way in these states.
export const AGENT_ACTIVE = new Set<string | undefined>(["asking", "working", "running", "starting"]);

/// The harnesses whose sessions are files a node can delete. The others keep
/// theirs in a database.
const DELETABLE = new Set(["claude", "codex", "pi"]);

interface SessionOffers {
  stop: boolean;
  close: boolean;
  resume: boolean;
  delete: boolean;
}

/// The agents whose sessions started outside Beep can be taken over.
export const TAKEOVER = new Set(["claude", "codex", "pi", "flsh", "opencode", "agy"]);

/// The items for a session in `status`. A session started outside Beep can
/// be continued here, and once it ended its conversation can still be deleted.
/// A running one must be closed first. `reachable` is whether this device can
/// ask the machine to delete.
export function sessionOffers(s: { status: string; external: boolean; agent: string; id?: string }, reachable = true): SessionOffers {
  if (s.id?.startsWith("subagent:")) return { stop: false, close: false, resume: false, delete: false };
  const ended = s.status === "ended";
  const del = ended && reachable && DELETABLE.has(s.agent);
  if (s.external) return { stop: false, close: false, resume: TAKEOVER.has(s.agent), delete: del };
  return { stop: AGENT_ACTIVE.has(s.status), close: !ended, resume: ended, delete: del };
}

/// What a session runs besides its main turn, as far as this window knows.
export interface SideWork {
  subagents: number;
  /// Background commands still running.
  tasks: number;
  /// Agents it started that still run.
  started: number;
}

/// Stop all asks first while anything besides the main turn runs.
export const stopAllAsks = (work: SideWork) => work.subagents > 0 || work.tasks > 0 || work.started > 0;
