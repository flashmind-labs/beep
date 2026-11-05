import test from "node:test";
import assert from "node:assert/strict";
import {
  type DiffFile, type DiffView, diffRows, splitRows, withLoaded,
} from "../src/review/diff-state.ts";

const file = (path: string, o: Partial<DiffFile> = {}): DiffFile => ({
  path, old_path: null, status: "modified", additions: 1, deletions: 1, hunks: [], left_out: null, ...o,
});

test("rows number both sides and count the lines between hunks", () => {
  const rows = diffRows({
    hunks: [
      { old_start: 1, old_lines: 3, new_start: 1, new_lines: 3, section: "", lines: [{ k: " ", t: "a" }, { k: "-", t: "b" }, { k: "+", t: "B" }, { k: " ", t: "c" }] },
      { old_start: 20, old_lines: 1, new_start: 20, new_lines: 2, section: "fn x() {", lines: [{ k: " ", t: "y" }, { k: "+", t: "z", e: true }] },
    ],
  });
  const text = rows.map(r => r.kind === "line" ? `${r.old ?? "."} ${r.new ?? "."} ${r.k}${r.text}${r.eol ? "$" : ""}` : r.kind === "gap" ? `gap ${r.lines}` : r.text);
  assert.deepEqual(text, [
    "@@ -1,3 +1,3 @@", "1 1  a", "2 . -b", ". 2 +B", "3 3  c",
    "gap 16", "@@ -20 +20,2 @@ fn x() {", "20 20  y", ". 21 +z$",
  ]);
});

test("a new file counts from its first line and a deleted one from its old", () => {
  const added = diffRows({ hunks: [{ old_start: 0, old_lines: 0, new_start: 1, new_lines: 2, section: "", lines: [{ k: "+", t: "a" }, { k: "+", t: "b" }] }] });
  assert.deepEqual(added.filter(r => r.kind === "line").map(r => r.kind === "line" && r.new), [1, 2]);
  const gone = diffRows({ hunks: [{ old_start: 1, old_lines: 1, new_start: 0, new_lines: 0, section: "", lines: [{ k: "-", t: "a" }] }] });
  assert.deepEqual(gone.filter(r => r.kind === "line").map(r => r.kind === "line" && r.old), [1]);
});

test("a file loaded in full takes its place", () => {
  const view: DiffView = { root: "/srv/project", base: "origin/main", merge_base: "abc", working_tree: true, files: [file("a.rs"), file("big.rs", { left_out: "too_large" })], additions: 2, deletions: 2, more: 0 };
  const full = file("big.rs", { hunks: [{ old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, section: "", lines: [] }] });
  const next = withLoaded(view, [full]);
  assert.deepEqual(next.files.map(f => f.left_out), [null, null]);
  assert.equal(next.files[1], full);
});

const hunk = (lines: string, old_start = 1, new_start = 1) => ({
  old_start, old_lines: 1, new_start, new_lines: 1, section: "",
  lines: [...lines].map(c => ({ k: c as "+" | "-" | " ", t: c === "+" ? "new" : c === "-" ? "old" : "same" })),
});
const pairs = (lines: string) => splitRows(diffRows({ hunks: [hunk(lines)] })).filter(r => r.kind === "pair")
  .map(r => (r.kind === "pair" ? [r.left?.num ?? null, r.right?.num ?? null] : []));

test("side by side pairs a replaced run row by row and pads the shorter side", () => {
  assert.deepEqual(pairs("--+"), [[1, 1], [2, null]]);
  assert.deepEqual(pairs("-+++"), [[1, 1], [null, 2], [null, 3]]);
});

test("side by side puts a pure add on the right and a pure delete on the left", () => {
  assert.deepEqual(pairs("++"), [[null, 1], [null, 2]]);
  assert.deepEqual(pairs("--"), [[1, null], [2, null]]);
});

test("side by side shows context on both sides and starts a new run after it", () => {
  assert.deepEqual(pairs("-+ -+"), [[1, 1], [2, 2], [3, 3]]);
  const rows = splitRows(diffRows({ hunks: [hunk(" -")] }));
  const ctx = rows[1];
  assert.equal(ctx.kind === "pair" && ctx.left?.k, " ");
  assert.equal(ctx.kind === "pair" && ctx.right?.text, "same");
});
