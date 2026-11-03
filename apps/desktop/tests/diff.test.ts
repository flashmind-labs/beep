import test from "node:test";
import assert from "node:assert/strict";
import {
  type DiffFile, type DiffView, FOLD_LINES, changedLabel, currentFile, diffRows, hunkHeader, isFolded, leftOutNote,
  inView, matchesFilter, numberWidth, splitPath, startsFolded, stepFile, withLoaded,
} from "../src/diff-state.ts";

const file = (path: string, o: Partial<DiffFile> = {}): DiffFile => ({
  path, old_path: null, status: "modified", additions: 1, deletions: 1, hunks: [], left_out: null, ...o,
});

test("deleted files, big files and lockfiles start folded", () => {
  assert.equal(startsFolded(file("a.rs")), false);
  assert.equal(startsFolded(file("gone.rs", { status: "deleted", additions: 0, deletions: 3 })), true);
  assert.equal(startsFolded(file("big.rs", { additions: FOLD_LINES, deletions: 0 })), false);
  assert.equal(startsFolded(file("big.rs", { additions: FOLD_LINES, deletions: 1 })), true);
  assert.equal(startsFolded(file("Cargo.lock", { left_out: "lockfile" })), true);
  assert.equal(startsFolded(file("new.rs", { status: "added", additions: 30, deletions: 0 })), false);
});

test("a file folded or opened by hand stays so", () => {
  const big = file("big.rs", { additions: 900 });
  assert.equal(isFolded(big, new Map()), true);
  assert.equal(isFolded(big, new Map([["big.rs", false]])), false);
  assert.equal(isFolded(file("a.rs"), new Map([["a.rs", true]])), true);
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
  assert.equal(hunkHeader({ old_start: 0, old_lines: 0, new_start: 1, new_lines: 2, section: "" }), "@@ -0,0 +1,2 @@");
});

test("the gutter fits the widest line number", () => {
  assert.equal(numberWidth({ hunks: [] }), 1);
  assert.equal(numberWidth({ hunks: [{ old_start: 990, old_lines: 5, new_start: 1000, new_lines: 3, section: "", lines: [] }] }), 4);
});

test("the filter matches either path of a rename, any case", () => {
  const moved = file("src/new.rs", { old_path: "src/Old.rs", status: "renamed" });
  assert.ok(matchesFilter(moved, "old"));
  assert.ok(matchesFilter(moved, " NEW "));
  assert.ok(matchesFilter(moved, ""));
  assert.ok(!matchesFilter(moved, "lib"));
});

test("files without lines say why and offer to load them", () => {
  assert.deepEqual(leftOutNote(file("Cargo.lock", { left_out: "lockfile", additions: 30, deletions: 12 })), { text: "Lockfile, 42 changed lines.", load: "Show" });
  assert.deepEqual(leftOutNote(file("big.json", { left_out: "too_large", additions: 2500, deletions: 1 })), { text: "Too large to show: 2,501 changed lines.", load: "Load anyway" });
  assert.equal(leftOutNote(file("logo.png", { status: "binary" }))?.load, null);
  assert.equal(leftOutNote(file("x.rs", { hunks: [{ old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, section: "", lines: [] }] })), null);
});

test("a file loaded in full takes its place", () => {
  const view: DiffView = { root: "/srv/project", base: "origin/main", merge_base: "abc", working_tree: true, files: [file("a.rs"), file("big.rs", { left_out: "too_large" })], additions: 2, deletions: 2, more: 0 };
  const full = file("big.rs", { hunks: [{ old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, section: "", lines: [] }] });
  const next = withLoaded(view, [full]);
  assert.deepEqual(next.files.map(f => f.left_out), [null, null]);
  assert.equal(next.files[1], full);
  assert.equal(changedLabel(next), "2 files changed");
  assert.equal(changedLabel({ files: [], more: 0 }), "No changes");
  assert.equal(changedLabel({ files: [file("a")], more: 1000 }), "1,001 files changed");
});

test("the file at the top is the current one once scrolled", () => {
  const tops = [0, 300, 900];
  assert.equal(currentFile(tops, 0), 0);
  assert.equal(currentFile(tops, 450), 1);
  assert.equal(currentFile(tops, 2000), 2);
  assert.ok(inView(300, 600, 0, 400));
  assert.ok(!inView(900, 1200, 0, 400));
  assert.ok(!inView(0, 280, 300, 400));
});

test("j and k step one file and stop at the ends", () => {
  assert.equal(stepFile(0, 3, 1), 1);
  assert.equal(stepFile(2, 3, 1), null);
  assert.equal(stepFile(1, 3, -1), 0);
  assert.equal(stepFile(0, 3, -1), null);
  assert.equal(stepFile(-1, 0, 1), null);
});

test("a path splits into its folder and name", () => {
  assert.deepEqual(splitPath("src/a/b.rs"), { dir: "src/a/", name: "b.rs" });
  assert.deepEqual(splitPath("README.md"), { dir: "", name: "README.md" });
});
