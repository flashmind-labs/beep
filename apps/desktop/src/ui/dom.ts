// DOM helpers the windows share: layout writes after a resize.
/// The terminal views still take `Ago` from here.
export type { Ago } from "./format.ts";

// Layout writes run in the next frame, outside ResizeObserver delivery.
// Several changed children in one delivery need only one layout pass.
export function frameResize(update: () => void): ResizeObserver {
  let frame: number | null = null;
  const watch = new ResizeObserver(() => {
    if (frame !== null) return;
    frame = requestAnimationFrame(() => {
      frame = null;
      update();
    });
  });
  return {
    observe: (target, options) => watch.observe(target, options),
    unobserve: target => watch.unobserve(target),
    disconnect: () => {
      watch.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
    },
  };
}
