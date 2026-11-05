import test from "node:test";
import assert from "node:assert/strict";
import { reuseRows } from "../src/agents/logdiff-state.ts";

test("an unchanged log keeps every row", () => {
  assert.deepEqual(reuseRows(["a", "b", "c"], ["a", "b", "c"]), [0, 1, 2]);
});

test("only the row that changed is drawn", () => {
  assert.deepEqual(reuseRows(["a", "b", "live 1"], ["a", "b", "live 2"]), [0, 1, -1]);
});

test("a log that slides keeps the rows that moved up", () => {
  assert.deepEqual(reuseRows(["a", "b", "c"], ["b", "c", "d"]), [1, 2, -1]);
});

test("identical rows are each reused once", () => {
  assert.deepEqual(reuseRows(["x", "x"], ["x", "x", "x"]), [0, 1, -1]);
});
