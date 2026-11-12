// The board's lanes: which lane a session sits in and which sessions ended
// today, apart from the DOM so it can be tested.

export type Lane = "needs" | "working" | "review" | "done";

export const LANES: readonly { lane: Lane; title: string }[] = [
  { lane: "needs", title: "Needs you" },
  { lane: "working", title: "Working" },
  { lane: "review", title: "Ready to review" },
  { lane: "done", title: "Done today" },
];

/// The lane a session's status puts it in: a question waits on the user, a
/// turn in hand is working, a finished turn waits to be read, and an ended
/// session is done.
export function laneOf(status: string): Lane {
  switch (status) {
    case "asking": return "needs";
    case "idle": return "review";
    case "ended": return "done";
    default: return "working";
  }
}

/// `cards` by lane, in the order given, every lane present.
export function byLane<T extends { lane: Lane }>(cards: readonly T[]): Record<Lane, T[]> {
  const lanes: Record<Lane, T[]> = { needs: [], working: [], review: [], done: [] };
  for (const card of cards) lanes[card.lane].push(card);
  return lanes;
}

/// The sessions that ended since local midnight of `now`, the latest first.
export function endedToday<T extends { status: string; last_activity: number }>(sessions: readonly T[], now: number): T[] {
  const midnight = new Date(now).setHours(0, 0, 0, 0);
  return sessions.filter(s => s.status === "ended" && s.last_activity >= midnight).sort((a, b) => b.last_activity - a.last_activity);
}

/// How far each card kept across a draw moved, by key, in pixels: what a card
/// that changed lane travels from. Cards that stayed put and new ones are left out.
export function travelled(before: ReadonlyMap<string, { x: number; y: number }>, after: ReadonlyMap<string, { x: number; y: number }>) {
  const moves = new Map<string, { dx: number; dy: number }>();

  for (const [key, at] of after) {
    const was = before.get(key);
    if (!was) continue;
    const dx = was.x - at.x, dy = was.y - at.y;
    if (Math.abs(dx) >= 1 || Math.abs(dy) >= 1) moves.set(key, { dx, dy });
  }

  return moves;
}
