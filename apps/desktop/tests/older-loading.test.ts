import { answers, bar, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage } from "../src/agents/agents-state.ts";
import { setView } from "../src/bar/core.ts";
import { loadOlder } from "../src/agents/older.ts";

function page(earlier: number, count: number): AgentPage {
  return {
    session: { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "codex", cwd: "/srv/project", status: "idle", external: false, question: null },
    entries: Array.from({ length: count }, (_, i) => ({ kind: i === 0 ? "user" : "agent", text: "message" })),
    earlier, plan: [], error: null, queued: 0, turns: 0, usage: null,
  };
}

function start() {
  bar.innerHTML = "";
  setView("agent");
  Object.assign(AG, { open: { machine_id: "m1", agent: "codex", session: "s1" }, page: page(400, 1), older: { stop: "", reading: null } });
  calls.length = 0;
}

test("find joins the background read and waits for the whole history", async () => {
  start();
  let resolve!: (page: AgentPage) => void;
  const pending = new Promise<AgentPage>(done => { resolve = done; });
  let reads = 0;
  Object.defineProperty(answers, "agent_older", { configurable: true, get: () => ++reads === 1 ? pending : page(0, 200) });

  try {
    const background = loadOlder();
    const find = loadOlder();
    assert.equal(find, background);
    assert.equal(reads, 1);
    resolve(page(200, 200));
    await find;
    assert.equal(AG.page?.earlier, 0);
    assert.equal(AG.page?.entries.length, 401);
    assert.deepEqual(calls.filter(c => c.cmd === "agent_older").map(c => (c.args as { before: number }).before), [400, 200]);
    assert.equal(AG.older.reading, null);
  } finally { delete answers.agent_older; }
});

test("switching conversations starts a new read and discards the previous answer", async () => {
  start();
  let resolve!: (page: AgentPage) => void;
  const pending = new Promise<AgentPage>(done => { resolve = done; });
  answers.agent_older = pending;

  try {
    const previous = loadOlder();
    Object.assign(AG, { open: { machine_id: "m1", agent: "codex", session: "s2" }, page: page(20, 1), older: { stop: "", reading: null } });
    answers.agent_older = page(0, 20);
    const current = loadOlder();
    await current;
    assert.equal(AG.page?.entries.length, 21);

    resolve(page(200, 200));
    await previous;
    assert.equal(calls.filter(c => c.cmd === "agent_older").length, 2);
    assert.equal(AG.page?.entries.length, 21);
  } finally { delete answers.agent_older; }
});

test("a replaced opening page loads from its new start", async () => {
  start();
  let resolve!: (page: AgentPage) => void;
  answers.agent_older = new Promise<AgentPage>(done => { resolve = done; });

  try {
    const reading = loadOlder();
    AG.page = page(75, 1);
    answers.agent_older = page(0, 75);
    resolve(page(200, 200));
    await reading;

    assert.deepEqual(calls.filter(c => c.cmd === "agent_older").map(c => (c.args as { before: number }).before), [400, 75]);
    assert.equal(AG.page.earlier, 0);
    assert.equal(AG.page.entries.length, 76);
  } finally { delete answers.agent_older; }
});
