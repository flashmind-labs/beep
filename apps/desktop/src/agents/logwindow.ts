// A session's log in the page: only the turns near the view are drawn, in
// a `.agturn` each; spacers above and below stand for the rest, as tall as
// those turns were measured. A redraw patches the drawn turns row by row;
// scrolling draws the turns it reaches from the html of the last redraw.

import { reuseRows } from "./logdiff-state.ts";
import { guess, shiftAbove, spacer, windowOf, type Resized } from "./logwindow-state.ts";
import { keptScrollTop } from "./logscroll.ts";
import type { ChatBody } from "../ui/turn.ts";

/// One turn of the log: a key that stays while the turn does, and its
/// rows as html, worked out only when it is drawn.
export interface Unit {
  key: string;
  rows: () => readonly string[];
}

/// A turn in the page: its element, the html of its rows and their elements.
interface Drawn {
  el: HTMLElement;
  html: readonly string[];
  nodes: Element[][];
}

interface LogWindow {
  units: readonly Unit[];
  drawn: Map<string, Drawn>;
  /// How much room each turn took, its gap included, by key.
  sizes: Map<string, number>;
  /// The log's width when those were measured.
  width: number;
  gap: number;
  top: HTMLElement;
  bottom: HTMLElement;
  watch: ResizeObserver;
}

const windows = new WeakMap<Element, LogWindow>();

/// How tall a turn is taken to be before any was measured.
const FALLBACK = 120;

/// Gives `log` the turns `units`. The turns drawn now are patched to them,
/// and those that left the log go; nothing new is drawn until `fillLog`.
/// False, changing nothing, when a row that would go holds one of `held`.
export function setUnits(log: HTMLElement, units: readonly Unit[], held: readonly Node[]) {
  const w = windows.get(log) ?? start(log);
  const keys = new Set(units.map(u => u.key));
  const plans: { d: Drawn; html: readonly string[]; reuse: number[] }[] = [];
  const going: Element[] = [];
  for (const u of units) {
    const d = w.drawn.get(u.key);
    if (!d) continue;
    const html = u.rows(), reuse = reuseRows(d.html, html);
    const keep = new Set(reuse.filter(i => i >= 0));
    d.nodes.forEach((n, i) => { if (!keep.has(i)) going.push(...n); });
    plans.push({ d, html, reuse });
  }
  for (const [key, d] of w.drawn) if (!keys.has(key)) going.push(d.el);
  if (going.some(el => held.some(n => el.contains(n)))) return false;
  for (const [key, d] of w.drawn) if (!keys.has(key)) undraw(w, key, d);
  for (const { d, html, reuse } of plans) patch(d, html, reuse);
  w.units = units;
  // Turns read further back come in above, perhaps between the head and the
  // turns drawn: the drawn ones nearest the end stay, in one run.
  const at = units.flatMap((u, i) => w.drawn.has(u.key) ? [i] : []);
  let first = at.length - 1;
  while (first > 0 && at[first - 1] === at[first] - 1) first--;
  for (const i of at.slice(0, Math.max(0, first))) undraw(w, units[i].key, w.drawn.get(units[i].key)!);
  // The spacers stand for the turns as they are now, so a scroll kept by a row
  // in view is kept against the new heights.
  space(w, at[first] ?? units.length, (at.at(-1) ?? units.length - 1) + 1);
  return true;
}

/// Draws the turns in view and a screen's height around it, and lets the
/// others go. With `pin` the log stays scrolled to its end; otherwise the
/// first turn in view stays where it is on screen.
export function fillLog(b: HTMLElement, pin: boolean) {
  const log = b.querySelector<HTMLElement>(".dcoderlog");
  const w = log && windows.get(log);
  if (!log || !w) return false;
  // Turns measured at another width are guessed again.
  if (log.clientWidth !== w.width) { w.width = log.clientWidth; w.sizes.clear(); }
  let changed = false;
  // Measuring what was guessed moves the window; it settles in a few rounds.
  for (let round = 0; round < 4; round++) {
    const bTop = b.getBoundingClientRect().top, y = bTop - log.getBoundingClientRect().top;
    const { from, to } = windowOf(sizesOf(w), y - b.clientHeight, y + 2 * b.clientHeight);
    const shown = w.units.slice(from, to), want = new Set(shown.map(u => u.key));
    const same = w.drawn.size === want.size && shown.every(u => w.drawn.has(u.key));
    // The first turn in view that stays, and where it is on screen.
    const anchor = pin ? undefined : shown.map(u => w.drawn.get(u.key)?.el).find(el => el && el.getBoundingClientRect().bottom > bTop);
    const before = anchor?.getBoundingClientRect().top;
    if (!same) {
      changed = true;
      for (const [key, d] of w.drawn) if (!want.has(key)) undraw(w, key, d);
      let cursor: Element | null = w.top.nextElementSibling;
      for (const u of shown) {
        const d = w.drawn.get(u.key) ?? draw(w, u);
        if (d.el === cursor) cursor = cursor.nextElementSibling;
        else log.insertBefore(d.el, cursor);
      }
    }
    measure(w);
    space(w, from, to);
    if (pin) b.scrollTop = b.scrollHeight;
    else if (anchor && before != null) b.scrollTop = keptScrollTop(b.scrollTop, before, anchor.getBoundingClientRect().top);
    if (same) break;
  }
  return changed;
}

function start(log: HTMLElement): LogWindow {
  const top = document.createElement("div"), bottom = document.createElement("div");
  top.className = bottom.className = "agspace";
  top.hidden = bottom.hidden = true;
  log.replaceChildren(top, bottom);
  const w: LogWindow = {
    units: [], drawn: new Map(), sizes: new Map(), width: 0,
    gap: parseFloat(getComputedStyle(log).rowGap) || 0, top, bottom,
    watch: new ResizeObserver(() => resized(log, w)),
  };
  windows.set(log, w);
  return w;
}

/// A turn grew or shrank by itself: a picture loaded, a fold opened. One
/// wholly above the view moves it by as much, so what is read stays put;
/// a log that follows its end stays at the end.
function resized(log: HTMLElement, w: LogWindow) {
  const b = log.closest<ChatBody>(".dbody");
  if (!b || !log.isConnected) return;
  const bTop = b.getBoundingClientRect().top, moved: Resized[] = [];
  for (const [key, d] of w.drawn) {
    const was = w.sizes.get(key), now = sizeOf(w, d);
    if (was == null || was === now) continue;
    moved.push({ top: d.el.getBoundingClientRect().top - bTop, was, now });
    w.sizes.set(key, now);
  }
  if (!moved.length) return;
  const keys = w.units.map(u => u.key);
  const drawn = keys.filter(k => w.drawn.has(k));
  // The guess for turns not drawn moved with these, and the spacers with it.
  const anchor = drawn.map(k => w.drawn.get(k)!.el).find(el => el.getBoundingClientRect().bottom > bTop);
  const before = anchor?.getBoundingClientRect().top;
  space(w, keys.indexOf(drawn[0]), keys.indexOf(drawn[drawn.length - 1]) + 1);
  if (b.stuck !== false) b.scrollTop = b.scrollHeight;
  else if (anchor && before != null) b.scrollTop = keptScrollTop(b.scrollTop, before - shiftAbove(moved), anchor.getBoundingClientRect().top);
}

/// The room a drawn turn takes, its gap included; a hidden one takes none.
const sizeOf = (w: LogWindow, d: Drawn) => d.el.hidden ? 0 : d.el.offsetHeight + w.gap;

function measure(w: LogWindow) {
  for (const [key, d] of w.drawn) w.sizes.set(key, sizeOf(w, d));
}

function sizesOf(w: LogWindow) {
  const each = guess(w.sizes.values(), FALLBACK);
  return w.units.map(u => w.sizes.get(u.key) ?? each);
}

/// Sets the spacers for the turns before `from` and from `to` on.
function space(w: LogWindow, from: number, to: number) {
  const sizes = sizesOf(w);
  for (const [el, h] of [[w.top, spacer(sizes, 0, from, w.gap)], [w.bottom, spacer(sizes, to, sizes.length, w.gap)]] as const) {
    el.hidden = h == null;
    el.style.height = `${h ?? 0}px`;
  }
}

function draw(w: LogWindow, u: Unit): Drawn {
  const el = document.createElement("div");
  el.className = "agturn";
  const d: Drawn = { el, html: [], nodes: [] };
  patch(d, u.rows(), []);
  w.drawn.set(u.key, d);
  w.watch.observe(el);
  return d;
}

function undraw(w: LogWindow, key: string, d: Drawn) {
  w.watch.unobserve(d.el);
  d.el.remove();
  w.drawn.delete(key);
}

/// Makes a drawn turn's rows `html`, reusing the rows `reuse` names.
function patch(d: Drawn, html: readonly string[], reuse: readonly number[]) {
  const keep = new Set(reuse.filter(i => i >= 0));
  d.nodes.forEach((n, i) => { if (!keep.has(i)) for (const el of n) el.remove(); });
  const nodes = html.map((row, i) => {
    if (reuse[i] >= 0) return d.nodes[reuse[i]];
    const t = document.createElement("template");
    t.innerHTML = row;
    return [...t.content.children];
  });
  let cursor: Element | null = d.el.firstElementChild;
  for (const el of nodes.flat()) {
    if (el === cursor) cursor = cursor.nextElementSibling;
    else d.el.insertBefore(el, cursor);
  }
  d.el.hidden = !d.el.firstElementChild;
  d.html = html;
  d.nodes = nodes;
}
