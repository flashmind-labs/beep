import test from "node:test";
import assert from "node:assert/strict";
import { picTile, retryIn } from "../src/agents/pic-state.ts";

const name = "image-0123456789abcdef";

test("a picture read by key draws the same tile loading and loaded", () => {
  // The row it is in is drawn from this html: a change would draw the row again.
  const asked = picTile(name, undefined, undefined);
  assert.equal(picTile(name, undefined, Promise.resolve(null)), asked);
  assert.equal(picTile(name, undefined, "data:image/jpeg;base64,AAAA"), asked);
  assert.ok(!asked.includes("src="));
  assert.ok(asked.includes('data-key="0123456789abcdef"'));
});

test("a picture the machine no longer has draws as gone", () => {
  assert.match(picTile(name, undefined, null), /class="pic none"/);
});

test("a picture sent from here draws at once", () => {
  assert.match(picTile("Pasted image 1.png", "data:image/png;base64,AAAA", undefined), /src="data:image\/png;base64,AAAA"/);
});

test("a picture that keeps failing is asked for less often", () => {
  assert.equal(retryIn(1), 2_000);
  assert.equal(retryIn(2), 4_000);
  assert.equal(retryIn(20), 60_000);
});
