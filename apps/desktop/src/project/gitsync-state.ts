// What a project page says about each checkout's branch and what Pull and
// Push may do there: pure, so it is tested without the window.

/// One checkout's branch and its distance from the upstream (GitRow).
export interface GitRow {
  machine_id: string;
  /// Null on a detached HEAD.
  branch: string | null;
  /// Null before the first push.
  upstream: string | null;
  ahead: number;
  behind: number;
  /// Why the checkout could not be read.
  error: string | null;
}

export type GitMove = "pull" | "push";

/// How a row reads: the words after the branch, which buttons work, and
/// whether there are buttons at all.
export interface GitLine {
  branch: string | null;
  text: string;
  buttons: boolean;
  pull: boolean;
  push: boolean;
  error: boolean;
}

/// "2 ahead, 1 behind", "not pushed", "up to date".
export function distance(row: Pick<GitRow, "upstream" | "ahead" | "behind">): string {
  if (!row.upstream) return "not pushed";
  const parts = [row.ahead ? `${row.ahead} ahead` : "", row.behind ? `${row.behind} behind` : ""].filter(Boolean);
  return parts.length ? parts.join(", ") : "up to date";
}

/// The line for one checkout. A button with nothing to do is off; with no
/// upstream Push stays on, as the first push sets it.
export function gitLine(row: GitRow): GitLine {
  if (row.error) return { branch: null, text: row.error, buttons: false, pull: false, push: false, error: true };
  if (!row.branch) return { branch: null, text: "Detached HEAD, nothing to pull or push", buttons: false, pull: false, push: false, error: false };
  return {
    branch: row.branch,
    text: distance(row),
    buttons: true,
    pull: !!row.upstream && row.behind > 0,
    push: !row.upstream || row.ahead > 0,
    error: false,
  };
}

/// Whether a refused pull is one an agent can settle: the branches diverged
/// or git reports a conflict, rather than a sign-in or a timeout.
export function agentCanFix(why: string): boolean {
  return /fast-forward|diverg|conflict|would be overwritten|unmerged/i.test(why);
}

/// What the start form is filled with to bring in the upstream branch.
export function fixPrompt(branch: string | null, upstream: string | null): string {
  const from = upstream ?? "its upstream branch";
  return `Bring ${from} into ${branch ?? "this branch"}, merge it, resolve any conflicts and keep both sides' intent. Do not force push.`;
}

/// What a button says while its move runs.
export function movingLabel(move: GitMove): string {
  return move === "pull" ? "Pulling…" : "Pushing…";
}

/// The key a running or failed move is kept under.
export function moveKey(project: string, machine: string): string {
  return `${project}\n${machine}`;
}

/// `rows` with `row` in place of the one for its machine, or added.
export function withRow(rows: readonly GitRow[] | undefined, row: GitRow): GitRow[] {
  const rest = (rows ?? []).filter(r => r.machine_id !== row.machine_id);
  return [...rest, row];
}
