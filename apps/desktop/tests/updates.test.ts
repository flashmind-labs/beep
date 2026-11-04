import test from "node:test";
import assert from "node:assert/strict";
import { withoutDismissed } from "../src/center/updates.ts";

const listed = [{ id: 3 }, { id: 2 }, { id: 1 }];

test("a list from before the dismiss does not bring the update back", () => {
  const dismissed = new Set([2]);
  assert.deepEqual(withoutDismissed(listed, dismissed).map(u => u.id), [3, 1]);
  assert.ok(dismissed.has(2), "kept until the server stops listing it");
});

test("an id the server no longer lists is forgotten", () => {
  const dismissed = new Set([2, 9]);
  withoutDismissed(listed, dismissed);
  assert.deepEqual([...dismissed], [2]);
  withoutDismissed([{ id: 3 }], dismissed);
  assert.equal(dismissed.size, 0);
});
