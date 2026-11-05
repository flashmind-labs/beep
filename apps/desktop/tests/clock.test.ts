import { test } from "node:test";
import assert from "node:assert/strict";
import { clockLine, clockTitle } from "../src/machines/clock-state.ts";

test("clockLine names the size and the side", () => {
  assert.equal(clockLine(-120_000), "Clock is 2 min behind");
  assert.equal(clockLine(7_200_000), "Clock is 2 h ahead");
  assert.equal(clockLine(5_000), "Clock is 5 s ahead");
  assert.equal(clockLine(-3 * 86_400_000), "Clock is 3 d behind");
});

test("clockTitle fits the OS", () => {
  assert.match(clockTitle("linux"), /timedatectl set-ntp true\.$/);
  assert.match(clockTitle("macos"), /Date and Time settings\.$/);
  assert.match(clockTitle(null), /Linux.*macOS and Windows/);
});
