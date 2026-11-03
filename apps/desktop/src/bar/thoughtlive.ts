// The thought a coding agent streams, shown open as plain text that
// follows its newest line. The log is drawn again on every read of the
// session, so whether each follows is kept here, by session and entry.

import { liveText } from "../karaoke.ts";
import { esc } from "./core.ts";

/// The live thoughts that follow their newest line, until the reader
/// scrolls them.
const follows = new Map<string, boolean>();

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

/// Keeps a box at its newest line while it follows, and fades its top
/// only once older lines went out of it.
function fit(el: HTMLElement, follow: boolean) {
  el.classList.toggle("ov", el.scrollHeight > el.clientHeight + 1);
  if (follow) el.scrollTop = el.scrollHeight;
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
    fit(el, follows.get(key) ?? true);
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
  for (const el of root.querySelectorAll<HTMLElement>(".kk.ended")) fit(el, true);
  for (const el of root.querySelectorAll<HTMLElement>(".dthink[data-folding]")) {
    // Its open state is laid out first, so taking it away animates.
    void el.offsetHeight;
    el.classList.remove("open");
    el.querySelector("button")?.setAttribute("aria-expanded", "false");
  }
}
