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

/// One side of a side by side row.
export interface SplitCell {
  k: DiffLine["k"];
  num: number;
  text: string;
  eol: boolean;
}

/// A row of the side by side view: the removed lines on the left across
/// from the added lines on the right, `null` where one side has none.
export type SplitRow =
  | { kind: "hunk"; text: string }
  | { kind: "gap"; lines: number }
  | { kind: "pair"; left: SplitCell | null; right: SplitCell | null };

/// `rows` laid out side by side: a run of removed lines is paired, row by
/// row, with the added lines that follow it, and an unchanged line shows on
/// both sides with its own number.
export function splitRows(rows: readonly DiffRow[]): SplitRow[] {
  const out: SplitRow[] = [];
  let dels: SplitCell[] = [];
  let adds: SplitCell[] = [];
  const flush = () => {
    for (let i = 0; i < Math.max(dels.length, adds.length); i++) out.push({ kind: "pair", left: dels[i] ?? null, right: adds[i] ?? null });
    dels = [];
    adds = [];
  };
  for (const r of rows) {
    if (r.kind !== "line") {
      flush();
      out.push(r);
    } else if (r.k === "-") {
      dels.push({ k: r.k, num: r.old!, text: r.text, eol: r.eol });
    } else if (r.k === "+") {
      adds.push({ k: r.k, num: r.new!, text: r.text, eol: r.eol });
    } else {
      flush();
      out.push({
        kind: "pair",
        left: { k: r.k, num: r.old!, text: r.text, eol: r.eol },
        right: { k: r.k, num: r.new!, text: r.text, eol: r.eol },
      });
    }
  }
  flush();
  return out;
}

/// How tall side by side rows are before they are drawn: a row wraps to its
/// longer cell. `cols` is the characters one cell fits.
export function splitHeight(rows: readonly SplitRow[], cols: number) {
  const fit = Math.max(1, cols);
  const lines = (c: SplitCell | null) => (c ? Math.max(1, Math.ceil(c.text.length / fit)) : 1);
  let h = 0;
  for (const r of rows) h += r.kind === "gap" ? 22 : r.kind === "hunk" ? 19 : 19 * Math.max(lines(r.left), lines(r.right));
  return h;
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

/// A comment on the diff, on a line of a file (`line` set, numbered on the
/// old or the new side) or on the whole file (`line` null).
export interface ReviewComment {
  path: string;
  side: "old" | "new";
  line: number | null;
  text: string;
}

/// How a row is named in [`Placed`]: the side and its line number.
export const lineKey = (side: ReviewComment["side"], n: number) => `${side}:${n}`;

/// "line 42" or "removed line 42" for a comment on a line, else "".
export function lineLabel(c: Pick<ReviewComment, "side" | "line">) {
  return c.line == null ? "" : c.side === "old" ? `removed line ${c.line}` : `line ${c.line}`;
}

/// Where a file shows its comments: on the whole file, on lines the diff
/// has (by [`lineKey`]), and `top` for those whose line it no longer has.
export interface PlacedFile {
  file: ReviewComment[];
  top: ReviewComment[];
  lines: Map<string, ReviewComment[]>;
}

export interface Placed {
  files: Map<string, PlacedFile>;
  /// Comments on files the diff no longer has.
  lost: ReviewComment[];
}

/// Sorts the draft by where each comment shows in `files`.
export function placeComments(files: readonly DiffFile[], draft: readonly ReviewComment[]): Placed {
  const by = new Map(files.map(f => [f.path, f]));
  const have = new Map<string, Set<string>>();
  const out: Placed = { files: new Map(), lost: [] };
  for (const c of draft) {
    const f = by.get(c.path);
    if (!f) { out.lost.push(c); continue; }
    let p = out.files.get(c.path);
    if (!p) out.files.set(c.path, p = { file: [], top: [], lines: new Map() });
    if (c.line == null) { p.file.push(c); continue; }
    let keys = have.get(c.path);
    if (!keys) {
      keys = new Set();
      for (const r of diffRows(f)) if (r.kind === "line") keys.add(r.k === "-" ? lineKey("old", r.old!) : lineKey("new", r.new!));
      have.set(c.path, keys);
    }
    const key = lineKey(c.side, c.line);
    if (!keys.has(key)) { p.top.push(c); continue; }
    const at = p.lines.get(key);
    if (at) at.push(c); else p.lines.set(key, [c]);
  }
  return out;
}

/// The message that sends `draft` to the agent: files in diff order, a
/// file's own comment first, then its lines in order.
export function reviewText(files: readonly Pick<DiffFile, "path" | "old_path" | "status">[], draft: readonly ReviewComment[]) {
  const by = new Map(files.map(f => [f.path, f]));
  const rank = new Map(files.map((f, i) => [f.path, i]));
  const at = (c: ReviewComment) => rank.get(c.path) ?? files.length;
  const sorted = draft.slice().sort((a, b) => at(a) - at(b) || a.path.localeCompare(b.path)
    || (a.line ?? 0) - (b.line ?? 0) || Number(a.side === "new") - Number(b.side === "new"));
  const lines = sorted.map(c => {
    const f = by.get(c.path);
    const notes: string[] = [];
    if (c.line == null) notes.push("whole file");
    else if (c.side === "old") notes.push("removed line");
    if (c.line == null && f?.status === "deleted") notes.push("deleted");
    if (f?.old_path) notes.push(`renamed from ${f.old_path}`);
    const where = c.line == null ? c.path : `${c.path}:${c.line}`;
    return `${where}${notes.length ? ` (${notes.join(", ")})` : ""}: ${c.text}`;
  });
  return `Review of your changes:\n\n${lines.join("\n")}`;
}

/// The draft as stored; what is not a comment is dropped.
export function parseDraft(raw: string | null): ReviewComment[] {
  try {
    const got: unknown = raw ? JSON.parse(raw) : [];
    if (!Array.isArray(got)) return [];
    return got.filter((c): c is ReviewComment => !!c && typeof c.path === "string" && typeof c.text === "string"
      && (c.side === "old" || c.side === "new") && (c.line === null || Number.isInteger(c.line)));
  } catch {
    return [];
  }
}
