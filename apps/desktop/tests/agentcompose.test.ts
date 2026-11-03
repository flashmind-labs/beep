import { answers, bar, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { S } from "../src/bar/core.ts";
import { AG, type AgentPage, type AgentRow, pollAgent, sendAgentMessage, switchSessionModel } from "../src/bar/agents.ts";

(globalThis.document as { visibilityState?: string }).visibilityState = "visible";

/// The bar shows only `els`, by selector.
function show(els: Record<string, unknown>) {
  Object.assign(bar, { querySelector: (sel: string) => els[sel] ?? null });
}

const field = (value = "") => ({ value, placeholder: "", disabled: false, focus() {} });

/// Opens session s1 in `status`; polls stay off unless `polls`.
function openSession(status: string, polls = false): AgentPage {
  const session: AgentRow = { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "claude", cwd: "/srv/project", status, external: false, question: null };
  const page: AgentPage = { session, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, turns: 0 };
  S.view = "agent";
  Object.assign(AG, { open: { machine_id: "m1", agent: "claude", session: "s1" }, page, queue: [], note: "", att: [], flushing: false, inflight: null, barHidden: !polls });
  calls.length = 0;
  delete answers.agent_steer;
  return page;
}

/// `cmd` fails with `why`, as Tauri rejects, until the next `openSession`.
function refuse(cmd: string, why: string) {
  Object.defineProperty(answers, cmd, { get() { throw why; }, configurable: true, enumerable: true });
}

test("an ended session puts the queue back in the field before what is typed there", async () => {
  const page = openSession("working", true);
  const input = field("and one more");
  show({ "#agmsg": input });
  AG.queue = ["first", "second"];
  answers.agent_view = { ...page, session: { ...page.session, status: "ended" } };
  await pollAgent();
  assert.equal(input.value, "first\n\nsecond\n\nand one more");
  assert.deepEqual(AG.queue, []);
});

test("an ended session keeps the queue while no field shows", async () => {
  const page = openSession("working", true);
  show({});
  AG.queue = ["first"];
  answers.agent_view = { ...page, session: { ...page.session, status: "ended" } };
  await pollAgent();
  assert.deepEqual(AG.queue, ["first"]);
});

test("a text message that fails to send goes back in the field under the error", async () => {
  const page = openSession("idle");
  const input = field("fix the build");
  show({ "#agmsg": input });
  refuse("agent_steer", "bad request: the machine is offline");
  await sendAgentMessage();
  assert.equal(input.value, "fix the build");
  assert.equal(AG.note, "the machine is offline");
  assert.deepEqual(page.entries, []);
});

test("the model pill keeps the model that runs", async () => {
  const page = openSession("idle");
  const pill = { innerHTML: "" };
  show({ "#agmodel": pill });
  const s = page.session;

  await switchSessionModel(s, { model: "opus-x", effort: null }, "model");
  assert.match(pill.innerHTML, /opus-x/);

  // Default cannot be sent to a running session, so opus-x keeps running.
  const sent = calls.length;
  await switchSessionModel(s, { model: "", effort: null }, "model");
  assert.equal(calls.length, sent);
  assert.match(pill.innerHTML, /opus-x/);
  assert.match(AG.note, /Default/);

  refuse("agent_steer", "bad request: the agent is still starting");
  await switchSessionModel(s, { model: "sonnet-y", effort: null }, "model");
  assert.match(pill.innerHTML, /opus-x/);
  assert.doesNotMatch(pill.innerHTML, /sonnet-y/);
});
