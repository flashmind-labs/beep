import { answers, bar, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage } from "../src/agents/agents.ts";
import { S } from "../src/bar/core.ts";
import { loadOlder } from "../src/agents/older.ts";

function page(earlier: number, count: number, prompt = true): AgentPage {
  return {
    session: { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "codex", cwd: "/srv/project", status: "idle", external: false, question: null },
    entries: Array.from({ length: count }, (_, i) => ({ kind: prompt && i === 0 ? "user" : "agent", text: "message" })),
    earlier, plan: [], files: [], last_message: "", error: null, queued: 0, turns: 0, usage: null,
  };
}

function start() {
  bar.innerHTML = "";
  S.view = "agent";
  Object.assign(AG, { open: { machine_id: "m1", agent: "codex", session: "s1" }, page: page(400, 1, false), older: { loading: false, stop: "", gaveUp: false } });
  calls.length = 0;
}

test("find waits for a prompt read and then reads the remaining history", async () => {
  start();
  let resolve!: (page: AgentPage) => void;
  const pending = new Promise<AgentPage>(done => { resolve = done; });
  let reads = 0;
  Object.defineProperty(answers, "agent_older", { configurable: true, get: () => ++reads === 1 ? pending : page(0, 200) });

  try {
    const prompt = loadOlder("prompt");
    const all = loadOlder("all");
    assert.equal(reads, 1);
    resolve(page(200, 200));
    await Promise.all([prompt, all]);
    assert.equal(AG.page?.earlier, 0);
    assert.equal(AG.page?.entries.length, 401);
    assert.deepEqual(calls.filter(c => c.cmd === "agent_older").map(c => (c.args as { before: number }).before), [400, 200]);
    assert.equal(AG.older.loading, false);
  } finally { delete answers.agent_older; }
});

test("a waiting find does not start reading a different conversation", async () => {
  start();
  let resolve!: (page: AgentPage) => void;
  const pending = new Promise<AgentPage>(done => { resolve = done; });
  answers.agent_older = pending;

  try {
    const prompt = loadOlder("prompt");
    const all = loadOlder("all");
    AG.open = { machine_id: "m1", agent: "codex", session: "s2" };
    resolve(page(200, 200));
    await Promise.all([prompt, all]);
    assert.equal(calls.filter(c => c.cmd === "agent_older").length, 1);
    assert.equal(AG.page?.earlier, 400);
  } finally { delete answers.agent_older; }
});
