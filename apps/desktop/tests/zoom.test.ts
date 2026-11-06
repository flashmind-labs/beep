import test from "node:test";
import assert from "node:assert/strict";
import { nextZoom, zoomKey } from "../src/ui/zoom-state.ts";

const key = (k: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) =>
  zoomKey({ key: k, metaKey: true, ctrlKey: false, altKey: false, ...mods });

test("⌘= and ⌘+ zoom in, ⌘- out, ⌘0 resets", () => {
  assert.equal(key("="), "in");
  assert.equal(key("+"), "in");
  assert.equal(key("-"), "out");
  assert.equal(key("0"), "reset");
  assert.equal(key("1"), null);
  assert.equal(key("=", { metaKey: false }), null);
  assert.equal(key("-", { altKey: true }), null);
  assert.equal(key("0", { ctrlKey: true }), null);
});

test("steps of 10% between 70% and 200%", () => {
  assert.equal(nextZoom(100, "in"), 110);
  assert.equal(nextZoom(100, "out"), 90);
  assert.equal(nextZoom(150, "reset"), 100);
  assert.equal(nextZoom(200, "in"), 200);
  assert.equal(nextZoom(70, "out"), 70);
  assert.equal(nextZoom(80, "out"), 70);
  assert.equal(nextZoom(190, "in"), 200);
});

test("an odd level goes from the nearest step and back into range", () => {
  assert.equal(nextZoom(104, "in"), 110);
  assert.equal(nextZoom(500, "out"), 200);
  assert.equal(nextZoom(10, "in"), 70);
});
