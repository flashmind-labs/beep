// Which worktree of a session's repository its pane shows: the list the node
// gives, and the worktree the agent's tool calls work in, read from the text
// of the log's entries. The same for every harness.

import type { AgentToolCall } from "../agents/agents.ts";
import { parseTool } from "../agents/agentlog.ts";
import { resolvePath } from "./media-state.ts";

/// A worktree, as `agent_worktrees` lists it.
export interface Tree {
  path: string;
  branch: string | null;
  head: string;
  /// The one holding the session's folder.
  own: boolean;
  /// The session the list was asked for made it.
  created: boolean;
}

/// The parts of a log entry the matching reads.
export interface Entry {
  kind: string;
  text: string;
  tool?: AgentToolCall | null;
}

/// What a worktree is called: its branch, else its commit.
export const treeName = (t: Tree) => t.branch ?? t.head;

const bare = (dir: string) => dir.replace(/\/+$/, "") || "/";

/// The folder the pane reads for `t`: the session's own folder for the
/// worktree holding it (it may be a subfolder), else the worktree's top.
export function rootOf(t: Tree, cwd: string) {
  return t.own ? bare(cwd) : bare(t.path);
}

/// `path` is `dir` or lies under it.
function within(path: string, dir: string) {
  const d = bare(dir);
  return d !== "/" && (path === d || path.startsWith(`${d}/`));
}

/// The worktree holding the absolute `path`; the longest match wins, so a
/// worktree inside the main checkout is picked over it. Null for a path
/// in none of them.
export function treeAt(trees: readonly Tree[], cwd: string, path: string) {
  let found: Tree | null = null;
  let length = 0;

  for (const t of trees) {
    for (const dir of t.own ? [t.path, cwd] : [t.path]) {
      if (within(path, dir) && bare(dir).length > length) { found = t; length = bare(dir).length; }
    }
  }

  return found;
}

/// The folder the pane reads for the checkout at `folder`, which the session's
/// machine says it works in: its worktree in `trees` (see `rootOf`). With no
/// list, the folder as it is. Null for a folder the list lacks, which was
/// removed or is too new to be listed: the report asks for the list again.
export function reportedRoot(trees: readonly Tree[], cwd: string, folder: string) {
  const t = trees.find(t => bare(t.path) === bare(folder));
  if (t) return rootOf(t, cwd);
  return trees.length ? null : bare(folder);
}

export function matchesTree(tree: Tree, query: string) {
  return `${treeName(tree)} ${tree.path}`.toLowerCase().includes(query.trim().toLowerCase());
}

/// Decode structured arguments, never JavaScript source.
function workingDirectory(entry: Entry): string | null {
  if (!entry.tool?.input || entry.tool.input_truncated) return null;
  try {
    const args: unknown = JSON.parse(entry.tool.input);
    if (!args || typeof args !== "object") return null;
    // A Beep tool naming a machine works in that machine's folder.
    if ("machine" in args && Reflect.get(args, "machine")) return null;
    for (const key of ["workdir", "cwd", "working_directory"]) {
      if (key in args) {
        const value: unknown = Reflect.get(args, key);
        if (typeof value === "string" && value) return value;
      }
    }
  } catch { /* Older harnesses may give plain text. */ }
  return null;
}

/// Words of a command or title, with quotes, `&&`, `;` and the like cut away.
const WORD = /[^\s"'`;|&()<>=,]+/g;
/// What names a folder in the word after it.
const DIR_AFTER = new Set(["cd", "pushd", "-C", "--cwd", "--directory", "--work-tree", "--prefix"]);
const DIFF = /^\[diff\] (\S.*?) \+\d+ -\d+$/;
const NESTED = /^(.*?\/(?:\.worktrees|\.claude\/worktrees)\/[^/]+)/;
const WORKTREE_WORD = /\bworktree\b/i;
const MOVES = /^(Enter|Exit)Worktree\b/;
const FLASHMIND_WORKTREE = /^worktree(?:: Worktree (?:create|switch|list|status))?$/;

/// Flashmind reports the active worktree as one record or as part of a list.
function currentWorktree(output: string): string | null {
  try {
    const result: unknown = JSON.parse(output);
    const rows: unknown[] = Array.isArray(result) ? result : [result];
    const paths = rows.flatMap(row => {
      if (!row || typeof row !== "object" || !("current" in row) || row.current !== true) return [];
      if (!("available" in row) || row.available !== true || !("bare" in row) || row.bare !== false) return [];
      if (!("path" in row) || typeof row.path !== "string" || !row.path.startsWith("/")) return [];
      return [bare(row.path)];
    });
    return paths.length === 1 ? paths[0] : null;
  } catch {
    return null;
  }
}

/// The places a tool entry's text names, resolved against `base` (relative
/// paths) and `cwd` (the paths of diffs, which the node cuts the session's
/// folder from), in the order they appear. `exit` is an ExitWorktree call
/// that names none.
function places({ title, output }: ReturnType<typeof parseTool>, base: string, cwd: string, directoriesOnly = false) {
  const found: string[] = [];
  const moves = MOVES.exec(title)?.[1];
  const scan = (words: string) => {
    let before = "";

    for (const word of words.match(WORD) ?? []) {
      if (directoriesOnly && !DIR_AFTER.has(before)) { before = word; continue; }
      if (word.startsWith("/")) found.push(resolvePath(base, word));
      else if (DIR_AFTER.has(before) && !word.startsWith("-") && !word.startsWith("~")) found.push(resolvePath(base, word));
      else if (!word.startsWith("-") && !word.startsWith("~") && !word.includes("://") && word.includes("/")) found.push(resolvePath(base, word));
      before = word;
    }
  };
  scan(title);

  for (const line of output.split("\n")) {
    const diff = DIFF.exec(line);
    if (diff && !directoriesOnly) found.push(resolvePath(cwd, diff[1]));
    // What Enter and ExitWorktree report is the worktree they went to.
    else if (moves) scan(line);
  }

  return { found, exit: moves === "Exit" && !found.length };
}

/// What reading some entries found.
export interface Followed {
  /// Where the agent works after them: the folder of its worktree (see
  /// `rootOf`), or `last` when they name none.
  last: string;
  /// They named a path in a worktree.
  hit: boolean;
  /// How many entries from the first are final, and where the agent was
  /// after them: an entry still running is read again when it ends.
  done: number;
  kept: string;
  /// What the entries show the list may not know: a `git worktree` command
  /// or a `.worktrees` folder it lacks.
  unknown: string[];
}

/// Follows the agent through `entries`, from `last`, the folder of the
/// worktree it was in (the session's folder to begin with). A relative
/// path is taken from `last`; a path in none of `trees` changes nothing.
export function follow(entries: readonly Entry[], trees: readonly Tree[], cwd: string, last: string): Followed {
  const out: Followed = { last, hit: false, done: 0, kept: last, unknown: [] };
  let settled = true;
  entries.forEach((en, i) => {
    const parsed = parseTool(en.kind === "tool" ? en.text : "");
    const { title, status, output } = parsed;
    const running = status === "pending" || status === "in_progress";

    if (en.kind === "tool" && status !== "failed") {
      if (WORKTREE_WORD.test(title)) out.unknown.push(title.split("\n")[0].slice(0, 200));
      if (FLASHMIND_WORKTREE.test(title)) {
        const current = status === "completed" ? currentWorktree(output) : null;
        if (current) {
          // Match a listed root, even when the agent returned to its launch checkout.
          const tree = trees.find(t => bare(t.path) === current);
          if (tree) { out.last = rootOf(tree, cwd); out.hit = true; }
          else out.unknown.push(current);
        }
      } else if (trees.length) {
        // Creating a worktree alone is not a move. A directory in the same
        // command still confirms usage, including `worktree add ... && cd ...`.
        const creating = WORKTREE_WORD.test(title) && !MOVES.test(title);
        const { found, exit } = places(parsed, out.last, cwd, creating);
        const directory = workingDirectory(en);
        if (directory) found.push(resolvePath(cwd, directory));
        const editing = /^(?:Edit|Write|Create|Delete|Move|apply_patch)\b/i.test(title)
          || output.split("\n").some(line => DIFF.test(line));
        if (exit) { out.last = bare(cwd); out.hit = true; }

        for (const path of found) {
          const nested = NESTED.exec(path)?.[1];
          if (nested && !trees.some(t => bare(t.path) === nested)) out.unknown.push(nested);
          const t = treeAt(trees, cwd, path);
          // Reading in the launch checkout must not hide the active worktree.
          if (t && trees.length > 1 && (!t.own || editing || out.last === bare(cwd))) {
            out.last = rootOf(t, cwd); out.hit = true;
          }
        }
      }
    }

    if (running) settled = false;
    if (settled) { out.done = i + 1; out.kept = out.last; }
  });
  return out;
}

/// The same for the steps of a subagent, as titles.
export function followSteps(titles: readonly string[], trees: readonly Tree[], cwd: string) {
  return follow(titles.map(text => ({ kind: "tool", text })), trees, cwd, bare(cwd));
}

/// `rel`, a path in the worktree at `root`, as a message names it: as it
/// is when `root` is the session's folder, else in full.
export function mentioned(root: string, cwd: string, rel: string) {
  return bare(root) === bare(cwd) ? rel : resolvePath(root, rel);
}

/// The heading of a review of the changes in `folder` (as the user reads it,
/// `~` for home), a linked worktree or a plain checkout.
export function reviewHeading(folder: string, worktree: boolean) {
  return worktree ? `Review on worktree: ${folder}` : `Review on ${folder}`;
}

/// Where Beep and Claude Code make worktrees, under a checkout.
const MADE_UNDER = /^(?:\.worktrees|\.claude\/worktrees)\/[^/]+(?:\/|$)/;

/// The main checkout's path when the session's folder `cwd` is gone: the
/// node then lists the checkout as the one holding the folder (`own`),
/// though `cwd` is not in its tree, or is in a folder where worktrees are
/// made, which no listed worktree holds. Null otherwise, a live worktree
/// included.
export function removedCheckout(trees: readonly Tree[], cwd: string) {
  const own = trees.find(t => t.own);
  if (!own) return null;
  const top = bare(own.path);
  if (!within(bare(cwd), top)) return top;
  return MADE_UNDER.test(bare(cwd).slice(top.length + 1)) ? top : null;
}
