import { createElement, Fragment } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { AgentTurnHeading, type TurnHeading } from "./turn-heading-view.tsx";
import { AgentReply, replyHtml, replyRow, type ReplyRow } from "./reply-view.tsx";
import { frameResize } from "../ui/dom.ts";

// A session's log in the page: only the turns near the view are drawn, in
// a `.agturn` each; spacers above and below stand for the rest, as tall as
// those turns were measured. A redraw patches the drawn turns row by row;
// scrolling draws the turns it reaches from the html of the last redraw.

import { reuseRows } from "./logdiff-state.ts";
import { guess, shiftAbove, spacer, windowOf, type Resized } from "./logwindow-state.ts";
import { keptScrollTop, pinWait } from "./logscroll.ts";
import type { ChatBody } from "../ui/turn.ts";

/// One turn of the log: a key that stays while the turn does, and its
/// rows as html, worked out only when it is drawn.
export type TranscriptRow = string | TurnHeading | ReplyRow;
const rowHtml = (row: TranscriptRow) => typeof row === "string" ? row : row.kind === "reply" ? replyHtml(row) : row.html;
const asReply = (row: TranscriptRow) => typeof row === "string" ? replyRow(row) : row.kind === "reply" ? row : null;

export interface Unit {
  key: string;
  rows: () => readonly TranscriptRow[];
}

/// A turn in the page: its element, the html of its rows and their elements.
interface Drawn {
  el: HTMLElement;
  html: readonly string[];
  nodes: Element[][];
  rows: readonly TranscriptRow[];
}

interface LogWindow {
  units: readonly Unit[];
  search?: { key: string; root: HTMLElement }[];
  drawn: Map<string, Drawn>;
  /// How much room each turn took, its gap included, by key.
  sizes: Map<string, number>;
  /// The log's width when those were measured.
  width: number;
  gap: number;
  top: HTMLElement;
  bottom: HTMLElement;
  watch: ResizeObserver;
  replies: Map<HTMLElement, { key: number; reply: ReplyRow }>;
  headings: Map<HTMLElement, { key: number; value: TurnHeading }>;
  nextReplyKey: number;
  replyRoot: Root | null;
}

const windows = new WeakMap<Element, LogWindow>();
const searchWatches = new WeakMap<HTMLElement, () => void>();

export function watchLogSearch(log: HTMLElement, changed: () => void) {
  searchWatches.set(log, changed);
  return () => { searchWatches.delete(log); const w = windows.get(log); if (w) w.search = undefined; };
}

/// How tall a turn is taken to be before any was measured.
const FALLBACK = 120;

/// Gives `log` the turns `units`. The turns drawn now are patched to them,
/// and those that left the log go; nothing new is drawn until `fillLog`.
/// False, changing nothing, when a row that would go holds one of `held`.
export function setUnits(log: HTMLElement, units: readonly Unit[], held: readonly Node[]) {
  const w = windows.get(log) ?? start(log);
  const keys = new Set(units.map(u => u.key));
  const plans: { d: Drawn; rows: readonly TranscriptRow[]; reuse: number[] }[] = [];
  const going: Element[] = [];

  for (const u of units) {
    const d = w.drawn.get(u.key);
    if (!d) continue;
    const rows = u.rows(), html = rows.map(rowHtml), reuse = reuseReplyRows(d.rows, rows);
    reuseHeadings(d.rows, rows, reuse);
    const keep = new Set(reuse.filter(i => i >= 0));
    const changed = new Set(reuse.filter((old, next) => old >= 0 && d.html[old] !== html[next]));
    d.nodes.forEach((nodes, i) => { if (!keep.has(i) || changed.has(i)) going.push(...nodes); });
    plans.push({ d, rows, reuse });
  }

  for (const [key, d] of w.drawn) if (!keys.has(key)) going.push(d.el);
  if (going.some(el => held.some(n => el.contains(n)))) return false;
  for (const [key, d] of w.drawn) if (!keys.has(key)) undraw(w, key, d);
  for (const { d, rows, reuse } of plans) patch(w, d, rows, reuse);
  w.units = units;
  w.search = undefined;
  searchWatches.get(log)?.();
  // Turns read further back come in above, perhaps between the head and the
  // turns drawn: the drawn ones nearest the end stay, in one run.
  const at = units.flatMap((u, i) => w.drawn.has(u.key) ? [i] : []);
  let first = at.length - 1;
  while (first > 0 && at[first - 1] === at[first] - 1) first--;
  for (const i of at.slice(0, Math.max(0, first))) undraw(w, units[i].key, w.drawn.get(units[i].key)!);
  renderReplies(w);
  // The spacers stand for the turns as they are now, so a scroll kept by a row
  // in view is kept against the new heights.
  space(w, at[first] ?? units.length, (at.at(-1) ?? units.length - 1) + 1);
  return true;
}

/// Draws the turns in view and a screen's height around it, and lets the
/// others go. With `follow` the log stays scrolled to its end (once the
/// wheel rests, see `pinEnd`); otherwise the first turn in view stays
/// where it is on screen.
export function fillLog(b: ChatBody, follow: boolean) {
  const log = b.querySelector<HTMLElement>(".dcoderlog");
  const w = log && windows.get(log);
  if (!log || !w) return false;
  const pin = follow && pinEnd(b);
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

    renderReplies(w);
    measure(w);
    space(w, from, to);

    if (pin) b.scrollTop = b.scrollHeight;
    else if (anchor && before != null) {
      const top = keptScrollTop(b.scrollTop, before, anchor.getBoundingClientRect().top);
      // Even writing the current position interrupts WebKit's wheel glide.
      if (top !== b.scrollTop) b.scrollTop = top;
    }

    if (same) break;
  }

  return changed;
}

const pinning = new WeakSet<ChatBody>();

/// Scrolls a log that follows its end to its end, true when done now. While
/// the reader's wheel turns it waits until the wheel rests, then goes only
/// if the log still follows: a write during a flick's glide stops it and
/// holds back the next scroll.
export function pinEnd(b: ChatBody) {
  const wait = pinWait(performance.now(), b.wheel);

  if (wait <= 0) {
    b.scrollTop = b.scrollHeight;
    return true;
  }

  if (!pinning.has(b)) {
    pinning.add(b);
    setTimeout(() => { pinning.delete(b); if (b.isConnected && b.stuck !== false) pinEnd(b); }, wait);
  }

  return false;
}

function start(log: HTMLElement): LogWindow {
  const top = document.createElement("div"), bottom = document.createElement("div");
  top.className = bottom.className = "agspace";
  top.hidden = bottom.hidden = true;
  log.replaceChildren(top, bottom);
  const w: LogWindow = {
    units: [], drawn: new Map(), sizes: new Map(), width: 0,
    gap: parseFloat(getComputedStyle(log).rowGap) || 0, top, bottom,
    watch: frameResize(() => resized(log, w)), replies: new Map(), headings: new Map(), nextReplyKey: 0, replyRoot: null,
  };
  const body = log.closest<ChatBody>(".dbody");
  if (body) w.watch.observe(body);
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

  // The composer can resize the viewport without changing any turn.
  if (!moved.length) {
    if (b.stuck !== false) pinEnd(b);
    return;
  }
  const keys = w.units.map(u => u.key);
  const drawn = keys.filter(k => w.drawn.has(k));
  // The guess for turns not drawn moved with these, and the spacers with it.
  const anchor = drawn.map(k => w.drawn.get(k)!.el).find(el => el.getBoundingClientRect().bottom > bTop);
  const before = anchor?.getBoundingClientRect().top;
  space(w, keys.indexOf(drawn[0]), keys.indexOf(drawn[drawn.length - 1]) + 1);

  if (b.stuck !== false) pinEnd(b);
  else if (anchor && before != null) {
    const top = keptScrollTop(b.scrollTop, before - shiftAbove(moved), anchor.getBoundingClientRect().top);
    if (top !== b.scrollTop) b.scrollTop = top;
  }
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
  el.dataset.findKey = u.key;
  const d: Drawn = { el, html: [], rows: [], nodes: [] };
  patch(w, d, u.rows(), []);
  w.drawn.set(u.key, d);
  w.watch.observe(el);
  return d;
}

function undraw(w: LogWindow, key: string, d: Drawn) {
  w.watch.unobserve(d.el);
  for (const nodes of d.nodes) for (const node of nodes) if (node instanceof HTMLElement) { w.replies.delete(node); w.headings.delete(node); }
  d.el.remove();
  w.drawn.delete(key);
}

/// Makes a drawn turn's rows `html`, reusing the rows `reuse` names.
function patch(w: LogWindow, d: Drawn, rows: readonly TranscriptRow[], reuse: readonly number[]) {
  const html = rows.map(rowHtml);
  const keep = new Set(reuse.filter(i => i >= 0));
  d.nodes.forEach((n, i) => { if (!keep.has(i)) for (const el of n) { if (el instanceof HTMLElement) { w.replies.delete(el); w.headings.delete(el); } el.remove(); } });
  const nodes = html.map((row, i) => {
    if (reuse[i] >= 0 && d.html[reuse[i]] === row) return d.nodes[reuse[i]];
    const typed = rows[i];
    const heading = typeof typed !== "string" && typed.kind === "heading" ? typed : null;
    const reply = asReply(typed);
    if (reuse[i] >= 0) {
      const nodes = d.nodes[reuse[i]];
      const existing = nodes[0] instanceof HTMLElement ? w.replies.get(nodes[0]) : null;
      if (reply && existing && nodes[0] instanceof HTMLElement) w.replies.set(nodes[0], { ...existing, reply });
      const current = nodes[0] instanceof HTMLElement ? w.headings.get(nodes[0]) : null;
      if (heading && current && nodes[0] instanceof HTMLElement) w.headings.set(nodes[0], { ...current, value: heading });
      return nodes;
    }
    if (heading) {
      const host = document.createElement("div");
      host.style.display = "contents";
      w.headings.set(host, { key: ++w.nextReplyKey, value: heading });
      return [host];
    }
    if (reply) {
      const host = document.createElement("div");
      host.style.display = "contents";
      w.replies.set(host, { key: ++w.nextReplyKey, reply });
      return [host];
    }
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
  d.rows = rows;
  d.nodes = nodes;
}

/// Preserve a reply's host while its streamed content changes.
function reuseReplyRows(before: readonly TranscriptRow[], next: readonly TranscriptRow[]) {
  const signature = (row: TranscriptRow) => typeof row === "string" ? `s${row}` : row.kind === "heading" ? `h${JSON.stringify([row.id, row.html])}` : `r${JSON.stringify([row.row, row.markdown, row.html])}`;
  const reuse = reuseRows(before.map(signature), next.map(signature));
  if (!reuse.includes(-1)) return reuse;
  const used = new Set(reuse.filter(index => index >= 0));
  const available = new Map<string, { indices: number[]; next: number }>();
  for (const [index, row] of before.entries()) {
    if (used.has(index)) continue;
    const reply = asReply(row);
    if (!reply) continue;
    let matches = available.get(reply.row);
    if (!matches) { matches = { indices: [], next: 0 }; available.set(reply.row, matches); }
    matches.indices.push(index);
  }
  for (let i = 0; i < next.length; i++) {
    if (reuse[i] >= 0) continue;
    const candidate = next[i];
    const reply = asReply(candidate);
    if (!reply) continue;
    const matches = available.get(reply.row);
    if (matches && matches.next < matches.indices.length) reuse[i] = matches.indices[matches.next++];
  }
  return reuse;
}

function reuseHeadings(before: readonly TranscriptRow[], next: readonly TranscriptRow[], reuse: number[]) {
  const used = new Set(reuse.filter(index => index >= 0));
  const available = new Map<string, { indices: number[]; next: number }>();
  before.forEach((row, i) => {
    if (typeof row === "string" || row.kind !== "heading" || used.has(i)) return;
    let matches = available.get(row.id);
    if (!matches) { matches = { indices: [], next: 0 }; available.set(row.id, matches); }
    matches.indices.push(i);
  });
  next.forEach((row, i) => {
    if (reuse[i] >= 0 || typeof row === "string" || row.kind !== "heading") return;
    const matches = available.get(row.id);
    if (matches && matches.next < matches.indices.length) reuse[i] = matches.indices[matches.next++];
  });
}

function renderReplies(w: LogWindow) {
  if (!w.replyRoot && !w.replies.size && !w.headings.size) return;
  w.replyRoot ??= createRoot(document.createElement("div"));
  flushSync(() => w.replyRoot?.render(createElement(Fragment, null,
    ...[...w.replies].map(([host, mounted]) => createPortal(createElement(AgentReply, { reply: mounted.reply }), host, mounted.key)),
    ...[...w.headings].map(([host, mounted]) => createPortal(createElement(AgentTurnHeading, { value: mounted.value }), host, mounted.key)))));
}

/// Search all held turns without mounting the entire conversation.
export function logSearchSources(log: HTMLElement) {
  const w = windows.get(log);
  return w ? w.search ??= w.units.map(u => {
    const root = document.createElement("div");
    root.innerHTML = u.rows().map(rowHtml).join("");
    return { key: u.key, root };
  }) : [];
}

/// Mounts the requested turn and scrolls it into the virtual window.
export function revealLogUnit(b: ChatBody, key: string) {
  const log = b.querySelector<HTMLElement>(".dcoderlog"), w = log && windows.get(log);
  if (!log || !w) return null;
  const at = w.units.findIndex(u => u.key === key);
  if (at < 0) return null;
  b.stuck = false;
  const d = w.drawn.get(key) ?? draw(w, w.units[at]);
  // Measure the target before filling its surrounding turns.
  for (const [k, other] of w.drawn) if (k !== key) undraw(w, k, other);
  log.insertBefore(d.el, w.bottom);
  renderReplies(w);
  measure(w);
  space(w, at, at + 1);
  b.scrollTop += d.el.getBoundingClientRect().top - b.getBoundingClientRect().top;
  fillLog(b, false);
  return w.drawn.get(key)?.el ?? null;
}
