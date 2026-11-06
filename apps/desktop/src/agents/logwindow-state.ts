// Which turns of a long log are in the page: those near the view. The
// others are spacers as tall as the turns were when last measured, or as
// a guess before they were.

/// How tall a turn never measured is taken to be: the mean of those
/// measured and shown, or `fallback` before any was.
export function guess(measured: Iterable<number>, fallback: number) {
  let sum = 0, n = 0;
  for (const h of measured) if (h > 0) { sum += h; n++; }
  return n ? sum / n : fallback;
}

/// The turns `[from, to)` that reach into `[top, bottom]`, pixels from
/// the log's top, where turn i takes `sizes[i]`: its height and the gap
/// after it. Past the end, the last turn.
export function windowOf(sizes: readonly number[], top: number, bottom: number) {
  let y = 0, from = -1, to = sizes.length;
  for (let i = 0; i < sizes.length; i++) {
    if (from < 0 && y + sizes[i] > top) from = i;
    if (y >= bottom) { to = i; break; }
    y += sizes[i];
  }
  if (from < 0) from = Math.max(0, sizes.length - 1);
  return { from, to: Math.max(from, to) };
}

/// How tall the spacer for turns `[from, to)` is: their sizes less the gap
/// the spacer takes itself. Null when it has nothing to stand for.
export function spacer(sizes: readonly number[], from: number, to: number, gap: number) {
  let sum = 0;
  for (let i = from; i < to; i++) sum += sizes[i];
  return sum > 0 ? Math.max(0, sum - gap) : null;
}

/// A drawn turn whose height changed: where its top is, from the view's
/// top, and how tall it was and is.
export interface Resized {
  top: number;
  was: number;
  now: number;
}

/// How far the view scrolls so what the reader sees stays put: turns that
/// were wholly above the view push it by what they grew; the others grow
/// below the reader's eye.
export function shiftAbove(resized: readonly Resized[]) {
  return resized.reduce((shift, r) => r.top + r.was <= 0 ? shift + r.now - r.was : shift, 0);
}
