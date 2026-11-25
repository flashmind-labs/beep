import test from "node:test";
import assert from "node:assert/strict";
import { codexTaskRunning, tokenDraft } from "../src/settings/agentaccounts.ts";

test("a pasted token is checked before it is sent", () => {
  assert.deepEqual(tokenDraft("claude", " sk-ant-oat01-abc \n"), { agent: "claude", secret: "sk-ant-oat01-abc" });
  assert.match(tokenDraft("claude", "sk-ant-api03-abc").error!, /API key/);
  assert.match(tokenDraft("codex", "").error!, /key/);
  assert.match(tokenDraft("codex", "a b").error!, /spaces/);
  assert.equal(tokenDraft(null, "x").error, "Nothing to save.");
});

test("a processes update says whether the Codex sign-in task still runs", () => {
  const tasks = (...list: unknown[]) => ({ Process: { Tasks: { tasks: list } } });
  assert.equal(codexTaskRunning(tasks({ id: 4, command: "codex login --device-auth", state: "running" }), 4), true);
  assert.equal(codexTaskRunning(tasks({ id: 4, command: "codex login --device-auth", state: { exited: { code: 0 } } }), 4), false);
  assert.equal(codexTaskRunning(tasks({ id: 5, state: "running" }), 4), false);
  assert.equal(codexTaskRunning({ Sessions: {} }, 4), null);
  assert.equal(codexTaskRunning({ error: "refused" }, 4), null);
});
