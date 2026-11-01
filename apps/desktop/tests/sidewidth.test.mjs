import test from "node:test";
import assert from "node:assert/strict";
import { clampWidth, stepWidth, SIDE_MIN, SIDE_MAX, SIDE_DEFAULT, SIDE_STEP } from "../dist/sidewidth.js";

test("a width stays between the limits", () => {
  assert.equal(clampWidth(100), SIDE_MIN);
  assert.equal(clampWidth(900), SIDE_MAX);
  assert.equal(clampWidth(250.4), 250);
});

test("a stored width that cannot be read is the default", () => {
  assert.equal(clampWidth(null), SIDE_DEFAULT);
  assert.equal(clampWidth(""), SIDE_DEFAULT);
  assert.equal(clampWidth("wide"), SIDE_DEFAULT);
  assert.equal(clampWidth("300"), 300);
});

test("the arrow keys move the width a step and stop at the limits", () => {
  assert.equal(stepWidth(200, "ArrowRight"), 200 + SIDE_STEP);
  assert.equal(stepWidth(200, "ArrowLeft"), 200 - SIDE_STEP);
  assert.equal(stepWidth(SIDE_MIN + 4, "ArrowLeft"), SIDE_MIN);
  assert.equal(stepWidth(SIDE_MAX, "ArrowRight"), SIDE_MAX);
  assert.equal(stepWidth(200, "ArrowUp"), null);
});
