import test from "node:test";
import assert from "node:assert/strict";
import { tokenDraft } from "../src/settings/agentaccounts.ts";

test("a pasted token is checked before it is sent", () => {
  assert.deepEqual(tokenDraft("claude", " sk-ant-oat01-abc \n"), { agent: "claude", secret: "sk-ant-oat01-abc" });
  assert.match(tokenDraft("claude", "sk-ant-api03-abc").error!, /API key/);
  assert.match(tokenDraft("codex", "").error!, /key/);
  assert.match(tokenDraft("codex", "a b").error!, /spaces/);
  assert.equal(tokenDraft(null, "x").error, "Nothing to save.");
});
