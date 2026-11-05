// Copies of other machines' sessions that this Mac's node brings in on its
// own, apart from the DOM so they can be tested: which ones the list offers
// to continue, and what its rows say.

/// A copy as the node lists it (CopyRow).
export interface CopyRow {
  agent: string;
  id: string;
  /// The machine that ran it, its name, and whether the mesh has it online now.
  from: string;
  from_name: string;
  online: boolean;
  project: string;
  path: string;
  /// Unix seconds: its last activity, and when the copy was last brought up to date.
  last_activity: number;
  fetched_at: number;
  size: number;
  /// When this version was set aside, for one that lost to another machine's.
  lost: number | null;
}

/// How long ago, in the unit that fits: `3 min`, `2 h`, `4 days`.
export function ago(seconds: number) {
  const s = Math.max(0, Math.round(seconds));
  if (s < 90) return "under 2 min";
  if (s < 3600 * 1.5) return `${Math.round(s / 60)} min`;
  if (s < 86400 * 1.5) return `${Math.round(s / 3600)} h`;
  return `${Math.round(s / 86400)} days`;
}

/// The copies the list offers: a session no machine lists and whose machine
/// is off by the mesh's own word (continue it here from the copy), and the
/// versions that lost to another machine's (kept until thrown away). Newest
/// first.
export function copyLists(copies: readonly CopyRow[], liveIds: ReadonlySet<string>) {
  const newest = (a: CopyRow, b: CopyRow) => b.last_activity - a.last_activity;
  return {
    offline: copies.filter(c => c.lost === null && !c.online && !liveIds.has(c.id)).sort(newest),
    lost: copies.filter(c => c.lost !== null).sort(newest),
  };
}

/// What an offline machine's row says under the title.
export const offlineLine = (c: CopyRow, now: number) => `${c.from_name} is offline. Copy from ${ago(now - c.fetched_at)} ago`;

/// What a version that lost says: another machine went on after it.
export const lostLine = () => "Another machine continued this session later. This version is kept";

/// What the Discard button says: the first click asks, in place, and the
/// second throws the version away for good.
export const discardLabel = (armed: boolean) => (armed ? "Throw away for good" : "Discard");

/// The copies as keyed for a button: a version that lost has its own key.
export const copyKey = (c: CopyRow) => `${c.agent}:${c.id}:${c.lost ?? ""}`;
