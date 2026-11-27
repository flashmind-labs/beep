// The thought a coding agent streams, shown open and following its newest
// line. The log redraws on every read, so whether each follows is kept here.


/// The live thoughts that follow their newest line, until the reader
/// scrolls them.
const follows = new Map<string, boolean>();

/// The thoughts already listening, which a redraw that kept them does not wire again.
const wired = new WeakSet<HTMLElement>();

/// Registers a thought before its view is rendered.
export function trackLive(key: string) { if (!follows.has(key)) follows.set(key, true); }

/// Wires the live thoughts just drawn under `root`: each follows its
/// newest line until the reader scrolls it. Forgets thoughts no longer drawn.
export function attachLive(root: HTMLElement) {
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
      requestAnimationFrame(() => { follows.set(key, el.scrollHeight - el.scrollTop - el.clientHeight < 4); });
    };
    el.addEventListener("wheel", byHand, { passive: true });
    el.addEventListener("pointerdown", byHand);
  }

  for (const key of follows.keys()) if (!drawn.has(key)) follows.delete(key);
}
