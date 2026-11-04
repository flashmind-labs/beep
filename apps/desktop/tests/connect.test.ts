import test from "node:test";
import assert from "node:assert/strict";
import { connectProblem } from "../src/center/connect.ts";

test("an unlinked or removed Mac says so and leads to the fix", () => {
  const unlinked = connectProblem("not linked to a phone yet")!;
  assert.equal(unlinked.kind, "unlinked");
  assert.equal(unlinked.view, "link");
  const revoked = connectProblem("this computer was removed from the account")!;
  assert.equal(revoked.kind, "revoked");
  assert.equal(revoked.view, "revoked");
});

test("a Mac with no server says so and has no fix to lead to", () => {
  const none = connectProblem("no server")!;
  assert.equal(none.kind, "noserver");
  assert.equal(none.action, null);
  assert.equal(none.view, null);
});

test("what waiting may fix is not a problem", () => {
  assert.equal(connectProblem(null), null);
  assert.equal(connectProblem("the server did not answer in 15 seconds"), null);
  assert.equal(connectProblem("internal error"), null);
  assert.equal(connectProblem("connecting: connection refused"), null);
});
