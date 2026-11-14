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

/// Home's rows: the cards of each project together, a project that waits on
/// the user first by default, or by recency when supplied. Cards of no project stay last.
export function byProject<T extends { lane: Lane; project: string | null }>(cards: readonly T[], recency: ReadonlyMap<string, number> | null = null): { project: string | null; cards: T[] }[] {
  const rows = new Map<string | null, T[]>();

  for (const card of cards) {
    const row = rows.get(card.project) ?? [];
    row.push(card);
    rows.set(card.project, row);
  }

  const rank = (row: { project: string | null; cards: T[] }) => row.project === null ? 2 : row.cards.some(card => card.lane === "needs") ? 0 : 1;
  return [...rows].map(([project, own]) => ({ project, cards: own })).sort((a, b) => {
    if (!recency) return rank(a) - rank(b);
    if (a.project === null) return b.project === null ? 0 : 1;
    if (b.project === null) return -1;
    return (recency.get(b.project) ?? 0) - (recency.get(a.project) ?? 0);
  });
}

/// A card's muted line under its project's heading, without the project's name.
export function withoutProject(sub: string, project: string | null) {
  if (project === null) return sub;
  if (sub === project) return "";
  return sub.startsWith(`${project} · `) ? sub.slice(project.length + 3) : sub;
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
