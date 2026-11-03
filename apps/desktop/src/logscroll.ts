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

/// What a redraw does with the scroll: `bottom` follows new entries,
/// `keep` puts the row the reader saw (or clicked) back where it was.
export type LogScroll = "bottom" | "keep";

/// How the log scrolls after a redraw. `wasAtBottom` is whether the
/// reader was at the bottom before it; `toggled` is whether the redraw
/// comes from their click opening or folding a row, which never jumps.
export function logScroll(wasAtBottom: boolean, toggled: boolean): LogScroll {
  return wasAtBottom && !toggled ? "bottom" : "keep";
}

/// The scrollTop that puts a row back `before` pixels from the box's top
/// when it is now `after` pixels from it, at `scrollTop`.
export function keptScrollTop(scrollTop: number, before: number, after: number) {
  return Math.max(0, scrollTop + after - before);
}
