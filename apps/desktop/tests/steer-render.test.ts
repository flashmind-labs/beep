import { answers, bar, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { S } from "../src/bar/core.ts";
import { AG, type AgentPage } from "../src/agents/agents.ts";
import * as agentlog from "../src/agents/agentlog.ts";
import { drawAgentBody } from "../src/agents/logview.ts";
import { editQueued, steerMessage } from "../src/agents/messages.ts";
import { pollAgent, stopAgentFollow } from "../src/agents/session.ts";
import { MN } from "../src/agents/workdrop.ts";

function setup() {
  globalThis.getSelection = () => null;
  globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;
  Object.assign(document, { visibilityState: "hidden" });
  S.view = "agent";
  const page: AgentPage = {
    session: { machine_id: "m1", machine_name: "build-box", id: "steer-render", agent: "claude", cwd: "/srv/project", status: "working", external: false, question: null },
    entries: [{ kind: "user", text: "Check the build" }, { kind: "other", text: "Compacting the conversation" }],
    earlier: 0, plan: [], files: [], last_message: "", error: null, queued: 0, turns: 0,
  };
  Object.assign(AG, {
    open: { machine_id: "m1", agent: "claude", session: "steer-render" }, page, log: agentlog,
    queue: [], att: [], note: "", child: null, held: 0, stopping: null,
  });
  Object.assign(MN, { paths: [], uploads: [] });
  bar.innerHTML = '<div id="agbody"></div><div id="agfield"><textarea id="agmsg"></textarea></div>';
  Object.assign(bar.querySelector("#agbody")!, { clientHeight: 800 });
  const input = bar.querySelector("#agmsg")!;
  input.value = "Use the existing fixture";
  calls.length = 0;
  return { page, input: input as unknown as HTMLTextAreaElement };
}

const settle = () => new Promise(resolve => setImmediate(resolve));

test("the first submission shows a chat row before acknowledgement and during compaction", async t => {
  const { page, input } = setup();
  let acknowledge!: (keys: string[]) => void;
  answers.agent_steer = new Promise<string[]>(resolve => { acknowledge = resolve; });
  t.after(() => { acknowledge([]); stopAgentFollow(); delete answers.agent_steer; });
  steerMessage(input);
  const pending = () => bar.querySelector("[data-pending-steer]");
  assert.equal(input.value, "");
  assert.ok(pending()?.textContent.includes("Use the existing fixture"));
  assert.ok(pending()?.textContent.includes("Sending"));
  assert.deepEqual(page.entries.map(e => e.text), ["Check the build", "Compacting the conversation"], "pending rows do not change transcript positions");
  acknowledge([]);
  await settle();
  assert.ok(pending()?.textContent.includes("Waiting for the current step"));
  page.queued = 1;
  drawAgentBody(undefined, true);
  assert.ok(pending(), "reads during compaction preserve the pending row");
  page.entries.push({ kind: "user", text: "Use the existing fixture" });
  drawAgentBody();
  assert.equal(pending(), null);
  assert.equal(bar.querySelectorAll(".dcoderen.user").filter(e => e.textContent.includes("Use the existing fixture")).length, 1);
});

test("a refused steer removes its pending row and restores the draft", async t => {
  const { input } = setup();
  let refuse!: (error: string) => void;
  answers.agent_steer = new Promise((_resolve, reject) => { refuse = reject; });
  t.after(() => { stopAgentFollow(); delete answers.agent_steer; });
  steerMessage(input);
  assert.ok(bar.querySelector("[data-pending-steer]"));
  refuse("bad request: message refused");
  await settle();
  assert.equal(bar.querySelector("[data-pending-steer]"), null);
  assert.equal(input.value, "Use the existing fixture");
  assert.ok(bar.textContent.includes("message refused"));
});

test("editing a pending steer removes its chat row and restores its text", async t => {
  const { input } = setup();
  answers.agent_steer = [];
  t.after(() => { stopAgentFollow(); delete answers.agent_steer; });
  steerMessage(input);
  await settle();
  assert.ok(bar.querySelector("[data-pending-steer]"));
  await editQueued(0, true);
  assert.equal(bar.querySelector("[data-pending-steer]"), null);
  assert.equal(input.value, "Use the existing fixture");
});

test("a pending steer returns after switching conversations without sending it again", async t => {
  const { page, input } = setup();
  t.mock.timers.enable({ apis: ["setTimeout"] });
  answers.agent_steer = [];
  answers.agent_tasks = [];
  t.after(() => {
    stopAgentFollow();
    for (const cmd of ["agent_steer", "agent_view", "agent_tasks"]) delete answers[cmd];
  });
  steerMessage(input);
  await settle();
  const waiting = { ...page, queued: 1, queued_messages: ["Use the existing fixture"] };
  const other = { ...page, session: { ...page.session, id: "other-conversation" }, queued: 0, queued_messages: [] };
  const visit = async (next: AgentPage) => {
    // Opening a different conversation clears its local composer and queue.
    Object.assign(AG, {
      open: { machine_id: next.session.machine_id, agent: next.session.agent, session: next.session.id },
      page: { ...next, queued: 0, queued_messages: [] }, queue: [], sig: "", rest: "",
    });
    await pollAgent(true);
    await settle();
  };
  answers.agent_view = other;
  await visit(other);
  assert.equal(bar.querySelector("[data-pending-steer]"), null);
  answers.agent_view = waiting;
  await visit(waiting);
  assert.ok(bar.querySelector("[data-pending-steer]")?.textContent.includes("Use the existing fixture"));
  assert.equal(calls.filter(call => call.cmd === "agent_steer").length, 1);
});

test("a send acknowledged after switching updates its original queue", async t => {
  const { input } = setup();
  let acknowledge!: (keys: string[]) => void;
  answers.agent_steer = new Promise<string[]>(resolve => { acknowledge = resolve; });
  t.after(() => { acknowledge([]); stopAgentFollow(); delete answers.agent_steer; });
  steerMessage(input);
  const original = AG.queue;
  Object.assign(AG, {
    open: { machine_id: "m1", agent: "claude", session: "other-conversation" },
    queue: [], note: "Keep this conversation's note",
  });
  acknowledge([]);
  await settle();
  assert.equal(original[0].state, "sent");
  assert.equal(AG.note, "Keep this conversation's note");
  assert.deepEqual(AG.queue, []);
});
