import { frameResize } from "./dom.ts";
import type { ChatBody } from "./turn.ts";

interface Follow { resize: ResizeObserver; changes: MutationObserver | null }
const followed = new WeakMap<ChatBody, Follow>();

// Display-contents hosts have no size of their own, so observe their boxes.
function boxes(parent: Element): HTMLElement[] {
  return [...parent.children].flatMap(child => child instanceof HTMLElement
    ? window.getComputedStyle(child).display === "contents" ? boxes(child) : [child]
    : []);
}

/// Pins a following log when its content grows; a reader above the end stays put.
export function followEnd(box: ChatBody, content?: HTMLElement) {
  if (followed.has(box)) return;
  const resize = frameResize(() => {
    if (box.isConnected && box.stuck !== false) box.scrollTop = box.scrollHeight;
  });
  if (content) {
    resize.observe(content);
    followed.set(box, { resize, changes: null });
    return;
  }
  let observed = new Set<HTMLElement>();
  const update = () => {
    const next = new Set(boxes(box));
    for (const element of observed) if (!next.has(element)) resize.unobserve(element);
    for (const element of next) if (!observed.has(element)) resize.observe(element);
    observed = next;
  };
  const changes = new MutationObserver(update);
  changes.observe(box, { childList: true, subtree: true });
  followed.set(box, { resize, changes });
  update();
}

export function unfollowEnd(box: ChatBody) {
  const current = followed.get(box);
  if (!current) return;
  current.changes?.disconnect(); current.resize.disconnect();
  followed.delete(box);
}
