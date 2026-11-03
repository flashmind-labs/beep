// A coding agent's thought as it streams, shown like karaoke lyrics: all
// of its text is there, dimmed, and a band reads it letter by letter at a
// steady pace, lighting the newest letters and fading them back behind
// it. These are the parts without the page.

/// How many of the newest characters the band covers, fading back to
/// the thought's color over its length.
export const BAND = 80;
/// The pace the band reads at, in characters a second...
export const RATE = 40;
/// ...and how far behind the text it may fall, in seconds: further back,
/// it reads faster.
const CATCH_UP_S = 2;
/// The most text left to read; beyond it the band jumps ahead, so a long
/// thought first seen mid-way is not read again from the top.
const MAX_LAG = 320;

/// A thought's Markdown as plain text for the live view: no bold, code or
/// heading marks, and at most one blank line in a row.
export function liveText(md: string) {
  return md
    .replace(/\*\*|__|`/g, "")
    .replace(/^[ \t]*#{1,6}[ \t]+/gm, "")
    .replace(/\n{3,}/g, "\n\n")
    .trimStart();
}

/// How much of `text` stays revealed when it replaces `before`: all that
/// was shown while it only grew, else up to where they part.
export function keptShown(before: string, text: string, shown: number) {
  if (text.startsWith(before)) return Math.min(shown, text.length);
  let same = 0;
  while (same < before.length && same < text.length && before[same] === text[same]) same++;
  return Math.min(shown, same);
}

/// How much is read `dt` seconds after `shown`, of `length`: at a steady
/// pace, faster when more than two seconds behind. `still` (reduced motion)
/// reads it all at once.
export function reveal(shown: number, length: number, dt: number, still: boolean) {
  if (still || shown >= length) return length;
  const from = Math.max(shown, length - MAX_LAG);
  const left = length - from;
  return Math.min(length, from + Math.max(RATE, left / CATCH_UP_S) * Math.max(0, dt));
}

/// The text in three parts: what was read, the newest `band` characters
/// read, under the band, and the rest, not read yet.
export function splitBand(text: string, shown: number, band = BAND) {
  const end = Math.max(0, Math.min(text.length, Math.floor(shown)));
  const cut = Math.max(0, end - band);
  return { old: text.slice(0, cut), lit: text.slice(cut, end), rest: text.slice(end) };
}
