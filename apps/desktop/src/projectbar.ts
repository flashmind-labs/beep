// The bar under a project page, as plain data: it starts a coding agent in
// the project's checkout on a machine, by default in the checkout as it is.
// center.ts draws it.

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
  return ready ? `Describe a task for a coding agent in ${name}` : `Pick a machine and folder for ${name}`;
}

/// The Folder pill: what the start works on, and what it starts from.
export function barCheckout(pick: CheckoutPick, base: string | null): { label: string; sub: string } {
  const from = `from ${base || "HEAD"}`;
  if (pick === "new") return { label: "New worktree", sub: from };
  if (pick === "branch") return { label: "New branch", sub: from };
  if (pick === "current") return { label: "Current checkout", sub: "as it is" };
  return { label: pick.worktree.split("/").pop() || pick.worktree, sub: "worktree" };
}

/// The line under the bar.

/// What Work on it, Continue or Fix this put in the bar: a new worktree on
/// `branch` (editable) from `base` for an issue, a pull request's own
/// branch, or the branch a failed workflow run ran on.
export type BarTarget =
  | { kind: "issue"; number: number; branch: string; base: string | null }
  | { kind: "pr"; number: number; branch: string }
  | { kind: "run"; number: number; branch: string };

/// What `agent_start` sends as `checkout` for a target.
export function targetPlan(t: BarTarget) {
  if (t.kind === "issue") return { named_worktree: { base: t.base?.trim() || null, branch: t.branch.trim() } };
  return { branch_worktree: { branch: t.branch } };
}

/// The Folder pill for a target.
export function targetCheckout(t: BarTarget): { label: string; sub: string } {
  if (t.kind === "issue") return { label: "New worktree", sub: `from ${t.base || "HEAD"}` };
  return { label: `Worktree ${t.branch}`, sub: t.kind === "run" ? `run #${t.number}` : `PR #${t.number}` };
}

/// Where a target's worktree is on a machine whose checkout is `cwd`: where
/// the node puts it (`.worktrees/` and the branch with each `/` as `-`),
/// and `known`, where the branch list found it.
export function targetFolders(t: Pick<BarTarget, "branch">, cwd: string, known?: string | null) {
  const trim = (p: string) => p.replace(/\/+$/, "");
  const out = [`${trim(cwd)}/.worktrees/${t.branch.trim().replace(/\//g, "-")}`];
  if (known && !out.includes(trim(known))) out.push(trim(known));
  return out;
}

/// The session fields `sessionIn` reads.
export interface BarSession {
  machine_id: string;
  cwd: string;
  external: boolean;
  last_activity: number;
}

/// The newest of our sessions (not one started outside Beep) on `machine`
/// in one of `folders` or below it, or null.
export function sessionIn<T extends BarSession>(sessions: readonly T[], machine: string, folders: readonly string[]): T | null {
  const inside = (cwd: string) => folders.some(f => cwd === f || cwd.startsWith(f + "/"));
  const found = sessions.filter(s => s.machine_id === machine && !s.external && inside(s.cwd.replace(/\/+$/, "")));
  return [...found].sort((a, b) => b.last_activity - a.last_activity)[0] ?? null;
}
