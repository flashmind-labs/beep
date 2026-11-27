// The diff viewer's data and the rules it draws by, apart from the DOM:
// which files start folded, a hunk's rows with their line numbers, the
// filter, and which file j and k go to.
import type { Colour, Span } from "../../ui/syntax.ts";

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
type LeftOut = "too_large" | "lockfile" | "over_total";

export interface DiffFile {
  path: string;
  old_path: string | null;
  status: DiffStatus;
  additions: number;
  deletions: number;
  hunks: DiffHunk[];
  left_out: LeftOut | null;
}

export function sameDiffFile(a: DiffFile, b: DiffFile): boolean {
  return a.path === b.path && a.old_path === b.old_path && a.status === b.status && a.additions === b.additions && a.deletions === b.deletions && a.left_out === b.left_out && a.hunks.length === b.hunks.length && a.hunks.every((hunk, index) => {
    const other = b.hunks[index];
    return hunk.old_start === other.old_start && hunk.old_lines === other.old_lines && hunk.new_start === other.new_start && hunk.new_lines === other.new_lines && hunk.section === other.section && hunk.lines.length === other.lines.length && hunk.lines.every((line, i) => line.k === other.lines[i].k && line.t === other.lines[i].t && line.e === other.lines[i].e);
  });
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
const FOLD_LINES = 400;

/// Whether `f` starts folded: a deleted file, one past [`FOLD_LINES`]
/// changed lines, and a lockfile left out.
function startsFolded(f: Pick<DiffFile, "status" | "additions" | "deletions" | "left_out">) {
  return f.status === "deleted" || f.additions + f.deletions > FOLD_LINES || f.left_out === "lockfile";
}

/// Whether `f` is folded now: as the user left it while the view is open,
/// else as it starts.
export function isFolded(f: DiffFile, kept: ReadonlyMap<string, boolean>) {
  return kept.get(f.path) ?? startsFolded(f);
}

export const STATUS_LETTER: Record<DiffStatus, string> = { added: "A", modified: "M", deleted: "D", renamed: "R", binary: "B" };

/// A hunk's header: `index` counts the file's hunks, `gap` the unchanged
/// lines left out before it, and `expanded` says some of them were shown.
export interface HunkRow { kind: "hunk"; text: string; index: number; gap: number; expanded: boolean }

/// One row of a file's body.
export type DiffRow =
  | HunkRow
  | { kind: "line"; k: DiffLine["k"]; old: number | null; new: number | null; text: string; eol: boolean };

function hunkHeader(h: Pick<DiffHunk, "old_start" | "old_lines" | "new_start" | "new_lines" | "section">) {
  const side = (start: number, n: number) => (n === 1 ? `${start}` : `${start},${n}`);
  return `@@ -${side(h.old_start, h.old_lines)} +${side(h.new_start, h.new_lines)} @@${h.section ? ` ${h.section}` : ""}`;
}

/// `f`'s rows: each hunk's header, with the unchanged lines left out before
/// it, then its lines numbered on both sides. `extents` is the context the
/// file was expanded with, a hunk each, or `null`.
export function diffRows(f: Pick<DiffFile, "hunks">, extents: readonly Extent[] | null = null): DiffRow[] {
  const out: DiffRow[] = [];
  let lastOld: number | null = null;

  f.hunks.forEach((h, index) => {
    // A side with no lines names the line before it, so the first one
    // after is the start, not the one past it.
    let old = h.old_lines ? h.old_start : h.old_start + 1;
    let now = h.new_lines ? h.new_start : h.new_start + 1;
    const gap = lastOld === null ? 0 : Math.max(0, old - lastOld - 1);
    const expanded = index > 0 && !!extents && (extents[index - 1].below > 0 || extents[index].above > 0);
    out.push({ kind: "hunk", text: hunkHeader(h), index, gap, expanded });

    for (const l of h.lines) {
      const row: DiffRow = { kind: "line", k: l.k, old: l.k === "+" ? null : old, new: l.k === "-" ? null : now, text: l.t, eol: !!l.e };
      if (l.k !== "+") old++;
      if (l.k !== "-") now++;
      out.push(row);
    }

    lastOld = old - 1;
  });

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
  | HunkRow
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
  for (const r of rows) h += r.kind === "hunk" ? 19 : 19 * Math.max(lines(r.left), lines(r.right));
  return h;
}

/// How many digits the widest line number of `f` has.
export function numberWidth(f: Pick<DiffFile, "hunks">) {
  let max = 1;
  for (const h of f.hunks) max = Math.max(max, h.old_start + h.old_lines, h.new_start + h.new_lines);
  return String(max).length;
}

/// Whether `f` matches the filter `q`: its path, old path or a changed
/// line holds it, any case.
export function matchesFilter(f: Pick<DiffFile, "path" | "old_path" | "hunks">, q: string) {
  const want = q.trim().toLowerCase();
  if (!want) return true;
  return f.path.toLowerCase().includes(want) || !!f.old_path?.toLowerCase().includes(want) || lineMatches(f, q, 1).length > 0;
}

/// A place in the diff holding the filter: a line, on the side its number
/// is of, and the offset of the text found in it.
export interface LineMatch { side: "old" | "new"; line: number; text: string; at: number }

/// A place found in file `path`, the one the filter steps to.
export interface Hit extends LineMatch { path: string }

/// The lines of `f` holding the filter `q`, any case, at most `limit`, each
/// with the first place it holds it.
export function lineMatches(f: Pick<DiffFile, "hunks">, q: string, limit: number): LineMatch[] {
  const out: LineMatch[] = [];

  for (const match of everyMatch(f, q)) {
    const last = out.at(-1);
    if (last && last.side === match.side && last.line === match.line) continue;
    if (out.push(match) >= limit) break;
  }

  return out;
}

/// Every place in the lines of `files` holding the filter `q`, any case, in order.
export function filterHits(files: readonly DiffFile[], q: string): Hit[] {
  return files.flatMap(f => [...everyMatch(f, q)].map(match => ({ ...match, path: f.path })));
}

export const sameHit = (a: Hit | null, b: Hit | null) =>
  a === b || (!!a && !!b && a.path === b.path && a.side === b.side && a.line === b.line && a.at === b.at);

function* everyMatch(f: Pick<DiffFile, "hunks">, q: string): Generator<LineMatch> {
  const want = q.trim().toLowerCase();
  if (!want) return;

  for (const h of f.hunks) {
    let old = h.old_lines ? h.old_start : h.old_start + 1;
    let now = h.new_lines ? h.new_start : h.new_start + 1;

    for (const l of h.lines) {
      const side = l.k === "-" ? "old" : "new", line = l.k === "-" ? old : now;
      for (const at of occurrences(l.t, want)) yield { side, line, text: l.t, at };
      if (l.k !== "+") old++;
      if (l.k !== "-") now++;
    }
  }
}

/// Where `text` holds `want`, already lower case, any case, without overlaps.
export function occurrences(text: string, want: string): number[] {
  const lower = text.toLowerCase(), out: number[] = [];
  if (!want) return out;
  for (let i = lower.indexOf(want); i >= 0; i = lower.indexOf(want, i + want.length)) out.push(i);
  return out;
}

/// Each line's coloured spans by [`lineKey`]: each side of a hunk is
/// coloured as one text, so a context line has spans on both sides.
export function diffSpans(f: Pick<DiffFile, "hunks">, colour: Colour): Map<string, readonly Span[]> {
  const out = new Map<string, readonly Span[]>();

  for (const h of f.hunks) {
    for (const side of ["old", "new"] as const) {
      const lines = h.lines.filter(l => l.k !== (side === "old" ? "+" : "-")), first = side === "old" ? h.old_start : h.new_start;
      colour(lines.map(l => l.t)).forEach((spans, i) => { if (spans.length) out.set(lineKey(side, first + i), spans); });
    }
  }

  return out;
}

/// `n` with thousands separators. One formatter for all: `toLocaleString`
/// makes a new one on every call, which shows in a list of thousands.
const NUMBERS = new Intl.NumberFormat("en-US");
export const grouped = (n: number) => NUMBERS.format(n);

const count = (n: number, one: string, many: string) => `${grouped(n)} ${n === 1 ? one : many}`;

/// "6 files", or "No changes".
export function changedLabel(v: Pick<DiffView, "files" | "more">) {
  const n = v.files.length + v.more;
  return n ? count(n, "file", "files") : "No changes";
}

/// What the diff is against, in full, for the header's tooltip.
export function baseTitle(v: Pick<DiffView, "base" | "merge_base" | "working_tree">, uncommitted: boolean) {
  if (uncommitted) return "Against HEAD, uncommitted changes only";
  let t = `Against ${v.base}`;
  if (v.merge_base) t += ` at ${v.merge_base.slice(0, 7)}`;
  if (v.working_tree) t += ", uncommitted included";
  return t;
}

/// Whether every file shown is folded, so the one Expand / Collapse toggle
/// offers Expand ("+"); with any open, or none shown, it offers Collapse.
export function allFolded(files: readonly DiffFile[], kept: ReadonlyMap<string, boolean>) {
  return files.length > 0 && files.every(f => isFolded(f, kept));
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

/// The file at the top of the list: the last of `count` whose header is at
/// or above `scroll`, given each file's top by `top`, rising down the list.
export function currentFile(count: number, top: (index: number) => number, scroll: number) {
  let low = 0, high = count;

  while (low < high) {
    const mid = (low + high) >> 1;
    if (top(mid) <= scroll + 1) low = mid + 1;
    else high = mid;
  }

  return Math.max(0, low - 1);
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

/// Keep each folder's files together in the tree's first-seen order.
export function filesInTreeOrder(files: readonly DiffFile[]): DiffFile[] {
  type Entry = { kind: "file"; file: DiffFile } | { kind: "folder"; children: Map<string, Entry> };
  const root = new Map<string, Entry>();

  for (const file of files) {
    const parts = file.path.split("/");
    let children = root;

    for (const part of parts.slice(0, -1)) {
      const key = `${part}/`;
      let folder = children.get(key);

      if (!folder) {
        folder = { kind: "folder", children: new Map() };
        children.set(key, folder);
      }

      if (folder.kind === "folder") children = folder.children;
    }

    children.set(file.path, { kind: "file", file });
  }

  const ordered: DiffFile[] = [];
  const visit = (children: ReadonlyMap<string, Entry>) => {
    for (const entry of children.values()) {
      if (entry.kind === "file") ordered.push(entry.file);
      else visit(entry.children);
    }
  };
  visit(root);
  return ordered;
}

/// Visible folders in the changed-file tree, keyed by their full path.
export function compactFolders(paths: readonly string[]) {
  const children = new Map<string, Set<string>>();
  const fileParents = new Set<string>();

  for (const path of paths) {
    const { dir } = splitPath(path);
    fileParents.add(dir);
    let parent = "";

    for (const part of dir.split("/").filter(Boolean)) {
      const folder = parent + part + "/";
      let siblings = children.get(parent);

      if (!siblings) {
        siblings = new Set();
        children.set(parent, siblings);
      }

      siblings.add(folder);
      parent = folder;
    }
  }

  const visible = new Set([...children.values()].flatMap(folders => [...folders])
    .filter(path => fileParents.has(path) || (children.get(path)?.size ?? 0) !== 1));
  const folders = new Map<string, { dir: string; name: string }>();

  for (const path of visible) {
    let parent = splitPath(path.slice(0, -1)).dir;

    while (parent && !visible.has(parent)) parent = splitPath(parent.slice(0, -1)).dir;

    folders.set(path, { dir: parent, name: path.slice(parent.length, -1) });
  }

  return folders;
}

/// A comment on the diff, on a line of a file (`line` set, numbered on the
/// old or the new side) or on the whole file (`line` null).
export interface ReviewComment {
  path: string;
  side: "old" | "new";
  line: number | null;
  text: string;
  /// The harness that wrote it, for a comment an agent made.
  by?: string;
}

/// How many comments each file has, by path.
export function commentCounts(comments: readonly ReviewComment[]) {
  const out = new Map<string, number>();
  for (const c of comments) out.set(c.path, (out.get(c.path) ?? 0) + 1);
  return out;
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

/// The message that sends `draft` to the agent, in Markdown under
/// `heading`: a section per comment, files in diff order, a file's own
/// comment first, then its lines in order.
export function reviewText(heading: string, files: readonly Pick<DiffFile, "path" | "old_path" | "status">[], draft: readonly ReviewComment[]) {
  const by = new Map(files.map(f => [f.path, f]));
  const rank = new Map(files.map((f, i) => [f.path, i]));
  const at = (c: ReviewComment) => rank.get(c.path) ?? files.length;
  const sorted = draft.slice().sort((a, b) => at(a) - at(b) || a.path.localeCompare(b.path)
    || (a.line ?? 0) - (b.line ?? 0) || Number(a.side === "new") - Number(b.side === "new"));
  const sections = sorted.map(c => {
    const f = by.get(c.path);
    const notes: string[] = [];
    if (c.line == null) notes.push("whole file");
    else if (c.side === "old") notes.push("removed line");
    if (c.line == null && f?.status === "deleted") notes.push("deleted");
    if (f?.old_path) notes.push(`renamed from ${f.old_path}`);
    const where = c.line == null ? c.path : `${c.path}:${c.line}`;
    return `### ${where}${notes.length ? ` (${notes.join(", ")})` : ""}\n\n${c.text}`;
  });
  return [`# ${heading}`, ...sections].join("\n\n");
}

/// The draft as stored; what is not a comment is dropped.
export function parseDraft(raw: string | null): ReviewComment[] {
  try {
    const got: unknown = raw ? JSON.parse(raw) : [];
    if (!Array.isArray(got)) return [];
    return got.filter((c): c is ReviewComment => !!c && typeof c.path === "string" && typeof c.text === "string"
      && (c.side === "old" || c.side === "new") && (c.line === null || Number.isInteger(c.line)))
      .map(c => (typeof c.by === "string" && /^[a-z0-9-]{1,32}$/.test(c.by) ? c : { path: c.path, side: c.side, line: c.line, text: c.text }));
  } catch {
    return [];
  }
}

/// How many more lines of context a hunk shows on each side.
export interface Extent { above: number; below: number }

/// A change to the context of one side of a hunk: ten more lines, or back
/// to none.
export interface ContextEdit { hunk: number; side: keyof Extent; show: boolean }

/// `extents` after `edits`.
export function editExtents(extents: readonly Extent[], edits: readonly ContextEdit[]): Extent[] {
  const next = extents.map(extent => ({ ...extent }));
  for (const { hunk, side, show } of edits) if (next[hunk]) next[hunk][side] = show ? next[hunk][side] + 10 : 0;
  return next;
}

/// Select surrounding lines from a wider patch without changing line
/// numbers: each hunk of `original` with its own extent. The result has a
/// hunk for each of `original`, also where two of them come to touch.
export function contextFile(original: DiffFile, full: DiffFile, extents: readonly Extent[]): DiffFile {
  const hunks: DiffHunk[] = [];

  for (const h of full.hunks) {
    let old = h.old_start, now = h.new_start;
    let kept: DiffHunk | null = null, owner: number | null = null;

    for (const line of h.lines) {
      const at = line.k === "-" ? hunkAt(original.hunks, extents, "old", old) : hunkAt(original.hunks, extents, "new", now);

      if (at === null) kept = null;
      else {
        if (!kept || at !== owner) { kept = { old_start: old, new_start: now, old_lines: 0, new_lines: 0, section: h.section, lines: [] }; hunks.push(kept); }
        kept.lines.push(line);
        if (line.k !== "+") kept.old_lines++;
        if (line.k !== "-") kept.new_lines++;
      }

      owner = at;
      if (line.k !== "+") old++;
      if (line.k !== "-") now++;
    }
  }

  return { ...full, hunks };
}

/// Which hunk shows line `at` of `side` with its extent, `null` for none.
/// Where two extents overlap the lines go to the later hunk, up to the
/// earlier one's own lines.
function hunkAt(hunks: readonly DiffHunk[], extents: readonly Extent[], side: "old" | "new", at: number): number | null {
  const range = (h: DiffHunk) => side === "old" ? [h.old_start, h.old_start + h.old_lines] : [h.new_start, h.new_start + h.new_lines];

  for (let i = hunks.length - 1; i >= 0; i--) {
    const [start, end] = range(hunks[i]);
    const floor = i ? range(hunks[i - 1])[1] : -Infinity;
    if (at >= Math.max(start - extents[i].above, floor)) return at < end + extents[i].below ? i : null;
  }

  return null;
}

/// Heights the file list lays out by before a file is drawn and measured:
/// its header, and each kind of row in its body.
const HEAD_HEIGHT = 37, NOTE_HEIGHT = 40, ROW_HEIGHT = 19, CONTEXT_HEIGHT = 44;

/// How tall `f`'s section is likely to be: its header, and its unified rows
/// while it is open.
export function estimateHeight(f: DiffFile, folded: boolean) {
  if (folded) return HEAD_HEIGHT;
  if (leftOutNote(f)) return HEAD_HEIGHT + NOTE_HEIGHT;
  const rows = f.hunks.reduce((sum, h) => sum + 1 + h.lines.length, 0);
  return HEAD_HEIGHT + CONTEXT_HEIGHT + rows * ROW_HEIGHT;
}
