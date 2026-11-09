import { answers } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { paneRoot, startWorktrees, worktreesReady } from "../src/agentpane/worktree.ts";
import type { Tree } from "../src/agentpane/worktree-state.ts";

test("initial file reads wait for a removed worktree to resolve to the main checkout", async () => {
  let reply: (trees: Tree[]) => void = () => {};
  answers.agent_worktrees = new Promise<Tree[]>(resolve => { reply = resolve; });
  startWorktrees({ id: "", machine_id: "build-box", agent: "codex", cwd: "/srv/app/.worktrees/removed" }, () => {});
  let ready = false;
  const waiting = worktreesReady().then(() => { ready = true; });
  await Promise.resolve();
  assert.equal(ready, false);
  reply([{ path: "/srv/app", branch: "main", head: "abc1234", own: true }]);
  await waiting;
  assert.equal(paneRoot(), "/srv/app");
  delete answers.agent_worktrees;
});
