// The thought a coding agent streams, shown open with a karaoke band
// reading it letter by letter. The log is drawn again on every read of the
// session, so how far the band read is kept here, by session and entry,
// and put back on each draw; between reads, one animation frame loop
// moves it on.

import { BAND, keptShown, liveText, reveal, splitBand } from "../karaoke.ts";
import { esc } from "./core.ts";

interface Shown {
  text: string;
  /// Characters read, fractional between frames.
  shown: number;
  /// The thought ended: once read through, it folds.
  ended: boolean;
  /// It follows the newest text, until the reader scrolls it.
  follow: boolean;
}

const shown = new Map<string, Shown>();
let frame = 0, last = 0;
/// Draws the session again, for an ended thought read through.
let readThrough = () => {};

function still() {
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/// The live thought `key`'s text: what was read, the band and the rest.
/// `md` is all it has so far, all of it once it `ended`.
export function karaokeHtml(key: string, md: string, ended: boolean) {
  const text = liveText(md);
  let s = shown.get(key);
  if (!s) shown.set(key, s = { text, shown: still() ? text.length : 0, ended, follow: true });
  else {
    s.shown = keptShown(s.text, text, s.shown);
    s.text = text;
    s.ended = ended;
  }
  const { old, lit, rest } = splitBand(s.text, s.shown, still() ? 0 : BAND);
  return `<div class="kk" data-kk="${esc(key)}"><span class="o">${esc(old)}</span><span class="n">${esc(lit)}</span><span class="u">${esc(rest)}</span></div>`;
}

/// Whether thought `key` is still to be read: not read through yet.
export function reading(key: string) {
  const s = shown.get(key);
  return !still() && (!s || s.shown < s.text.length);
}

/// A thought that just ended, as its live view last was, all of it shown,
/// for the moment it folds.
export function endedHtml(key: string, md: string) {
  shown.delete(key);
  return `<div class="kk ended"><span class="o">${esc(liveText(md))}</span></div>`;
}

/// Keeps a box at its newest line while it follows, and fades its top
/// only once older lines went out of it.
function fit(el: HTMLElement, follow: boolean) {
  el.classList.toggle("ov", el.scrollHeight > el.clientHeight + 1);
  if (follow) el.scrollTop = el.scrollHeight;
}

/// Wires the live thoughts just drawn under `root`: each follows its
/// newest line until the reader scrolls it, which `touched` hears of; a
/// thought that ended folds now, and `done` draws again once an ended
/// one is read through. Forgets thoughts no longer drawn.
export function attachKaraoke(root: HTMLElement, touched: (key: string) => void, done: () => void) {
  readThrough = done;
  const drawn = new Set<string>();
  for (const el of root.querySelectorAll<HTMLElement>(".kk[data-kk]")) {
    const key = el.dataset.kk ?? "", s = shown.get(key);
    if (!s) continue;
    drawn.add(key);
    fit(el, s.follow);
    // A wheel or a drag of the scroll bar is the reader's; scrolling back
    // to the bottom follows again.
    const byHand = () => {
      touched(key);
      requestAnimationFrame(() => { s.follow = el.scrollHeight - el.scrollTop - el.clientHeight < 4; });
    };
    el.addEventListener("wheel", byHand, { passive: true });
    el.addEventListener("pointerdown", byHand);
  }
  for (const key of shown.keys()) if (!drawn.has(key)) shown.delete(key);
  for (const el of root.querySelectorAll<HTMLElement>(".kk.ended")) fit(el, true);
  for (const el of root.querySelectorAll<HTMLElement>(".dthink[data-folding]")) {
    // Its open state is laid out first, so taking it away animates.
    void el.offsetHeight;
    el.classList.remove("open");
    el.querySelector("button")?.setAttribute("aria-expanded", "false");
  }
  if (drawn.size && !frame) {
    last = performance.now();
    frame = requestAnimationFrame(tick);
  }
}

/// Reads the live thoughts a frame further; stops once none is drawn.
function tick(now: number) {
  const dt = Math.min(0.1, (now - last) / 1000);
  last = now;
  const els = document.querySelectorAll<HTMLElement>(".kk[data-kk]");
  frame = 0;
  if (!els.length) return;
  const calm = still();
  let through = false;
  for (const el of els) {
    const s = shown.get(el.dataset.kk ?? "");
    if (!s || s.shown >= s.text.length) continue;
    const before = Math.floor(s.shown);
    s.shown = reveal(s.shown, s.text.length, dt, calm);
    if (Math.floor(s.shown) === before) continue;
    const { old, lit, rest } = splitBand(s.text, s.shown, calm ? 0 : BAND);
    const [o, n, u] = el.children;
    if (o) o.textContent = old;
    if (n) n.textContent = lit;
    if (u) u.textContent = rest;
    fit(el, s.follow);
    if (s.ended && s.shown >= s.text.length) through = true;
  }
  frame = requestAnimationFrame(tick);
  if (through) readThrough();
}
