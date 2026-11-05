// Pointer drags on a grip, shared by every resizer.

/// What a drag does: `move` runs at most once a frame with the latest pointer
/// event, `end` once on release or cancel, after the last move is applied.
export type Drag = { move: (ev: PointerEvent) => void; end: () => void };

/// Wires `el` as a grip. `start` runs on a primary-button press and returns
/// the drag, or nothing to ignore the press.
export function dragHandle(el: HTMLElement, start: (e: PointerEvent) => Drag | void) {
  el.addEventListener("pointerdown", e => {
    if (e.button !== 0) return;
    e.preventDefault();
    const drag = start(e);
    if (!drag) return;
    el.setPointerCapture(e.pointerId);
    let last = e, frame = 0;
    const flush = () => { frame = 0; drag.move(last); };
    const move = (ev: PointerEvent) => {
      last = ev;
      if (!frame) frame = requestAnimationFrame(flush);
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      if (frame) { cancelAnimationFrame(frame); flush(); }
      drag.end();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  });
}
