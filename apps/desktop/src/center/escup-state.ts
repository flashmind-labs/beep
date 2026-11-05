// Esc up the window's pages: a project's open tile, then the Projects page,
// then Home. A session goes to its project first (agents.ts).

import type { ViewName } from "./state.ts";

export interface EscAt {
  /// The page showing.
  page: ViewName | null;
  /// The project the Projects page shows; null for the list.
  picked: string | null;
  /// A tile of the project's list is open.
  tile: boolean;
  /// A project's own window, which has no Projects list or Home above it.
  alone: boolean;
}

export type EscUp = { to: "tile" } | { to: "projects"; project: null } | { to: "home" } | null;

/// Where Esc goes from `at`; null for nowhere.
export function escUp(at: EscAt): EscUp {
  if (at.page !== "projects") return null;
  if (at.tile) return { to: "tile" };
  if (at.alone) return null;
  return at.picked ? { to: "projects", project: null } : { to: "home" };
}
