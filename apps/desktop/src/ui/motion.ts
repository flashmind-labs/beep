// Navigation motion: one short transition per move, never ambient. A board
// card grows into the session's head and back; the next conversation slides in.

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
