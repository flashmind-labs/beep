// The thought a coding agent streams, shown open and following its newest
// line. The log redraws on every read, so whether each follows is kept here.

import { liveText } from "./karaoke.ts";
import { esc } from "../ui/format.ts";

/// The live thoughts that follow their newest line, until the reader
/// scrolls them.
const follows = new Map<string, boolean>();

/// The thoughts already listening, which a redraw that kept them does not wire again.
const wired = new WeakSet<HTMLElement>();

/// The live thought `key`'s text, all it has so far in `md`.
export function liveHtml(key: string, md: string) {
  if (!follows.has(key)) follows.set(key, true);
  return `<div class="kk" data-kk="${esc(key)}">${esc(liveText(md))}</div>`;
}

/// A thought that just ended, as its live view last was, for the moment
/// it folds.
export function endedHtml(key: string, md: string) {
  follows.delete(key);
  return `<div class="kk ended">${esc(liveText(md))}</div>`;
}

/// Wires the live thoughts just drawn under `root`: each follows its
/// newest line until the reader scrolls it, which `touched` hears of; a
/// thought that ended folds now. Forgets thoughts no longer drawn.
export function attachLive(root: HTMLElement, touched: (key: string) => void) {
  const drawn = new Set<string>();
  for (const el of root.querySelectorAll<HTMLElement>(".kk[data-kk]")) {
    const key = el.dataset.kk ?? "";
    if (!follows.has(key)) continue;
    drawn.add(key);
    if (follows.get(key) ?? true) el.scrollTop = el.scrollHeight;
    if (wired.has(el)) continue;
    wired.add(el);
    // A wheel or a drag of the scroll bar is the reader's; scrolling back
    // to the bottom follows again.
    const byHand = () => {
      touched(key);
      requestAnimationFrame(() => { follows.set(key, el.scrollHeight - el.scrollTop - el.clientHeight < 4); });
    };
    el.addEventListener("wheel", byHand, { passive: true });
    el.addEventListener("pointerdown", byHand);
  }
  for (const key of follows.keys()) if (!drawn.has(key)) follows.delete(key);
  for (const el of root.querySelectorAll<HTMLElement>(".kk.ended")) el.scrollTop = el.scrollHeight;
  const folding = root.querySelectorAll<HTMLElement>(".dthink[data-folding]");
  // Their open state is laid out once first, so taking it away animates.
  if (folding.length) void root.offsetHeight;
  for (const el of folding) {
    el.classList.remove("open");
    el.querySelector("button")?.setAttribute("aria-expanded", "false");
  }
}
