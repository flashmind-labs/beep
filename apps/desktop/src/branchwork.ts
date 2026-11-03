// Branches with work outside the review screen: the machines a branch can get
// a worktree on, what the import sheet asks for, and the open PRs and issues
// in repos without a project.
import type { GitItemRow } from "./openitems.ts";

/// A checkout of the repository found on a machine (CheckoutRow).
export interface CheckoutRow {
  machine_id: string;
  machine_name: string;
  folder: string;
  remote: string;
}

/// A branch with work, as an import preview lists it (WorkBranchRow).
export interface WorkBranchRow {
  name: string;
  pr: number | null;
  pr_title: string;
  ahead: number;
  worktree: string | null;
  in_checkout: boolean;
  /// In milliseconds since the Unix epoch.
  committed_at: number | null;
}

/// What `preview_import` gives (PreviewView).
export interface PreviewView {
  project: string;
  checkouts: CheckoutRow[];
  default_branch: string;
  branches: WorkBranchRow[];
  note: string;
}

/// Where each branch gets a worktree, by name: a machine id, `""` for
/// where the project lands; absent or null to skip.
export type BranchPicks = Record<string, string | null>;

/// A machine a worktree can go on.
export interface Place {
  id: string;
  name: string;
}

/// A branch the helpers read.
type Branch = Pick<WorkBranchRow, "name" | "pr" | "pr_title" | "ahead" | "worktree" | "in_checkout">;

/// A preview as the helpers read it.
interface Preview {
  default_branch?: string;
  checkouts?: readonly Pick<CheckoutRow, "machine_id" | "machine_name">[];
  branches?: readonly Branch[];
}

/// One repo's open items outside the projects.
export interface OutsideRow<T> {
  repo: string;
  host: string;
  last: number;
  items: T[];
}

/// The online machines a worktree can go on: the one the branches were
/// read on first, then the rest by name.
export function worktreeMachines(machines: readonly (Place & { online: boolean })[] | null | undefined, readOn: string | null | undefined) {
  const online: Place[] = (machines || []).filter(m => m.online).map(m => ({ id: m.id, name: m.name }));
  online.sort((a, b) => (Number(b.id === readOn) - Number(a.id === readOn)) || a.name.localeCompare(b.name));
  return online;
}

/// ".worktrees/fix-it" from "fix/it", as the server names a worktree.
export function worktreeName(branch: string) {
  return `.worktrees/${String(branch).replace(/\//g, "-")}`;
}

/// Where each preview branch goes by default, by name: the first checkout's
/// machine for a branch with a pull request or commits ahead and no worktree
/// yet; nothing (Skip) for the rest.
export function defaultPlaces(preview: Preview | null | undefined) {
  const out: BranchPicks = {};
  if (!preview) return out;
  const home = preview.checkouts?.[0]?.machine_id ?? "";
  for (const b of preview.branches || []) {
    if (b.name === preview.default_branch || b.worktree) continue;
    if (b.pr != null || b.ahead > 0) out[b.name] = home;
  }
  return out;
}

/// The machines a preview branch can get its worktree on: those with a
/// checkout, or the one it is imported on when none has one.
export function placeChoices(preview: Preview | null | undefined, machines: readonly Place[] | null | undefined) {
  const names = new Map((machines || []).map(m => [m.id, m.name]));
  const seen = new Set<string>();
  const out: Place[] = [];
  for (const c of preview?.checkouts || []) {
    if (seen.has(c.machine_id)) continue;
    seen.add(c.machine_id);
    out.push({ id: c.machine_id, name: c.machine_name || names.get(c.machine_id) || c.machine_id });
  }
  if (!out.length) out.push({ id: "", name: "Where it is imported" });
  return out;
}

/// What `import_project` gets as `worktrees` from the picks: one per
/// branch with a machine, in the preview's order.
export function worktreesToAsk(preview: Preview | null | undefined, picks: BranchPicks | null | undefined) {
  return (preview?.branches || [])
    .filter(b => picks && Object.prototype.hasOwnProperty.call(picks, b.name) && picks[b.name] != null)
    .map(b => ({ branch: b.name, machine_id: picks![b.name] || null }));
}

/// One preview branch's line: its PR or how far ahead, and where it lives.
export function previewLine(b: Branch, defaultBranch: string) {
  const parts = [];
  if (b.name === defaultBranch) parts.push("default branch");
  else if (b.pr != null) parts.push(`PR #${b.pr}${b.pr_title ? ` ${b.pr_title}` : ""}`);
  if (b.ahead > 0 && b.name !== defaultBranch) parts.push(`${b.ahead} ahead`);
  if (!b.in_checkout) parts.push("not in the checkout");
  if (b.worktree) parts.push(b.worktree);
  return parts.join(" · ");
}

/// The open PRs and issues in repos no project has, newest first, grouped
/// by repo. `projects` are the project rows, by `name` (`org/repo`).
export function outsideThreads<T extends Pick<GitItemRow, "host" | "repo" | "updated_at">>(
  view: { items?: readonly T[] } | null | undefined,
  projects: readonly { name: string }[] | null | undefined,
) {
  const known = new Set((projects || []).map(p => String(p.name).toLowerCase()));
  const rows = new Map<string, OutsideRow<T>>();
  for (const item of view?.items || []) {
    if (known.has(item.repo.toLowerCase())) continue;
    const key = `${item.host} ${item.repo}`;
    let row = rows.get(key);
    if (!row) rows.set(key, row = { repo: item.repo, host: item.host, last: 0, items: [] });
    row.items.push(item);
    row.last = Math.max(row.last, item.updated_at);
  }
  for (const row of rows.values()) row.items.sort((a, b) => b.updated_at - a.updated_at);
  return [...rows.values()].sort((a, b) => b.last - a.last);
}

/// What the import sheet says it found: the checkouts and through which
/// remote, or that there is none yet.
export function checkoutNote(preview: { checkouts?: readonly Omit<CheckoutRow, "machine_id">[] } | null | undefined) {
  const found = preview?.checkouts || [];
  if (!found.length) return "No checkout on your online machines yet; importing clones it.";
  const one = (c: Omit<CheckoutRow, "machine_id">) => `${c.machine_name} at ${c.folder}${c.remote === "origin" ? "" : ` (through ${c.remote})`}`;
  const more = found.length > 1 ? ` and ${found.length - 1} more` : "";
  return `Found a checkout on ${one(found[0])}${more}.`;
}

/// What `find_import_fork` gives.
export interface ImportFork {
  project: string;
  /// `login/repo`, or null when the repository itself is cloned.
  fork: string | null;
  branches: string[];
}

/// The line saying the fork is cloned rather than the repository, or null.
export function forkNote(found: ImportFork | null | undefined): string | null {
  if (!found?.fork) return null;
  const n = found.branches.length;
  const branches = n === 0 ? "" : n === 1 ? ` Your PR branch ${found.branches[0]} is made local.` : ` Your ${n} PR branches are made local.`;
  return `Your fork ${found.fork} will be cloned, upstream added.${branches}`;
}
