import test from "node:test";
import assert from "node:assert/strict";
import { ago, since } from "../src/components/format.ts";

const NOW = 1_700_000_000_000;

test("ago reads minutes, hours and days, and now under a minute", () => {
  assert.equal(ago(NOW - 20_000, NOW), "now");
  assert.equal(ago(NOW - 12 * 60_000, NOW), "12 min");
  assert.equal(ago(NOW - 3 * 3600_000, NOW), "3 h");
  assert.equal(ago(NOW - 2 * 86400_000, NOW), "2 d");
});

test("since says just now, never now ago", () => {
  assert.equal(since(NOW - 20_000, NOW), "just now");
  assert.equal(since(NOW - 12 * 60_000, NOW), "12 min ago");
});
