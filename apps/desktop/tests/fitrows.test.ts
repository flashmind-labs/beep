import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { fitRows } from "../src/components/parts.ts";

test("fitRows shows every row in one scrolling box", () => {
  const rows = [1, 2, 3, 4, 5, 6].map(() => document.createElement("div"));
  const box = fitRows(rows, 5);
  assert.equal(box.className, "pj-scroll");
  assert.equal(box.children.length, 6);
});
