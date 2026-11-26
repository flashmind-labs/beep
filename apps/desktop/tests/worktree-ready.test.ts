import { answers } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { followAgent, paneRoot, startWorktrees, worktreesReady } from "../src/agentpane/worktree.ts";
import type { Tree } from "../src/agentpane/worktree-state.ts";

test("initial file reads wait for a removed worktree to resolve to the main checkout", async () => {
  let reply: (trees: Tree[]) => void = () => {};
  answers.agent_worktrees = new Promise<Tree[]>(resolve => { reply = resolve; });
  startWorktrees({ id: "", machine_id: "build-box", agent: "codex", cwd: "/srv/app/.worktrees/removed" }, () => {}, () => false);
  let ready = false;
  const waiting = worktreesReady().then(() => { ready = true; });
  await Promise.resolve();
  assert.equal(ready, false);
  reply([{ path: "/srv/app", branch: "main", head: "abc1234", own: true, created: false }]);
  await waiting;
  assert.equal(paneRoot(), "/srv/app");
  delete answers.agent_worktrees;
});

test("the pane follows a Flashmind worktree from older restored entries", async () => {
  const cwd = "/srv/app";
  const path = `${cwd}/.worktrees/fix`;
  answers.agent_worktrees = [
    { path: cwd, branch: "main", head: "abc1234", own: true, created: false },
    { path, branch: "fix", head: "abc1234", own: false, created: false },
  ] satisfies Tree[];
  const row = (path: string) => JSON.stringify({ path, current: true, available: true, bare: false });
  const latest = { kind: "agent", text: "The changes are ready." };

  try {
    startWorktrees({ id: "resumed", machine_id: "build-box", agent: "flsh", cwd }, () => {}, () => false);
    await worktreesReady();
    followAgent([latest], 2, null);
    assert.equal(paneRoot(), cwd);

    followAgent([
      { kind: "other", text: "Resumed after beep-node restarted" },
      { kind: "tool", text: `worktree [completed]\n${row(path)}` },
      latest,
    ], 0, null);
    assert.equal(paneRoot(), path);

    followAgent([{ kind: "tool", text: `worktree [completed]\n${row(cwd)}` }], 3, null);
    assert.equal(paneRoot(), cwd);
  } finally {
    delete answers.agent_worktrees;
  }
});
