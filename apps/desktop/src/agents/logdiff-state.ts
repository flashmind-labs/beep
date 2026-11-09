// Which rows of the log a redraw can keep. The log is drawn from strings;
// a row whose string is the one it was drawn from is the same row.

/// For each row of `next`, the index in `prev` of an identical row it can
/// reuse, or -1 when it must be drawn. A row of `prev` is reused once.
export function reuseRows(prev: readonly string[], next: readonly string[]) {
  const free = new Map<string, { indices: number[]; next: number }>();
  prev.forEach((html, i) => {
    const at = free.get(html);
    if (at) at.indices.push(i); else free.set(html, { indices: [i], next: 0 });
  });
  return next.map(html => {
    const at = free.get(html);
    return at && at.next < at.indices.length ? at.indices[at.next++] : -1;
  });
}
