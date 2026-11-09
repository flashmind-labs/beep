import { answers, bar, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage, type AgentRow } from "../src/agents/agents.ts";
import { S } from "../src/bar/core.ts";
import { sendAgentMessage } from "../src/agents/messages.ts";
import { pollAgent, stopAgentFollow } from "../src/agents/session.ts";
import { MN } from "../src/agents/workdrop.ts";

const png = { name: "shot.png", mime: "image/png", size: 8, data: "iVBORw0KGgo=", url: "data:image/png;base64,iVBORw0KGgo=" };
Object.assign(globalThis, { getSelection: () => null, getComputedStyle: () => ({ rowGap: "0" }) });
function openSession(status: string): AgentPage {
  const session: AgentRow = { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "claude", cwd: "/srv/project", status, external: false, question: null };
  const page: AgentPage = { session, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, turns: 0 };
  S.view = "agent";
  Object.assign(document, { visibilityState: "hidden" });
  Object.assign(AG, { open: { machine_id: "m1", agent: "claude", session: "s1" }, page, queue: [], note: "", att: [], flushing: false, inflight: null, stopping: null });
  Object.assign(MN, { paths: [], uploads: [], ask: null });
  answers.agent_view = { ...page, session: { ...session, status: "idle" } };
  calls.length = 0;
  delete answers.agent_steer;
  bar.innerHTML = '<div id="agbody"></div><textarea id="agmsg"></textarea>';
  return page;
}
const field = () => bar.querySelector("#agmsg")!;
const steers = () => calls.filter(c => c.cmd === "agent_steer").map(c => (c.args as { steer: unknown }).steer);
function refuse(cmd: string, why: string) { Object.defineProperty(answers, cmd, { get() { throw why; }, configurable: true }); }

test("an image continues an ended session without a prompt", async () => {
  const page = openSession("ended");
  const input = field();
  AG.att = [png];
  answers.agent_resume = { session: { ...page.session, status: "idle" }, why: "" };
  answers.agent_steer = ["picture-key"];
  await sendAgentMessage();
  const resume = calls.find(c => c.cmd === "agent_resume")!;
  assert.equal((resume.args as { prompt: unknown }).prompt, null);
  assert.deepEqual(steers(), [{ steer: { text: "", images: [{ name: png.name, mime: png.mime, data: png.data }] } }]);
  assert.deepEqual(AG.att, []);
  assert.equal(input.disabled, false);
  clearTimeout(AG.timer);
});

test("a refused image remains attached when continuing an ended session", async () => {
  const page = openSession("ended");
  const input = field();
  AG.att = [png];
  answers.agent_resume = { session: { ...page.session, status: "idle" }, why: "" };
  refuse("agent_steer", "Images unavailable");
  assert.equal(await sendAgentMessage(), false);
  assert.deepEqual(AG.att, [png]);
  assert.equal(input.disabled, false);
  clearTimeout(AG.timer);
});


test("resuming reconnects a stream that closed before its first snapshot", async t => {
  const page = openSession("ended");
  Object.assign(document, { visibilityState: "visible" });
  answers.agent_follow = null;
  await pollAgent(true);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(calls.filter(c => c.cmd === "agent_follow").length, 1);
  answers.agent_resume = { session: { ...page.session, status: "starting" }, why: "" };
  answers.agent_follow = new Promise<void>(() => {});
  t.after(() => { stopAgentFollow(); clearTimeout(AG.timer); delete answers.agent_follow; });
  field().value = "continue";
  await sendAgentMessage();
  assert.equal(calls.filter(c => c.cmd === "agent_follow").length, 2);
});
