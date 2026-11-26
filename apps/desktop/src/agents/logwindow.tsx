// A session's log in the page: only the turns near the view are drawn, in
// a `.agturn` each; spacers above and below stand for the rest, as tall as
// those turns were measured. React draws the turns from their typed rows;
// this module picks which turns are drawn and keeps the reader's place.
// A turn draws again only when one of its rows changed, and then only that
// row, so a streaming reply updates alone.
import { flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import type { TurnHeading } from "./turn-heading-view.tsx";
import type { BlockedRow, ContextRow, PromptRow, ReplyRow } from "./reply-view.tsx";
import type { ActivityRow } from "./activity-view.tsx";
import { TurnRows } from "./log-shell-view.tsx";
import { frameResize } from "../ui/dom.ts";
import { endOf, guess, shiftAbove, spacer, windowOf, type Resized } from "./logwindow-state.ts";
import { keptScrollTop, markPlace, pinWait } from "./logscroll.ts";
import type { ChatBody } from "../ui/turn.ts";

export type TranscriptRow = TurnHeading | BlockedRow | ReplyRow | PromptRow | ContextRow | ActivityRow;

/// One turn of the log: a key that stays while the turn does, and its
/// rows, worked out only when it is drawn.
export interface Unit {
  key: string;
  rows: () => readonly TranscriptRow[];
  /// The message the user sent to open the turn, or null.
  prompt: string | null;
}

/// A row as drawn and its key in the turn.
export interface KeyedRow { key: string; row: TranscriptRow }
/// A drawn turn. The same object stays while none of its rows changes.
/// `source` is the rows it was last drawn from: given them again, it stays.
export interface DrawnTurn { key: string; rows: readonly KeyedRow[]; source: readonly TranscriptRow[] }
interface LogView { turns: readonly DrawnTurn[]; top: number | null; bottom: number | null }

export interface LogWindow {
  units: readonly Unit[];
  drawn: Map<string, DrawnTurn>;
  /// Turns whose rows changed or were mounted since their height was last read.
  dirty: Set<string>;
  /// The drawn turns' elements, by key, while they are in the page.
  elements: Map<string, HTMLElement>;
  /// How much room each turn took, its gap included, by key.
  sizes: Map<string, number>;
  /// The log's width when those were measured.
  width: number;
  gap: number;
  /// The spacers' heights, null when a spacer stands for nothing.
  top: number | null;
  bottom: number | null;
  el: HTMLElement | null;
  /// The session body the log scrolls in.
  body: ChatBody;
  watch: ResizeObserver;
  view: LogView;
  listeners: Set<() => void>;
  search: { sources: { key: string; root: HTMLElement }[]; view: DocumentView } | null;
  searchChanged: (() => void) | null;
  attach(el: HTMLDivElement | null): void;
  subscribe(listener: () => void): () => void;
  snapshot(): LogView;
}

const windows = new WeakMap<Element, LogWindow>();
/// Each session body's log window, so a body finds its log without a lookup.
const bodies = new WeakMap<ChatBody, LogWindow>();

/// How tall a turn is taken to be before any was measured.
const FALLBACK = 120;

/// A log window for session body `body`; `attach` is the ref of its `.dcoderlog`.
export function createLog(body: ChatBody): LogWindow {
  const w: LogWindow = {
    units: [], drawn: new Map(), dirty: new Set(), elements: new Map(), sizes: new Map(), width: 0, gap: 0, top: null, bottom: null, el: null, body,
    watch: frameResize(() => resized(w)), view: { turns: [], top: null, bottom: null }, listeners: new Set(), search: null, searchChanged: null,
    attach: el => {
      if (w.el) windows.delete(w.el);
      w.el = el;
      if (!el) return;
      windows.set(el, w);
      w.gap = parseFloat(getComputedStyle(el).rowGap) || 0;
      w.watch.observe(body);
    },
    subscribe: listener => { w.listeners.add(listener); return () => { w.listeners.delete(listener); }; },
    snapshot: () => w.view,
  };
  bodies.set(body, w);
  return w;
}

/// Forgets the turns, as when the log leaves the page.
export function resetLog(w: LogWindow) {
  w.units = [];
  w.drawn = new Map();
  w.dirty.clear();
  w.sizes.clear();
  w.top = w.bottom = null;
  dropSearch(w);
  publish(w);
}

/// A detached cached log remains live until its session body is replaced.
export function disposeLog(w: LogWindow) {
  w.watch.disconnect();
  dropSearch(w);
  w.listeners.clear();
  if (w.el) windows.delete(w.el);
  if (bodies.get(w.body) === w) bodies.delete(w.body);
}

export function watchLogSearch(log: HTMLElement, changed: () => void) {
  const w = windows.get(log);
  if (!w) return () => {};
  w.searchChanged = changed;
  return () => { if (w.searchChanged === changed) w.searchChanged = null; dropSearch(w); };
}

/// Gives the log the turns `units`. The turns drawn now take their new rows,
/// and those that left the log go; nothing new is drawn until `fillLog`.
/// False, changing nothing, when a row that would change or go holds one of `held`.
export function setUnits(w: LogWindow, units: readonly Unit[], held: readonly Node[]) {
  const keys = new Set(units.map(u => u.key));
  const next = new Map<string, DrawnTurn>();
  const going: Element[] = [];

  for (const u of units) {
    const prior = w.drawn.get(u.key);
    if (!prior) continue;
    const turn = drawnTurn(u, prior), el = w.elements.get(u.key);
    next.set(u.key, turn);
    if (turn === prior) continue;
    w.dirty.add(u.key);
    if (!el) continue;

    const kept = new Set(turn.rows);
    prior.rows.forEach((row, i) => {
      const child = el.children[i];
      if (child && !kept.has(row)) going.push(child);
    });
  }

  for (const key of w.drawn.keys()) {
    const el = w.elements.get(key);
    if (!keys.has(key) && el) going.push(el);
  }

  // Leave the whole update pending when it would replace a selection or focus.
  if (going.some(el => held.some(n => el.contains(n)))) return false;
  for (const key of [...w.dirty]) if (!keys.has(key)) w.dirty.delete(key);
  w.units = units;
  dropSearch(w);
  w.searchChanged?.();
  // Turns read further back come in above, perhaps between the head and the
  // turns drawn: the drawn ones nearest the end stay, in one run.
  const at = units.flatMap((u, i) => next.has(u.key) ? [i] : []);
  let first = at.length - 1;
  while (first > 0 && at[first - 1] === at[first] - 1) first--;
  for (const i of at.slice(0, Math.max(0, first))) next.delete(units[i].key);
  w.drawn = next;
  // The spacers stand for the turns as they are now, so a scroll kept by a row
  // in view is kept against the new heights.
  space(w, at[first] ?? units.length, (at.at(-1) ?? units.length - 1) + 1);
  publish(w);
  return true;
}

/// Draws the turns in view and a screen's height around it, and lets the
/// others go. With `follow` the log stays scrolled to its end (once the
/// wheel rests, see `pinEnd`); otherwise the first turn in view stays
/// where it is on screen.
export function fillLog(b: ChatBody, follow: boolean) {
  const w = bodies.get(b), log = w?.el;
  if (!log || !w) return false;
  // A log at its end draws the turns there from their sizes, with no layout
  // read before they are in, and goes to its end once they are.
  const pin = follow && mayPin(b);
  // Turns measured at another width are guessed again.
  if (log.clientWidth !== w.width) { w.width = log.clientWidth; w.sizes.clear(); w.dirty.clear(); }
  let changed = false;

  // Measuring what was guessed can shift the window; one correction pass settles it.
  for (let round = 0; round < 2; round++) {
    const bTop = pin ? 0 : b.getBoundingClientRect().top;
    const { from, to } = pin ? endOf(sizesOf(w), b.clientHeight) : viewOf(w, b, log, bTop);
    const shown = w.units.slice(from, to);
    const same = w.drawn.size === shown.length && shown.every(u => w.drawn.has(u.key));
    const needsMeasure = shown.some(u => !w.sizes.has(u.key) || w.dirty.has(u.key));

    if (same && !needsMeasure) {
      if (pin) { b.scrollTop = b.scrollHeight; markPlace(b); }
      break;
    }

    // The first turn in view that stays, and where it is on screen.
    const anchor = pin ? undefined : shown.map(u => w.elements.get(u.key)).find(el => el && el.getBoundingClientRect().bottom > bTop);
    const before = anchor?.getBoundingClientRect().top;

    if (!same) {
      changed = true;
      for (const u of shown) if (!w.drawn.has(u.key)) w.dirty.add(u.key);
      w.drawn = new Map(shown.map(u => [u.key, w.drawn.get(u.key) ?? drawnTurn(u, null)]));
      space(w, from, to);
      publish(w);
    }

    measure(w);
    space(w, from, to);
    publish(w);

    if (pin) b.scrollTop = b.scrollHeight;
    else if (anchor && before != null) {
      const top = keptScrollTop(b.scrollTop, before, anchor.getBoundingClientRect().top);
      // Even writing the current position interrupts WebKit's wheel glide.
      if (top !== b.scrollTop) b.scrollTop = top;
    }

    // The place is what the code left, so a hidden log comes back to the same row.
    markPlace(b);

    if (same) break;
  }

  return changed;
}

/// The turns around the view of `b`, whose top is at `bTop`.
function viewOf(w: LogWindow, b: ChatBody, log: HTMLElement, bTop: number) {
  const y = bTop - log.getBoundingClientRect().top;
  return windowOf(sizesOf(w), y - b.clientHeight, y + 2 * b.clientHeight);
}

const pinning = new WeakSet<ChatBody>();

/// Scrolls a log that follows its end to its end, true when done now.
export function pinEnd(b: ChatBody) {
  if (!mayPin(b)) {
    // The gap may have grown with the pin held back: the next scroll is
    // judged against where the box stands now, not where it last stood.
    markPlace(b);
    return false;
  }
  b.scrollTop = b.scrollHeight;
  markPlace(b);
  return true;
}

/// Whether a log that follows its end may go there now. While the reader's
/// wheel turns it waits until the wheel rests, then goes only if the log
/// still follows: a write during a flick's glide stops it and holds back
/// the next scroll.
function mayPin(b: ChatBody) {
  if (b.touching) return false;
  const wait = pinWait(performance.now(), b.wheel);
  if (wait <= 0) return true;

  if (!pinning.has(b)) {
    pinning.add(b);
    setTimeout(() => { pinning.delete(b); if (b.isConnected && b.stuck !== false) pinEnd(b); }, wait);
  }

  return false;
}

/// Search all held turns without mounting the entire conversation: they
/// are drawn apart from the page until the log changes or search ends.
export function logSearchSources(log: HTMLElement) {
  const w = windows.get(log);
  if (!w) return [];

  if (!w.search) {
    const host = document.createElement("div"), view = createDocumentView(host);
    flushSync(() => view.render(w.units.map(u => <div key={u.key}><TurnRows rows={keyedRows(u.rows())} /></div>)));
    const roots = [...host.children].filter((el): el is HTMLElement => el instanceof HTMLElement);
    w.search = { view, sources: w.units.map((u, i) => ({ key: u.key, root: roots[i] ?? document.createElement("div") })) };
  }

  return w.search.sources;
}

/// Mounts the requested turn and scrolls it into the virtual window.
export function revealLogUnit(b: ChatBody, key: string) {
  const w = bodies.get(b), log = w?.el;
  if (!log || !w) return null;
  const at = w.units.findIndex(u => u.key === key);
  if (at < 0) return null;
  b.stuck = false;
  // Measure the target before filling its surrounding turns.
  w.dirty.add(key);
  w.drawn = new Map([[key, w.drawn.get(key) ?? drawnTurn(w.units[at], null)]]);
  space(w, at, at + 1);
  publish(w);
  measure(w);
  space(w, at, at + 1);
  publish(w);
  const el = w.elements.get(key);
  if (el) b.scrollTop += el.getBoundingClientRect().top - b.getBoundingClientRect().top;
  fillLog(b, false);
  return w.elements.get(key) ?? null;
}

function publish(w: LogWindow) {
  const turns = w.units.flatMap(u => { const d = w.drawn.get(u.key); return d ? [d] : []; });
  const view = w.view;
  if (view.top === w.top && view.bottom === w.bottom && view.turns.length === turns.length && view.turns.every((turn, i) => turn === turns[i])) return;
  w.view = { turns, top: w.top, bottom: w.bottom };
  flushSync(() => { for (const listener of w.listeners) listener(); });
}

function dropSearch(w: LogWindow) {
  w.search?.view.unmount();
  w.search = null;
}

/// The turn `u` as drawn now, keeping `prior`'s rows that did not change.
function drawnTurn(u: Unit, prior: DrawnTurn | null): DrawnTurn {
  const source = u.rows();
  if (prior?.source === source) return prior;
  const rows = keyedRows(source);
  if (!prior) return { key: u.key, rows, source };
  const old = new Map(prior.rows.map(row => [row.key, row]));
  const merged = rows.map(row => { const was = old.get(row.key); return was && sameData(was.row, row.row) ? was : row; });

  if (merged.length === prior.rows.length && merged.every((row, i) => row === prior.rows[i])) {
    prior.source = source;
    return prior;
  }

  return { key: u.key, rows: merged, source };
}

function keyedRows(rows: readonly TranscriptRow[]): KeyedRow[] {
  const seen = new Map<string, number>();
  return rows.map(row => {
    const base = row.kind === "reply" || row.kind === "prompt" ? `${row.kind}:${row.row}` : `${row.kind}:${row.id}`;
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    return { key: n ? `${base}#${n}` : base, row };
  });
}

/// Whether two rows hold the same data. Rows are plain data; their
/// callbacks are made again each draw and do not count.
function sameData(a: unknown, b: unknown): boolean {
  if (Object.is(a, b) || (typeof a === "function" && typeof b === "function")) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null || Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((value, i) => sameData(value, b[i]));
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every(key => Object.hasOwn(b, key) && sameData(Reflect.get(a, key), Reflect.get(b, key)));
}

/// A turn grew or shrank by itself: a picture loaded, a fold opened. One
/// wholly above the view moves it by as much, so what is read stays put;
/// a log that follows its end stays at the end.
function resized(w: LogWindow) {
  const log = w.el, b = log?.closest<ChatBody>(".dbody");
  if (!log || !b || !log.isConnected) return;
  const bTop = b.getBoundingClientRect().top, moved: Resized[] = [];

  for (const [key, el] of w.elements) {
    const was = w.sizes.get(key), now = sizeOf(w, el);
    if (was == null || was === now) continue;
    moved.push({ top: el.getBoundingClientRect().top - bTop, was, now });
    w.sizes.set(key, now);
  }

  // The composer can resize the viewport without changing any turn.
  if (!moved.length) {
    if (b.stuck !== false) pinEnd(b);
    return;
  }

  const keys = w.units.map(u => u.key);
  const drawn = keys.filter(k => w.elements.has(k));
  // The guess for turns not drawn moved with these, and the spacers with it.
  const anchor = drawn.flatMap(k => w.elements.get(k) ?? []).find(el => el.getBoundingClientRect().bottom > bTop);
  const before = anchor?.getBoundingClientRect().top;
  space(w, keys.indexOf(drawn[0]), keys.indexOf(drawn[drawn.length - 1]) + 1);
  publish(w);

  if (b.stuck !== false) pinEnd(b);
  else if (anchor && before != null) {
    const top = keptScrollTop(b.scrollTop, before - shiftAbove(moved), anchor.getBoundingClientRect().top);
    if (top !== b.scrollTop) b.scrollTop = top;
    markPlace(b);
  }
}

/// The room a drawn turn takes, its gap included; a hidden one takes none.
const sizeOf = (w: LogWindow, el: HTMLElement) => el.hidden ? 0 : el.offsetHeight + w.gap;

/// A message the user sent, for the conversation's scrollbar: its turn and
/// where it starts, in pixels from the top of what `b` scrolls.
interface PromptMark { key: string; top: number; prompt: string }

/// Where each message the user sent starts in `b`, measured turns as drawn and
/// the others as guessed, as the spacers stand for them.
export function promptMarks(b: ChatBody): PromptMark[] {
  const w = bodies.get(b), log = w?.el;
  if (!w || !log?.isConnected) return [];
  const sizes = sizesOf(w), marks: PromptMark[] = [];
  let top = log.getBoundingClientRect().top - b.getBoundingClientRect().top + b.scrollTop;

  w.units.forEach((u, i) => {
    if (u.prompt) marks.push({ key: u.key, top, prompt: u.prompt });
    top += sizes[i];
  });

  return marks;
}

/// Calls `changed` when the turns drawn in `b`'s log change; returns how to stop.
export function onLogDrawn(b: ChatBody, changed: () => void) {
  return bodies.get(b)?.subscribe(changed) ?? (() => {});
}

function measure(w: LogWindow) {
  for (const key of w.drawn.keys()) {
    if (w.sizes.has(key) && !w.dirty.has(key)) continue;
    const el = w.elements.get(key);
    if (el) {
      w.sizes.set(key, sizeOf(w, el));
      w.dirty.delete(key);
    }
  }
}

function sizesOf(w: LogWindow) {
  const each = guess(w.sizes.values(), FALLBACK);
  return w.units.map(u => w.sizes.get(u.key) ?? each);
}

/// Sets the spacers for the turns before `from` and from `to` on.
function space(w: LogWindow, from: number, to: number) {
  const sizes = sizesOf(w);
  w.top = spacer(sizes, 0, from, w.gap);
  w.bottom = spacer(sizes, to, sizes.length, w.gap);
}
