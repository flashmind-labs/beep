// What a redraw does with each child of a list that was drawn before.

/// What to do with child `i` of the new list: keep the node on screen, replace it, or add it.
/// Children on screen past the new list are removed. `busy` tells that the node on screen has a
/// request in flight, so it stays until that answers.
export type Step = "keep" | "replace" | "add";

export function patchSteps(drawn: readonly string[], next: readonly string[], busy: (i: number) => boolean): Step[] {
  return next.map((sig, i) => {
    if (i >= drawn.length) return "add";
    if (drawn[i] === sig) return "keep";
    return busy(i) ? "keep" : "replace";
  });
}

/// The signatures to remember after those steps: a kept node that was left stale keeps the
/// signature it was drawn with, so the next redraw tries again.
export function patchedSigs(drawn: readonly string[], next: readonly string[], steps: readonly Step[]) {
  return next.map((sig, i) => (steps[i] === "keep" ? drawn[i] : sig));
}
