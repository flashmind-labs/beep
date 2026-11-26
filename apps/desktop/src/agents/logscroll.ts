// Where a session's log scrolls when it is drawn again: a reader at the
// bottom follows what arrives; a click that opens or folds a row keeps
// that row where it was on screen, wherever the reader is.

/// How near the bottom, in pixels, still counts as at the bottom.
const BOTTOM_SLACK = 40;

/// How a scrolled box stands.
interface ScrollBox {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

/// Whether `b` is scrolled to its end, or near enough.
export function atBottom(b: ScrollBox) {
  return b.scrollHeight - b.scrollTop - b.clientHeight < BOTTOM_SLACK;
}

/// How far `b` is from its end, never below 0 (a bounce past the end is not a gap).
export const gapOf = (b: ScrollBox) => Math.max(0, b.scrollHeight - b.scrollTop - b.clientHeight);

/// Notes where `b` stands, as the reader or the code left it: the place a
/// hidden log comes back to, and the gap a later scroll is judged against.
/// A box out of the page reads 0 and says nothing.
export function markPlace(b: ScrollBox & { top?: number; gap?: number }) {
  if (!b.clientHeight) return;
  b.top = b.scrollTop;
  b.gap = gapOf(b);
}

/// Whether a log that `stuck` to its end still does after a scroll that
/// found it `gapBefore` pixels from the end: any move away from the end lets
/// go, even inside the slack, and reaching the bottom takes it back. The gap
/// is compared, not `scrollTop`, so a pin to the end and the reader's move
/// up that arrive as one scroll event still count as up.
export function followsAfterScroll(stuck: boolean, gapBefore: number, b: ScrollBox, layoutChanged = false) {
  if (stuck && layoutChanged) return true;
  if (gapOf(b) > gapBefore) return false;
  return stuck || atBottom(b);
}

/// How long, in ms, after the reader's last wheel event the log is theirs:
/// macOS sends wheel events through a flick's glide, and a scroll written
/// then stops the glide and holds back the next scroll.
const HANDS_OFF_MS = 150;

/// How long a pin to the end waits at `now` for a wheel last turned at `wheel`.
export function pinWait(now: number, wheel?: number) {
  return wheel == null ? 0 : Math.max(0, wheel + HANDS_OFF_MS - now);
}

/// What a redraw does with the scroll: `bottom` follows new entries,
/// `keep` puts the row the reader saw (or clicked) back where it was.
type LogScroll = "bottom" | "keep";

/// Why the log is drawn again: `news` from a read of the newest entries,
/// `toggle` for rows the reader opened or folded, `older` for entries read
/// further back and put above.
type Redraw = "news" | "toggle" | "older";

/// How the log scrolls after a redraw: only a toggle holds a log that
/// follows its end where it is.
export function logScroll(wasAtBottom: boolean, why: Redraw): LogScroll {
  return wasAtBottom && why !== "toggle" ? "bottom" : "keep";
}

/// Where a log put back in the page scrolls: to its end when it followed
/// it, else to `top`, where the reader left it.
export function returnScroll(b: ScrollBox & { stuck?: boolean }, top: number) {
  return b.stuck !== false ? b.scrollHeight : top;
}

/// The scrollTop that puts a row back `before` pixels from the box's top
/// when it is now `after` pixels from it, at `scrollTop`.
export function keptScrollTop(scrollTop: number, before: number, after: number) {
  return Math.max(0, scrollTop + after - before);
}
