// The diff viewer's data and the rules it draws by, apart from the DOM:
// which files start folded, a hunk's rows with their line numbers, the
// filter, and which file j and k go to.

/// A line as `work_diff` gives it: `k` is `+`, `-` or ` `, `e` set when
/// the file ends after it without a newline.
export interface DiffLine {
  k: "+" | "-" | " ";
  t: string;
  e?: boolean;
}

export interface DiffHunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  section: string;
  lines: DiffLine[];
}

export type DiffStatus = "added" | "modified" | "deleted" | "renamed" | "binary";
export type LeftOut = "too_large" | "lockfile" | "over_total";

export interface DiffFile {
  path: string;
  old_path: string | null;
  status: DiffStatus;
  additions: number;
  deletions: number;
  hunks: DiffHunk[];
  left_out: LeftOut | null;
}

/// What `work_diff` and `branch_diff` give (DiffView).
export interface DiffView {
  root: string;
  base: string;
  merge_base: string;
  working_tree: boolean;
  files: DiffFile[];
  additions: number;
  deletions: number;
  more: number;
}

/// Files with more changed lines than this start folded.
export const FOLD_LINES = 400;

/// Whether `f` starts folded: a deleted file, one past [`FOLD_LINES`]
/// changed lines, and a lockfile left out.
export function startsFolded(f: Pick<DiffFile, "status" | "additions" | "deletions" | "left_out">) {
  return f.status === "deleted" || f.additions + f.deletions > FOLD_LINES || f.left_out === "lockfile";
}

/// Whether `f` is folded now: as the user left it while the view is open,
/// else as it starts.
export function isFolded(f: DiffFile, kept: ReadonlyMap<string, boolean>) {
  return kept.get(f.path) ?? startsFolded(f);
}

export const STATUS_LETTER: Record<DiffStatus, string> = { added: "A", modified: "M", deleted: "D", renamed: "R", binary: "B" };

/// One row of a file's body.
export type DiffRow =
  | { kind: "hunk"; text: string }
  | { kind: "gap"; lines: number }
  | { kind: "line"; k: DiffLine["k"]; old: number | null; new: number | null; text: string; eol: boolean };

export function hunkHeader(h: Pick<DiffHunk, "old_start" | "old_lines" | "new_start" | "new_lines" | "section">) {
  const side = (start: number, n: number) => (n === 1 ? `${start}` : `${start},${n}`);
  return `@@ -${side(h.old_start, h.old_lines)} +${side(h.new_start, h.new_lines)} @@${h.section ? ` ${h.section}` : ""}`;
}

/// `f`'s rows: each hunk's header, then its lines numbered on both sides,
/// with how many unchanged lines lie between two hunks.
export function diffRows(f: Pick<DiffFile, "hunks">): DiffRow[] {
  const out: DiffRow[] = [];
  let lastOld: number | null = null;
  for (const h of f.hunks) {
    // A side with no lines names the line before it, so the first one
    // after is the start, not the one past it.
    let old = h.old_lines ? h.old_start : h.old_start + 1;
    let now = h.new_lines ? h.new_start : h.new_start + 1;
    if (lastOld != null && old - lastOld > 1) out.push({ kind: "gap", lines: old - lastOld - 1 });
    out.push({ kind: "hunk", text: hunkHeader(h) });
    for (const l of h.lines) {
      const row: DiffRow = { kind: "line", k: l.k, old: l.k === "+" ? null : old, new: l.k === "-" ? null : now, text: l.t, eol: !!l.e };
      if (l.k !== "+") old++;
      if (l.k !== "-") now++;
      out.push(row);
    }
    lastOld = old - 1;
  }
  return out;
}

/// How many digits the widest line number of `f` has.
export function numberWidth(f: Pick<DiffFile, "hunks">) {
  let max = 1;
  for (const h of f.hunks) max = Math.max(max, h.old_start + h.old_lines, h.new_start + h.new_lines);
  return String(max).length;
}

/// Whether `f` matches the filter `q`: its path or old path holds it, any case.
export function matchesFilter(f: Pick<DiffFile, "path" | "old_path">, q: string) {
  const want = q.trim().toLowerCase();
  if (!want) return true;
  return f.path.toLowerCase().includes(want) || !!f.old_path?.toLowerCase().includes(want);
}

const count = (n: number, one: string, many: string) => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

/// "6 files changed", or "No changes".
export function changedLabel(v: Pick<DiffView, "files" | "more">) {
  const n = v.files.length + v.more;
  return n ? `${count(n, "file", "files")} changed` : "No changes";
}

/// What a file without its lines says, and the button that loads them.
export function leftOutNote(f: Pick<DiffFile, "status" | "left_out" | "additions" | "deletions" | "hunks">): { text: string; load: string | null } | null {
  const lines = count(f.additions + f.deletions, "changed line", "changed lines");
  if (f.left_out === "lockfile") return { text: `Lockfile, ${lines}.`, load: "Show" };
  if (f.left_out === "too_large") return { text: `Too large to show: ${lines}.`, load: "Load anyway" };
  if (f.left_out === "over_total") return { text: `Left out: the files before it fill the view. ${lines}.`, load: "Load" };
  if (f.status === "binary") return { text: "Binary file, not shown.", load: null };
  if (!f.hunks.length) return { text: f.status === "renamed" ? "Moved without changes." : "No line changes, only its mode.", load: null };
  return null;
}

/// `v` with `loaded`, read in full, in place of the files of the same path.
export function withLoaded(v: DiffView, loaded: readonly DiffFile[]): DiffView {
  const by = new Map(loaded.map(f => [f.path, f]));
  return { ...v, files: v.files.map(f => by.get(f.path) ?? f) };
}

/// The file at the top of the list: the last whose header is at or above
/// `scroll`, given each file's top in `tops`.
export function currentFile(tops: readonly number[], scroll: number) {
  let at = 0;
  for (let i = 0; i < tops.length; i++) if (tops[i] <= scroll + 1) at = i;
  return at;
}

/// Where j (`dir` 1) or k (-1) goes from file `at` of `count`; `null`
/// past either end.
export function stepFile(at: number, count: number, dir: 1 | -1) {
  const next = at + dir;
  return next < 0 || next >= count ? null : next;
}

/// Whether a box from `top` to `bottom` shows in a list scrolled to
/// `scroll` with `height` on screen.
export function inView(top: number, bottom: number, scroll: number, height: number) {
  return bottom > scroll && top < scroll + height;
}

/// `path` split for the file list: its folder (with the trailing `/`) and its name.
export function splitPath(path: string) {
  const cut = path.lastIndexOf("/");
  return { dir: path.slice(0, cut + 1), name: path.slice(cut + 1) };
}
