import test from "node:test";
import assert from "node:assert/strict";
import { compareFiles, compareView, lineDiff } from "../src/project/compare.ts";

test("compare lists the lines that differ with their numbers", () => {
  const hunks = lineDiff("a\nb\nc\nd\n", "a\nB\nc\nd\ne\n")!;
  assert.equal(hunks.length, 1);
  assert.deepEqual(hunks[0].lines.map(l => l.k + l.t), [" a", "-b", "+B", " c", " d", "+e"]);
  assert.deepEqual([hunks[0].old_start, hunks[0].old_lines, hunks[0].new_start, hunks[0].new_lines], [1, 4, 1, 5]);
  assert.deepEqual(lineDiff("same\n", "same\n"), []);
  assert.equal(compareView({ original: "n.txt" }, "x\n", "x\n"), "The two files are the same");
});

test("compare keeps far apart changes in separate hunks", () => {
  const base = Array.from({ length: 30 }, (_, i) => `l${i}`);
  const other = base.slice();
  other[2] = "x";
  other[25] = "y";
  const hunks = lineDiff(base.join("\n"), other.join("\n"))!;
  assert.deepEqual(hunks.map(h => [h.old_start, h.old_lines]), [[1, 6], [23, 7]]);
});

test("compare says a file is too large, binary or gone in one line", async () => {
  const machine = { machine_id: "m1", folder: "/srv/project" };
  const c = { path: "n (conflict 1a2b3c4d).txt", original: "n.txt" };
  const reads = (o: object, bad = false) => (async () => {
    if (bad) throw new Error("gone");
    return { size: 10, text: "", cut: false, binary: false, ...o };
  }) as never;
  assert.match(String(await compareFiles(reads({ binary: true }), machine, c)), /binary/);
  assert.match(String(await compareFiles(reads({ cut: true, size: 3_000_000 }), machine, c)), /Too large/);
  assert.match(String(await compareFiles(reads({}, true), machine, c)), /gone/);
  assert.match(String(await compareFiles(reads({}), { machine_id: "m1", folder: "" }, c)), /folder/);
});
