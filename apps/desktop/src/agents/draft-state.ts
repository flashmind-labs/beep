// The empty agent view's choices, as plain data.
import type { CheckoutPick } from "./agentpick.ts";

/// The folder a terminal opened from the empty view starts in: the worktree
/// picked in the start input, else the project's checkout on that machine.
export function draftCwd(checkout: string, pick: CheckoutPick) {
  return typeof pick === "object" ? pick.worktree : checkout;
}

/// Esc in the empty view's input: with text in it nothing happens, so the
/// prompt is not lost to a stray key; empty, it goes back to the project.
export function draftEsc(typed: string): "stay" | "project" {
  return typed.trim() ? "stay" : "project";
}
