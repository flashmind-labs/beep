// Navigation motion: one short transition per move, never ambient. A board
// card grows into the session's head and back; the next conversation slides in; a
// two-finger swipe carries the page back or forward. A jump glides or snaps to its
// place, and a list of cards scrolls a card at a time with inertia.

import { isTouchDevice } from "./platform.ts";

export type Move = "open" | "back" | "next";

let playing: Move | null = null;
/// The board card that shares its shape with the open session's head.
let hero: string | null = null;

export const heroCard = () => hero;
export function setHeroCard(key: string | null) { hero = key; }

/// The move playing, if any, which decides how the page under it comes in.
export const movePlaying = () => playing;

/// Whether motion may play now: not under reduced motion, nor in a hidden window.
export const motionAllowed = () =>
  !window.matchMedia("(prefers-reduced-motion: reduce)").matches && !document.hidden;

/// Runs `step`, which changes the page, as one transition. The page changes a
/// frame later, after the old one is captured; without support, or when motion
/// is off, it changes at once.
export function withMove(move: Move, step: () => void) {
  if (playing || !motionAllowed() || typeof document.startViewTransition !== "function") return step();
  const root = document.documentElement;
  const done = () => { playing = null; delete root.dataset.move; };
  playing = move;
  root.dataset.move = move;
  document.startViewTransition(step).finished.then(done, done);
}

/// The way a two-finger swipe moves through the window's pages.
export type Swipe = "back" | "forward";

/// What a swipe needs of the window: whether there is a page that way, and going to it.
interface SwipePages { has(to: Swipe): boolean; go(to: Swipe): void }

/// How far the fingers travel before a swipe goes, how far before its direction is read, and
/// how long the wheel rests between two swipes.
const SWIPE_PX = 120, AIM_PX = 12, REST_MS = 150;

let swiped: Swipe | null = null;

/// The swipe that is changing the page, if any, which decides the side the page comes in from.
export const swipePlaying = () => swiped;

/// A two-finger swipe sideways on the trackpad goes back or forward. The page follows the
/// fingers, leaves once they have gone far enough and comes back when they stop short.
/// What scrolls sideways under the pointer keeps the swipe, and so does a handler that took
/// the wheel. One swipe is one step: the wheel must rest before the next, which also drops
/// the events a trackpad keeps sending after the fingers lift.
export function trackpadSwipe(pages: SwipePages) {
  let phase: "idle" | "aim" | "follow" | "spent" = "idle";
  let x = 0, y = 0, last = 0, dx = 0;
  let scrolls: Record<Swipe, boolean> | null = null;
  let to: Swipe = "back";
  let rest: ReturnType<typeof setTimeout> | undefined;
  const main = () => document.querySelector<HTMLElement>(".cc-main");

  document.addEventListener("wheel", e => {
    if (phase !== "follow" && e.timeStamp - last > REST_MS) phase = "idle";
    last = e.timeStamp;
    if (phase === "spent") return;

    if (phase === "idle") {
      const plain = !e.defaultPrevented && e.deltaMode === WheelEvent.DOM_DELTA_PIXEL && !e.ctrlKey && !e.shiftKey && !e.metaKey && !e.altKey;
      phase = plain && !document.body.classList.contains("mobile") ? "aim" : "spent";
      x = 0;
      y = 0;
      scrolls = null;
      if (phase === "spent") return;
    }

    x += e.deltaX;
    y += e.deltaY;

    if (phase === "aim") {
      if (e.deltaX !== 0) scrolls ??= scrollsSideways(e.target);
      if (Math.hypot(x, y) < AIM_PX) return;
      to = x < 0 ? "back" : "forward";
      if (Math.abs(x) < 2 * Math.abs(y) || scrolls?.[to] || !pages.has(to)) { phase = "spent"; return; }
      phase = "follow";
    }

    const travel = Math.max(0, to === "back" ? -x : x);
    clearTimeout(rest);
    if (travel >= SWIPE_PX) return leave();
    rest = setTimeout(settle, REST_MS);
    const page = main();
    if (!page || !motionAllowed()) return;
    dx = to === "back" ? travel : -travel;
    const { transform, clipPath } = shifted(dx);
    Object.assign(page.style, { willChange: "transform", transform, clipPath });
  }, { passive: true });

  /// The fingers stopped short: the page goes back to its place.
  function settle() {
    phase = "idle";
    const page = main();
    if (!page || dx === 0) return;
    page.animate([shifted(dx), shifted(0)], { duration: 180, easing: "cubic-bezier(.2, .8, .2, 1)" });
    still(page);
  }

  /// The page slides the rest of the way out, then the next one comes in from the other side.
  function leave() {
    phase = "spent";
    const page = main();
    const step = () => {
      swiped = to;

      try {
        pages.go(to);
      } finally {
        swiped = null;
      }
    };
    if (!page || !motionAllowed()) return step();

    const out = page.animate([{ ...shifted(dx), opacity: 1 }, { ...shifted(to === "back" ? SWIPE_PX + 60 : -SWIPE_PX - 60), opacity: 0 }], { duration: 90, easing: "ease-in", fill: "forwards" });
    still(page);
    const land = () => {
      step();
      out.cancel();
      // The page coming in stays inside the main area too.
      page.style.clipPath = "inset(0)";
      const coming = document.getAnimations().filter(a => a.effect instanceof KeyframeEffect && a.effect.target?.matches(".cc-box"));
      void Promise.allSettled(coming.map(a => a.finished)).then(() => { if (phase !== "follow") page.style.clipPath = ""; });
    };
    out.finished.then(land, land);
  }

  function still(page: HTMLElement) {
    dx = 0;
    Object.assign(page.style, { willChange: "", transform: "", clipPath: "" });
  }
}

/// The page moved `dx` sideways and cut where it would cross the sidebar, whose background
/// lets it show through.
const shifted = (dx: number) => ({ transform: `translateX(${dx}px)`, clipPath: `inset(0 0 0 ${Math.max(0, -dx)}px)` });

/// Whether something from `target` up can still scroll each way, and so takes the swipe.
function scrollsSideways(target: EventTarget | null): Record<Swipe, boolean> {
  const scrolls = { back: false, forward: false };

  for (let el = target instanceof Element ? target : null; el; el = el.parentElement) {
    const room = el.scrollWidth - el.clientWidth;
    if (room <= 1) continue;
    const overflow = getComputedStyle(el).overflowX;
    if (overflow !== "auto" && overflow !== "scroll") continue;
    if (el.scrollLeft > 0) scrolls.back = true;
    if (el.scrollLeft < room - 1) scrolls.forward = true;
  }

  return scrolls;
}

/// How a jump moves: a glide to a place to read, a snap between items and sections.
type ScrollFeel = "glide" | "snap";

/// Where a jump goes: a position, or the end of the scroller, which may grow on the way.
type ScrollGoal = number | "end";

/// What a jump does once it arrives: light up `landed`, then run `done`.
interface Arrival { landed: HTMLElement | null; done: (() => void) | null }

/// A feel's duration, from a base and a little more per pixel up to a cap, how far it
/// travels before cutting the rest short, and how long its landing stays lit.
const FEELS: Record<ScrollFeel, { minMs: number; maxMs: number; msPerPx: number; farViews: number; lightMs: number }> = {
  glide: { minMs: 450, maxMs: 700, msPerPx: 0.2, farViews: 1.5, lightMs: 900 },
  snap: { minMs: 110, maxMs: 160, msPerPx: 0.04, farViews: 1, lightMs: 260 },
};

/// What moves each scroller now, so the next motion stops it first.
const moving = new WeakMap<HTMLElement, () => void>();

/// Scrolls `el` to `goal` with an exponential ease-out: fast at first, settling softly. A
/// long way jumps most of it first. The wheel, a press or a key stops it. Under reduced motion,
/// or in a hidden window, it jumps.
export function glide(el: HTMLElement, goal: ScrollGoal, feel: ScrollFeel, arrival: Arrival) {
  moving.get(el)?.();
  const spec = FEELS[feel];
  const target = () => {
    const end = Math.max(0, el.scrollHeight - el.clientHeight);
    return goal === "end" ? end : Math.min(Math.max(0, goal), end);
  };
  const arrive = () => { light(arrival.landed, spec.lightMs); arrival.done?.(); };

  if (!motionAllowed()) {
    el.scrollTop = target();
    return arrive();
  }

  let from = el.scrollTop;
  const first = target(), far = el.clientHeight * spec.farViews;
  if (Math.abs(first - from) > far) {
    from = first - Math.sign(first - from) * far;
    el.scrollTop = from;
  }

  const duration = Math.min(spec.maxMs, spec.minMs + Math.abs(first - from) * spec.msPerPx), start = performance.now();
  let frame = 0;
  const stop = () => {
    cancelAnimationFrame(frame);
    el.removeEventListener("wheel", stop);
    untilInput(stop, false);
    if (moving.get(el) === stop) moving.delete(el);
  };
  const step = (now: number) => {
    const t = Math.min(1, (now - start) / duration);
    el.scrollTop = from + (target() - from) * easeOut(t);
    if (t < 1) { frame = requestAnimationFrame(step); return; }

    stop();
    arrive();
  };

  el.addEventListener("wheel", stop, { passive: true });
  untilInput(stop, true);
  moving.set(el, stop);
  frame = requestAnimationFrame(step);
}

/// Glides `list` to put `item` where its first item sits.
export function glideTo(list: HTMLElement, item: HTMLElement, feel: ScrollFeel, arrival: Arrival) {
  const top = item.getBoundingClientRect().top - list.getBoundingClientRect().top + list.scrollTop - inset(list);
  glide(list, top, feel, arrival);
}

const easeOut = (t: number) => t >= 1 ? 1 : 1 - 2 ** (-10 * t);

/// Adds or removes `stop` for a press or a key anywhere, which takes the scroll back: a
/// click elsewhere may change the page under it.
function untilInput(stop: () => void, on: boolean) {
  for (const type of ["pointerdown", "keydown"] as const) {
    if (on) document.addEventListener(type, stop, true);
    else document.removeEventListener(type, stop, true);
  }
}

/// Lights `el` in the accent for a moment, where a jump landed.
function light(el: HTMLElement | null, ms: number) {
  el?.animate([{ backgroundColor: "color-mix(in srgb, var(--accent) 30%, transparent)" }, { backgroundColor: "transparent" }], { duration: ms, easing: "ease-out" });
}

/// The scrollers that can hold a board, and the board items they stop at. An item belongs
/// to the nearest scroller around it. A scroller showing no board item scrolls freely.
const LISTS = [".cc-view", ".pj-det", ".pj-board", ".bd-lane.cell"].join(", ");
const ITEMS = [".bd-card"].join(", ");

/// The first part of a swipe moves one item and every further stretch one more; a pause
/// longer than the gap starts a new swipe. A list that reached its end holds the next
/// swipes its way that follow within the hold. The pixels of a wheel line, then the spring
/// the list follows its item on, a touch under critical damping so it settles like a click.
const FIRST_PX = 24, MORE_PX = 140, GESTURE_GAP_MS = 140, HELD_SWIPES = 2, HOLD_MS = 700, LINE_PX = 16, SPRING = 30, DAMPING = 0.75;

/// A board scrolls a card at a time. A small scroll moves one card; a fast one moves
/// more, and the list carries its speed into the card it ends on. A swipe belongs to the
/// innermost list that can still move its way, and stays with it to the swipe's end. A
/// list that ran out of cards also keeps the next swipes that follow at once, so scrolling
/// on through it does not carry the page.
export function itemScroll() {
  let last = -Infinity, decided = false, owner: HTMLElement | null = null, heading = 0, travel = 0, steps = 0;
  let held: { list: HTMLElement; sign: number; swipes: number } | null = null;

  document.addEventListener("wheel", event => {
    if (isTouchDevice() || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
    const dy = event.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? event.deltaY : event.deltaY * LINE_PX;
    if (Math.abs(event.deltaX) > Math.abs(dy)) return;

    const gap = event.timeStamp - last;
    last = event.timeStamp;
    if (gap > GESTURE_GAP_MS) decided = false;
    if (gap > HOLD_MS) held = null;
    // A trackpad opens a swipe with an empty event; one left to the browser hands it the
    // whole swipe.
    if (dy === 0) {
      if (!decided && listAround(event.target)) event.preventDefault();
      return;
    }

    const sign = Math.sign(dy);
    if (!decided || sign !== heading) {
      decided = true;
      heading = sign;
      travel = 0;
      steps = 0;
      const keep = held && held.sign === sign && held.swipes < HELD_SWIPES && event.target instanceof Node && held.list.contains(event.target) ? held : null;
      // Only a swipe after the list has arrived counts toward the hold.
      if (keep && !springs.has(keep.list)) keep.swipes += 1;
      if (!keep) held = null;
      owner = keep?.list ?? listFor(event.target, sign);
    }
    if (!owner || !event.cancelable) return;

    event.preventDefault();
    travel += Math.abs(dy);
    const want = travel < FIRST_PX ? 0 : 1 + Math.floor((travel - FIRST_PX) / MORE_PX);
    for (; steps < want; steps += 1) stepItem(owner, heading);
    if (!held && atEnd(owner, heading)) held = { list: owner, sign: heading, swipes: 0 };
  }, { passive: false });
}

/// Whether `list` is at its end, or on its way there, `sign`'s way.
function atEnd(list: HTMLElement, sign: number) {
  const at = springs.get(list)?.target ?? list.scrollTop, room = list.scrollHeight - list.clientHeight;
  return sign > 0 ? at >= room - 1 : at <= 1;
}

/// The board the pointer is over, if any.
function listAround(target: EventTarget | null) {
  const list = target instanceof Element ? target.closest<HTMLElement>(LISTS) : null;
  return list && boardIn(list) ? list : null;
}

/// Whether scroller `list` shows a board item of its own.
function boardIn(list: HTMLElement) {
  for (const item of list.querySelectorAll<HTMLElement>(ITEMS)) if (item.closest(LISTS) === list && item.getClientRects().length) return true;

  return false;
}

/// The scroller that takes a swipe `sign`'s way: the innermost one that can still move,
/// when it shows a board. Another scroller in between keeps the swipe for itself.
function listFor(target: EventTarget | null, sign: number) {
  for (let el = target instanceof HTMLElement ? target : null; el; el = el.parentElement) {
    if (el.scrollHeight - el.clientHeight <= 1 || !/auto|scroll/.test(getComputedStyle(el).overflowY) || atEnd(el, sign)) continue;

    return el.matches(LISTS) && boardIn(el) ? el : null;
  }

  return null;
}

interface Spring { target: number; pos: number; speed: number; then: number; frame: number }
const springs = new WeakMap<HTMLElement, Spring>();

/// Moves `list`'s target one item `sign`'s way, starting its spring if it is still.
function stepItem(list: HTMLElement, sign: number) {
  const spring = springs.get(list), from = spring?.target ?? list.scrollTop, all = stops(list);
  const to = sign > 0 ? all.find(stop => stop > from + 1) : all.findLast(stop => stop < from - 1);
  if (to === undefined) return;

  if (spring) {
    spring.target = to;
    return;
  }

  moving.get(list)?.();
  if (!motionAllowed()) {
    list.scrollTop = to;
    return;
  }

  const s: Spring = { target: to, pos: list.scrollTop, speed: 0, then: performance.now(), frame: 0 };
  const stop = () => {
    cancelAnimationFrame(s.frame);
    springs.delete(list);
    untilInput(stop, false);
    if (moving.get(list) === stop) moving.delete(list);
  };
  const move = (now: number) => {
    const dt = Math.min(1 / 30, (now - s.then) / 1000);
    s.then = now;
    s.speed += (SPRING * SPRING * (s.target - s.pos) - 2 * DAMPING * SPRING * s.speed) * dt;
    s.pos += s.speed * dt;
    list.scrollTop = s.pos;
    // At an end the list cannot pass its item, so arriving there is the end of the move.
    const room = list.scrollHeight - list.clientHeight, edge = s.target <= 0 || s.target >= room - 0.5;
    const passed = (s.target - s.pos) * Math.sign(s.speed) <= 0;
    if ((Math.abs(s.target - s.pos) >= 0.5 || Math.abs(s.speed) >= 20) && !(edge && passed)) { s.frame = requestAnimationFrame(move); return; }

    list.scrollTop = s.target;
    stop();
  };

  springs.set(list, s);
  moving.set(list, stop);
  untilInput(stop, true);
  s.frame = requestAnimationFrame(move);
}

/// Where `list` can rest: each of its items where its first item sits, its ends, and in a
/// stretch taller than the list, stops most of a screen apart so the middle can be read.
function stops(list: HTMLElement) {
  const end = list.scrollHeight - list.clientHeight, view = list.clientHeight;
  const origin = list.getBoundingClientRect().top - list.scrollTop + inset(list);
  const found = [0, end];

  for (const item of list.querySelectorAll<HTMLElement>(ITEMS)) {
    if (item.closest(LISTS) !== list) continue;
    const rect = item.getBoundingClientRect();
    if (rect.height) found.push(Math.min(Math.max(rect.top - origin, 0), end));
  }

  const out: number[] = [];
  for (const stop of found.sort((a, b) => a - b)) {
    const prev = out.at(-1);
    if (prev !== undefined && stop - prev < 2) continue;
    for (let fill = (prev ?? stop) + view * 0.8; prev !== undefined && fill < stop - view * 0.2; fill += view * 0.8) out.push(fill);
    out.push(stop);
  }

  return out;
}

/// How far below `list`'s top its items rest: under a header that sticks there, with the
/// list's gap, or else at its padding.
function inset(list: HTMLElement) {
  const style = getComputedStyle(list);
  let covered = 0;

  for (const child of list.children) {
    if (!(child instanceof HTMLElement)) continue;
    const own = getComputedStyle(child);
    if (own.position === "sticky") covered = Math.max(covered, (Number.parseFloat(own.top) || 0) + child.offsetHeight + (Number.parseFloat(style.rowGap) || 0));
  }

  return covered || Number.parseFloat(style.paddingTop) || 0;
}
