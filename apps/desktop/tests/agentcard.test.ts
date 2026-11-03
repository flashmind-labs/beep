import test from "node:test";
import assert from "node:assert/strict";
import { cardKey, parseAgentReport } from "../src/agentcard.ts";

const working = [
  "claude session a3f9c1 in /srv/project/atlas: working, 2 turns, 1 prompts waiting",
  "Title: Fix the login test",
  "Doing: Running the auth tests",
  "Plan:",
  "  [done] Find why the login test fails",
  "  [now] Run the auth tests",
  "  [todo] Push the branch fix-login",
  "Files changed: src/auth/session.ts, tests/login.test.ts",
  "Still working after 90 s.",
].join("\n");

test("a code_agent result names its session, plan and files", () => {
  const r = parseAgentReport(working);
  assert.ok(r);
  assert.equal(r.agent, "claude");
  assert.equal(r.id, "a3f9c1");
  assert.equal(r.cwd, "/srv/project/atlas");
  assert.equal(r.status, "working");
  assert.equal(r.title, "Fix the login test");
  assert.equal(r.doing, "Running the auth tests");
  assert.deepEqual(r.plan, [
    { content: "Find why the login test fails", status: "completed" },
    { content: "Run the auth tests", status: "in_progress" },
    { content: "Push the branch fix-login", status: "pending" },
  ]);
  assert.deepEqual(r.files, ["src/auth/session.ts", "tests/login.test.ts"]);
  assert.equal(r.question, null);
  assert.equal(cardKey(r), "claude:a3f9c1");
});

test("a question and an ended session are read", () => {
  const asking = parseAgentReport("codex session s1 in /srv/a b: asking, 1 turn\nQuestion 3: Run git push?\n  git push origin fix\nOptions: 1. Yes (allow)");
  assert.equal(asking?.cwd, "/srv/a b");
  assert.equal(asking?.status, "asking");
  assert.equal(asking?.question, "Run git push?");
  const ended = parseAgentReport("claude session s2 in /srv/a: ended (failed: exit 1, no output), 3 turns");
  assert.equal(ended?.cwd, "/srv/a");
  assert.equal(ended?.status, "ended");
  const lost = parseAgentReport("claude session s3 in /srv/a: lost track of it; it may still run, check its status, 3 turns");
  assert.equal(lost?.status, "unknown");
});

test("the agent's last message is not read as the card's fields", () => {
  const r = parseAgentReport("claude session s1 in /srv/a: idle, 1 turn\nLast turn end_turn. Its last message:\nFiles changed: made up\nPlan:\n  [done] made up");
  assert.deepEqual(r?.files, []);
  assert.deepEqual(r?.plan, []);
});
