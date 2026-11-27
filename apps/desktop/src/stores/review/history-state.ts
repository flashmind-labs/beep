import type { SessionRow } from "../project/projects-state.ts";
import type { DiffStatus } from "./diff-state.ts";

export interface BranchSessionRow extends SessionRow { worktree?: string }

export interface GitCommit {
  id: string;
  parents: string[];
  author: string;
  committed_at: number;
  subject: string;
}
interface GitRef { name: string; head: string; kind: "local" | "remote" | "tag" }
export interface GitWorktree { path: string; head: string | null; branch: string | null; main: boolean }
export interface HistoryPage {
  machine_id: string;
  machine_name: string;
  root: string;
  refs: GitRef[];
  worktrees: GitWorktree[];
  commits: GitCommit[];
  next: number | null;
}
export interface ChangedFile { path: string; old_path: string | null; status: DiffStatus }
export type GitSelection = { kind: "commit"; commit: GitCommit; origin: string } | { kind: "working"; worktree: GitWorktree };

export function appendCommits(before: readonly GitCommit[], page: readonly GitCommit[]) {
  const ids = new Set(before.map(c => c.id));
  return [...before, ...page.filter(c => { if (ids.has(c.id)) return false; ids.add(c.id); return true; })];
}

export function selectionKey(selection: GitSelection) {
  return selection.kind === "commit" ? `commit:${selection.commit.id}` : `working:${selection.worktree.path}`;
}

export function dayGroups(commits: readonly GitCommit[]) {
  const groups = new Map<string, GitCommit[]>();

  for (const c of commits) {
    const day = new Date(c.committed_at).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
    const list = groups.get(day) || [];
    list.push(c);
    groups.set(day, list);
  }

  return [...groups];
}

/// A rename needs both paths to keep rename detection when Git filters a patch.
export function diffPaths(files: readonly ChangedFile[], paths: readonly string[]) {
  return [...new Set(paths.flatMap(path => {
    const file = files.find(f => f.path === path);
    return file?.old_path ? [file.old_path, path] : [path];
  }))];
}
