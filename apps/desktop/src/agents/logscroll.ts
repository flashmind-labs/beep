// Where a session's log scrolls when it is drawn again: a reader at the
// bottom follows what arrives; a click that opens or folds a row keeps
// that row where it was on screen, wherever the reader is.

/// How near the bottom, in pixels, still counts as at the bottom.
export const BOTTOM_SLACK = 40;

/// How a scrolled box stands.
export interface ScrollBox {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

/// Whether `b` is scrolled to its end, or near enough.
export function atBottom(b: ScrollBox) {
  return b.scrollHeight - b.scrollTop - b.clientHeight < BOTTOM_SLACK;
}

/// Whether a log that `stuck` to its end still does after the reader moved
/// it from `was`: any move up lets go, even inside the slack, and reaching
/// the bottom takes it back. A move up from past the end (a bounce
/// settling) is not the reader's.
export function followsAfterScroll(stuck: boolean, was: number, b: ScrollBox) {
  if (b.scrollTop < Math.min(was, b.scrollHeight - b.clientHeight)) return false;
  return stuck || atBottom(b);
}

/// How long, in ms, after the reader's last wheel event the log is theirs:
/// macOS sends wheel events through a flick's glide, and a scroll written
/// then stops the glide and holds back the next scroll.
export const HANDS_OFF_MS = 150;

/// How long a pin to the end waits at `now` for a wheel last turned at `wheel`.
export function pinWait(now: number, wheel?: number) {
  return wheel == null ? 0 : Math.max(0, wheel + HANDS_OFF_MS - now);
}

/// What a redraw does with the scroll: `bottom` follows new entries,
/// `keep` puts the row the reader saw (or clicked) back where it was.
export type LogScroll = "bottom" | "keep";

/// How the log scrolls after a redraw.
export function logScroll(wasAtBottom: boolean, toggled: boolean): LogScroll {
  return wasAtBottom && !toggled ? "bottom" : "keep";
}

/// The scrollTop that puts a row back `before` pixels from the box's top
/// when it is now `after` pixels from it, at `scrollTop`.
export function keptScrollTop(scrollTop: number, before: number, after: number) {
  return Math.max(0, scrollTop + after - before);
}
