// What a coding agent session's right-click menu offers, apart from the DOM.

/// A turn is under way in these states.
const TURN = new Set(["asking", "working", "running", "starting"]);

/// The harnesses whose sessions are files a node can delete. The others keep
/// theirs in a database.
const DELETABLE = new Set(["claude", "codex", "pi"]);

export interface SessionOffers {
  stop: boolean;
  close: boolean;
  resume: boolean;
  delete: boolean;
}

/// The agents whose sessions started outside Beep can be taken over.
export const TAKEOVER = new Set(["claude", "codex", "pi", "flsh", "opencode"]);

/// The items for a session in `status`. A session started outside Beep can
/// be continued here, and once it ended its conversation can still be deleted.
/// A running one must be closed first. `reachable` is whether this Mac can ask
/// the machine to delete: not one only a server reaches.
export function sessionOffers(s: { status: string; external: boolean; agent: string }, reachable = true): SessionOffers {
  const ended = s.status === "ended";
  const del = ended && reachable && DELETABLE.has(s.agent);
  if (s.external) return { stop: false, close: false, resume: TAKEOVER.has(s.agent), delete: del };
  return { stop: TURN.has(s.status), close: !ended, resume: ended, delete: del };
}
