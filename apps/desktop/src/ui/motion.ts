// Navigation motion: one short transition per move, never ambient. A board
// card grows into the session's head and back; the next conversation slides in; a
// two-finger swipe carries the page back or forward.

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
