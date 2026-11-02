// Updates the user dismissed stay off Home even when a list asked for
// before the dismiss reached the server comes back with them.

/// Drops the updates in `dismissed`, a Set of ids, from `listed`. An id a
/// list no longer has is gone on the server too, so it leaves the set.
export function withoutDismissed<T extends { id: number }>(listed: T[], dismissed: Set<number>): T[] {
  const ids = new Set(listed.map(u => u.id));
  for (const id of dismissed) if (!ids.has(id)) dismissed.delete(id);
  return listed.filter(u => !dismissed.has(u.id));
}
