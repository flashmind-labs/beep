// The bar under a project page, as plain data: it starts a coding agent in
// the project's checkout on a machine, by default on a new branch in a new
// worktree. center.ts draws it.

import type { CheckoutPick } from "./bar/agentpick.ts";

/// A machine that can run a coding agent, the fields the bar reads.
export interface BarHost {
  machine_id: string;
  machine_name: string;
  agents: readonly string[];
}

/// Where a start from the bar runs: the machine and the project's folder
/// there; `auto` when the bar picked the machine itself.
export interface BarPlace {
  machine_id: string;
  machine_name: string;
  cwd: string;
  auto: boolean;
}

/// Where the bar starts: on `machine` (a machine id, or "auto") at the
/// project's checkout there. Auto takes the first machine by name that has
/// both a checkout and a coding agent. Null when there is none.
export function barPlace(places: readonly { machine: string; cwd: string }[], hosts: readonly BarHost[], machine: string): BarPlace | null {
  const able = hosts.filter(h => h.agents.length && places.some(p => p.machine === h.machine_id));
  const host = machine === "auto"
    ? [...able].sort((a, b) => a.machine_name.localeCompare(b.machine_name))[0]
    : able.find(h => h.machine_id === machine);
  const cwd = host && places.find(p => p.machine === host.machine_id)?.cwd;
  if (!host || !cwd) return null;
  return { machine_id: host.machine_id, machine_name: host.machine_name, cwd, auto: machine === "auto" };
}

/// What the bar's field asks for: a task, or a machine and folder first
/// when the project has no checkout an agent can run in.
export function barPrompt(name: string, ready: boolean) {
  return ready ? `Describe a task for a coding agent in ${name}` : `Pick a machine and folder for ${name}, then describe the task`;
}

/// The Folder pill: what the start works on, and what it starts from.
export function barCheckout(pick: CheckoutPick, base: string | null): { label: string; sub: string } {
  const from = `from ${base || "HEAD"}`;
  if (pick === "new" || pick === "auto") return { label: "New worktree", sub: from };
  if (pick === "branch") return { label: "New branch", sub: from };
  if (pick === "current") return { label: "Current checkout", sub: "as it is" };
  return { label: pick.worktree.split("/").pop() || pick.worktree, sub: "worktree" };
}

/// The line under the bar.
export const BAR_NOTE = "The agent works on its own branch in a new worktree unless you pick the checkout. Its session opens here.";
