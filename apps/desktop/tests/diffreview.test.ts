import test from "node:test";
import assert from "node:assert/strict";
import { type DiffFile, type ReviewComment, placeComments, reviewText } from "../src/review/diff-state.ts";

const file = (path: string, o: Partial<DiffFile> = {}): DiffFile => ({
  path, old_path: null, status: "modified", additions: 1, deletions: 1, left_out: null,
  hunks: [{ old_start: 40, old_lines: 3, new_start: 40, new_lines: 3, section: "", lines: [{ k: " ", t: "a" }, { k: "-", t: "b" }, { k: "+", t: "B" }, { k: " ", t: "c" }] }],
  ...o,
});
const line = (path: string, n: number, text: string, side: "old" | "new" = "new"): ReviewComment => ({ path, side, line: n, text });
const whole = (path: string, text: string): ReviewComment => ({ path, side: "new", line: null, text });

test("the review names each line, removed line, whole file and rename, in diff order", () => {
  const files = [
    file("src/login.rs"),
    file("src/old.rs", { status: "deleted" }),
    file("src/new.rs", { status: "renamed", old_path: "src/old2.rs" }),
  ];
  const text = reviewText(files, [
    whole("src/new.rs", "Keep the old name."),
    line("src/login.rs", 50, "Why did this go?", "old"),
    whole("src/old.rs", "The iOS app still reads it."),
    line("src/login.rs", 42, "Use the shared limit here."),
  ]);
  assert.equal(text, [
    "Review of your changes:",
    "",
    "src/login.rs:42: Use the shared limit here.",
    "src/login.rs:50 (removed line): Why did this go?",
    "src/old.rs (whole file, deleted): The iOS app still reads it.",
    "src/new.rs (whole file, renamed from src/old2.rs): Keep the old name.",
  ].join("\n"));
});

test("within a file the whole-file comment comes first, then lines, old side before new", () => {
  const text = reviewText([file("a.ts")], [line("a.ts", 9, "three"), line("a.ts", 9, "two", "old"), whole("a.ts", "one")]);
  assert.deepEqual(text.split("\n").slice(2), ["a.ts (whole file): one", "a.ts:9 (removed line): two", "a.ts:9: three"]);
});

test("a comment whose line is gone moves to the top of its file, and one whose file is gone is kept apart", () => {
  const files = [file("a.ts")];
  const here = line("a.ts", 41, "removed side", "old");
  const moved = line("a.ts", 99, "gone line");
  const away = line("b.ts", 1, "gone file");
  const placed = placeComments(files, [here, moved, away, whole("a.ts", "all")]);
  const a = placed.files.get("a.ts")!;
  assert.deepEqual(a.lines.get("old:41"), [here]);
  assert.deepEqual(a.top, [moved]);
  assert.equal(a.file.length, 1);
  assert.deepEqual(placed.lost, [away]);
});

test("a context line takes comments on its new side only", () => {
  const files = [file("a.ts")];
  const placed = placeComments(files, [line("a.ts", 40, "ok"), line("a.ts", 40, "old numbering", "old")]);
  const a = placed.files.get("a.ts")!;
  assert.equal(a.lines.get("new:40")?.length, 1);
  assert.equal(a.top.length, 1);
});
