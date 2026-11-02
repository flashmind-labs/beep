import test from "node:test";
import assert from "node:assert/strict";
import { connectProblem } from "../src/connect.ts";

test("an unlinked or removed Mac says so and leads to the fix", () => {
  const unlinked = connectProblem("not linked to a phone yet")!;
  assert.equal(unlinked.kind, "unlinked");
  assert.equal(unlinked.view, "link");
  const revoked = connectProblem("this computer was removed from the account")!;
  assert.equal(revoked.kind, "revoked");
  assert.equal(revoked.view, "revoked");
});

test("a refusal from the server shows its message", () => {
  const p = connectProblem("bad request: unknown op 7")!;
  assert.equal(p.kind, "refused");
  assert.equal(p.text, "unknown op 7");
  assert.equal(connectProblem("not found")?.kind, "refused");
});

test("what waiting may fix is not a problem", () => {
  assert.equal(connectProblem(null), null);
  assert.equal(connectProblem("the server did not answer in 15 seconds"), null);
  assert.equal(connectProblem("internal error"), null);
  assert.equal(connectProblem("connecting: connection refused"), null);
});
