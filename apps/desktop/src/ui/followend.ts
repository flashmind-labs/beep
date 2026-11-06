// A log that follows its end stays there while what it shows grows by
// itself: history drawn after the first paint, pictures loading, markdown
// laid out. A reader who scrolled up (`stuck` false) is left where they are.

import type { ChatBody } from "./turn.ts";

const followed = new WeakSet<ChatBody>();

/// Pins `box` to its end whenever `content`, or each child of `box` when
/// none is given, changes size while the box follows its end.
export function followEnd(box: ChatBody, content?: HTMLElement) {
  if (followed.has(box)) return;
  followed.add(box);
  const watch = new ResizeObserver(() => {
    if (box.isConnected && box.stuck !== false) box.scrollTop = box.scrollHeight;
  });
  if (content) return watch.observe(content);
  for (const child of box.children) watch.observe(child);
  new MutationObserver(changes => {
    for (const c of changes) {
      for (const n of c.addedNodes) if (n instanceof Element) watch.observe(n);
      for (const n of c.removedNodes) if (n instanceof Element) watch.unobserve(n);
    }
  }).observe(box, { childList: true });
}
