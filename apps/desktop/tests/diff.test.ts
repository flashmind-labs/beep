import test from "node:test";
import assert from "node:assert/strict";
import {
  type DiffFile, type DiffView, diffRows, withLoaded,
} from "../src/diff-state.ts";

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
