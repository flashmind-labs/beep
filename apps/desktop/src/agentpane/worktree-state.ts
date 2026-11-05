// Which worktree of a session's repository its pane shows: the list the node
// gives, and the worktree the agent's tool calls work in, read from the text
// of the log's entries. The same for every harness.

import { parseTool } from "../agents/agentlog.ts";
import { resolvePath } from "./media-state.ts";

/// A worktree, as `agent_worktrees` lists it.
export interface Tree {
  path: string;
  branch: string | null;
  head: string;
  /// The one holding the session's folder.
  own: boolean;
}

/// The parts of a log entry the matching reads.
export interface Entry {
  kind: string;
  text: string;
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

/// Words of a command or title, with quotes, `&&`, `;` and the like cut away.
const WORD = /[^\s"'`;|&()<>=,]+/g;
/// What names a folder in the word after it.
const DIR_AFTER = new Set(["cd", "pushd", "-C", "--cwd", "--directory", "--work-tree", "--prefix"]);
const DIFF = /^\[diff\] (\S.*?) \+\d+ -\d+$/;
const NESTED = /^(.*?\/\.worktrees\/[^/]+)/;
const WORKTREE_WORD = /\bworktree\b/i;
const MOVES = /^(Enter|Exit)Worktree\b/;

/// The places a tool entry's text names, resolved against `base` (relative
/// paths) and `cwd` (the paths of diffs, which the node cuts the session's
/// folder from), in the order they appear. `exit` is an ExitWorktree call
/// that names none.
function places(text: string, base: string, cwd: string) {
  const { title, output } = parseTool(text);
  const found: string[] = [];
  const moves = MOVES.exec(title)?.[1];
  const scan = (words: string) => {
    let before = "";
    for (const word of words.match(WORD) ?? []) {
      if (word.startsWith("/")) found.push(resolvePath(base, word));
      else if (DIR_AFTER.has(before) && !word.startsWith("-") && !word.startsWith("~")) found.push(resolvePath(base, word));
      else if (!word.startsWith("-") && !word.startsWith("~") && !word.includes("://") && word.includes("/")) found.push(resolvePath(base, word));
      before = word;
    }
  };
  scan(title);
  for (const line of output.split("\n")) {
    const diff = DIFF.exec(line);
    if (diff) found.push(resolvePath(cwd, diff[1]));
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
    const { title, status } = parseTool(en.kind === "tool" ? en.text : "");
    const running = status === "pending" || status === "in_progress";
    if (en.kind === "tool" && status !== "failed") {
      if (WORKTREE_WORD.test(title)) out.unknown.push(title.split("\n")[0].slice(0, 200));
      if (trees.length > 1) {
        const { found, exit } = places(en.text, out.last, cwd);
        if (exit) { out.last = bare(cwd); out.hit = true; }
        for (const path of found) {
          const nested = NESTED.exec(path)?.[1];
          if (nested && !trees.some(t => bare(t.path) === nested)) out.unknown.push(nested);
          const t = treeAt(trees, cwd, path);
          if (t) { out.last = rootOf(t, cwd); out.hit = true; }
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

/// The first line of a review, led by the worktree it is of when that is
/// not the session's folder.
export function reviewIn(text: string, root: string, cwd: string) {
  return bare(root) === bare(cwd) ? text : `In ${root}: ${text}`;
}
