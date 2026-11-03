// A coding agent's thought as it streams, shown like karaoke lyrics: the
// text arrives in bursts (one per read of the session), and is revealed at
// the pace it came, with a bright band on the newest words that fades them
// back to muted behind it. These are the parts without the page.

/// How many of the newest characters the bright band covers.
export const BAND = 56;
/// The slowest the text is revealed, in characters a second...
const MIN_RATE = 30;
/// ...and the time constant a burst is revealed with, so most of it shows
/// well before the next read of the session, 1.5s later.
const CATCH_UP_S = 0.6;
/// The most text left to reveal; beyond it the reveal jumps ahead.
const MAX_LAG = 600;
/// How much of a thought first seen mid-way is revealed again.
const FIRST_TAIL = 120;

/// A thought's Markdown as plain text for the live view: no bold, code or
/// heading marks, and at most one blank line in a row.
export function liveText(md: string) {
  return md
    .replace(/\*\*|__|`/g, "")
    .replace(/^[ \t]*#{1,6}[ \t]+/gm, "")
    .replace(/\n{3,}/g, "\n\n")
    .trimStart();
}

/// Where the reveal starts for a thought first seen with `length`
/// characters: near its end, so opening a long one does not replay it.
export function firstShown(length: number) {
  return Math.max(0, length - FIRST_TAIL);
}

/// How much of `text` stays revealed when it replaces `before`: all that
/// was shown while it only grew, else up to where they part.
export function keptShown(before: string, text: string, shown: number) {
  if (text.startsWith(before)) return Math.min(shown, text.length);
  let same = 0;
  while (same < before.length && same < text.length && before[same] === text[same]) same++;
  return Math.min(shown, same);
}

/// How much is revealed `dt` seconds after `shown`, of `length`: faster
/// the more is left. `still` (reduced motion) reveals it all at once.
export function reveal(shown: number, length: number, dt: number, still: boolean) {
  if (still || shown >= length) return length;
  const from = Math.max(shown, length - MAX_LAG);
  const left = length - from;
  return Math.min(length, from + Math.max(MIN_RATE, left / CATCH_UP_S) * Math.max(0, dt));
}

/// The revealed text in two parts: what went muted, and the newest
/// `band` characters under the band.
export function splitBand(text: string, shown: number, band = BAND) {
  const end = Math.max(0, Math.min(text.length, Math.floor(shown)));
  const cut = Math.max(0, end - band);
  return { old: text.slice(0, cut), lit: text.slice(cut, end) };
}
