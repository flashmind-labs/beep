// What a coding agent session's right-click menu offers, apart from the DOM.

/// A turn is under way in these states.
const TURN = new Set(["asking", "working", "running", "starting"]);

export interface SessionOffers {
  stop: boolean;
  close: boolean;
  resume: boolean;
}

/// The steering items for a session in `status`. A session started outside
/// Beep can only be followed, so it gets none.
export function sessionOffers(s: { status: string; external: boolean }): SessionOffers {
  if (s.external) return { stop: false, close: false, resume: false };
  const ended = s.status === "ended";
  return { stop: TURN.has(s.status), close: !ended, resume: ended };
}
