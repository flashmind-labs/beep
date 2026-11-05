// What differs between a conflict copy and the file it stands beside: both
// are read from a machine that has them and diffed here, so no node change
// is needed. The result is a `DiffView` the diff viewer draws.
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { DiffHunk, DiffLine, DiffView } from "../review/diff-state.ts";
import { bytes, reason } from "../ui/format.ts";
import type { Conflict, MachineDetail } from "./filesync.ts";

/// The start of a file, as `agent_tree_text` reads it.
interface TreeText {
  size: number;
  text: string;
  cut: boolean;
  binary: boolean;
}

/// Unchanged lines kept around each change.
const CONTEXT = 3;
/// The most cells of the table a diff may fill, about 16 MB.
const MAX_CELLS = 4_000_000;

/// A file's lines, without the empty one a closing newline leaves.
function linesOf(text: string) {
  const out = text.split("\n");
  if (out[out.length - 1] === "") out.pop();
  return out;
}

/// The lines of `a` and `b` in order, each marked as in both, only `a` (`-`)
/// or only `b` (`+`); null when the part that differs is too big to match.
function marked(a: readonly string[], b: readonly string[]): DiffLine[] | null {
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head++;
  let tail = 0;
  while (tail < a.length - head && tail < b.length - head && a[a.length - 1 - tail] === b[b.length - 1 - tail]) tail++;
  const x = a.slice(head, a.length - tail);
  const y = b.slice(head, b.length - tail);
  if (x.length * y.length > MAX_CELLS) return null;
  const w = y.length + 1;
  const t = new Uint32Array((x.length + 1) * w);
  for (let i = x.length - 1; i >= 0; i--) {
    for (let j = y.length - 1; j >= 0; j--) {
      t[i * w + j] = x[i] === y[j] ? t[(i + 1) * w + j + 1] + 1 : Math.max(t[(i + 1) * w + j], t[i * w + j + 1]);
    }
  }
  const out: DiffLine[] = a.slice(0, head).map(l => ({ k: " ", t: l }));
  let i = 0, j = 0;
  while (i < x.length || j < y.length) {
    if (i < x.length && j < y.length && x[i] === y[j]) {
      out.push({ k: " ", t: x[i] });
      i++;
      j++;
    } else if (i < x.length && (j === y.length || t[(i + 1) * w + j] >= t[i * w + j + 1])) {
      out.push({ k: "-", t: x[i++] });
    } else {
      out.push({ k: "+", t: y[j++] });
    }
  }
  for (const l of a.slice(a.length - tail)) out.push({ k: " ", t: l });
  return out;
}

/// The hunks that turn `a` into `b`, with [`CONTEXT`] lines around each
/// change; empty when they are equal, null when too big to compare.
export function lineDiff(a: string, b: string): DiffHunk[] | null {
  const lines = marked(linesOf(a), linesOf(b));
  if (!lines) return null;
  // Old and new lines before each line.
  const before = [{ old: 0, now: 0 }];
  for (const l of lines) {
    const p = before[before.length - 1];
    before.push({ old: p.old + (l.k === "+" ? 0 : 1), now: p.now + (l.k === "-" ? 0 : 1) });
  }
  const spans: { lo: number; hi: number }[] = [];
  lines.forEach((l, i) => {
    if (l.k === " ") return;
    const lo = Math.max(0, i - CONTEXT), hi = Math.min(lines.length, i + CONTEXT + 1);
    const last = spans[spans.length - 1];
    if (last && lo <= last.hi) last.hi = hi;
    else spans.push({ lo, hi });
  });
  return spans.map(({ lo, hi }) => {
    const old_lines = before[hi].old - before[lo].old;
    const new_lines = before[hi].now - before[lo].now;
    return {
      old_start: old_lines ? before[lo].old + 1 : before[lo].old,
      old_lines,
      new_start: new_lines ? before[lo].now + 1 : before[lo].now,
      new_lines,
      section: "",
      lines: lines.slice(lo, hi),
    };
  });
}

/// The diff of the file `c.original` against its copy as a view of one
/// file, or the one line saying why there is none.
export function compareView(c: Pick<Conflict, "original">, kept: string, copy: string): DiffView | string {
  const hunks = lineDiff(kept, copy);
  if (!hunks) return "Too many lines differ to compare here";
  if (!hunks.length) return "The two files are the same";
  const count = (k: DiffLine["k"]) => hunks.reduce((n, h) => n + h.lines.filter(l => l.k === k).length, 0);
  const additions = count("+"), deletions = count("-");
  return {
    root: "", base: "", merge_base: "", working_tree: false, more: 0, additions, deletions,
    files: [{ path: c.original, old_path: null, status: "modified", additions, deletions, hunks, left_out: null }],
  };
}

/// Reads the conflict's two files from `machine` and compares them.
export async function compareFiles(invoke: typeof Invoke, machine: Pick<MachineDetail, "machine_id" | "folder">, c: Pick<Conflict, "path" | "original">) {
  if (!machine.folder) return "That machine's folder is not known";
  try {
    const read = (path: string) => invoke<TreeText>("agent_tree_text", { machineId: machine.machine_id, root: machine.folder, path });
    const [kept, copy] = await Promise.all([read(c.original), read(c.path)]);
    for (const f of [kept, copy]) {
      if (f.binary) return "A binary file: nothing to compare as text";
      if (f.cut) return `Too large to compare here (${bytes(Math.max(kept.size, copy.size))})`;
    }
    return compareView(c, kept.text, copy.text);
  } catch (e) {
    return reason(e);
  }
}
