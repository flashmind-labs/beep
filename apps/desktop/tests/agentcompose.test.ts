import { answers, bar, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { S } from "../src/bar/core.ts";
import { AG, type AgentPage, type AgentRow, agentKeys, drawAgents } from "../src/agents/agents.ts";
import { loadSessions } from "../src/agents/sessions.ts";
import { agentHints, pollAgent } from "../src/agents/session.ts";
import { interruptAgent, sendAgentMessage } from "../src/agents/messages.ts";
import { switchSessionModel } from "../src/agents/modelpill.ts";
import type { Waiting } from "../src/agents/agentqueue.ts";
import { MN } from "../src/agents/workdrop.ts";

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
  Object.assign(AG, { open: { machine_id: "m1", agent: "claude", session: "s1" }, page, queue: [], note: "", att: [], flushing: false, inflight: null, barHidden: !polls, stopping: null });
  Object.assign(MN, { paths: [], uploads: [], ask: null });
  calls.length = 0;
  delete answers.agent_steer;
  return page;
}

/// A queued message of plain words.
const words = (text: string): Waiting => ({ how: "queue", typed: text, text, paths: [], images: [], state: null, after: 0 });

const png = { name: "shot.png", mime: "image/png", size: 8, data: "iVBORw0KGgo=", url: "data:image/png;base64,iVBORw0KGgo=" };

/// Enter in the session's field, with Option when `alt`.
function enter(input: unknown, alt = false) {
  const e = { key: "Enter", altKey: alt, shiftKey: false, isComposing: false, target: Object.assign(input as object, { id: "agmsg" }), preventDefault() {} };
  return agentKeys(e as unknown as KeyboardEvent, false);
}

/// Lets the sends a poll started run.
const settle = () => new Promise(resolve => setImmediate(resolve));

/// The `agent_steer` calls made, by what each sent.
const steers = () => calls.filter(c => c.cmd === "agent_steer").map(c => (c.args as { steer: unknown }).steer);

/// Reads the session once as `page` says, without following it after.
async function read(page: AgentPage) {
  answers.agent_view = page;
  AG.barHidden = false;
  await pollAgent();
  AG.barHidden = true;
  clearTimeout(AG.timer);
  await settle();
}

/// `cmd` fails with `why`, as Tauri rejects, until the next `openSession`.
function refuse(cmd: string, why: string) {
  Object.defineProperty(answers, cmd, { get() { throw why; }, configurable: true, enumerable: true });
}

test("an ended session puts the queue back in the field before what is typed there", async () => {
  const page = openSession("working", true);
  const input = field("and one more");
  show({ "#agmsg": input });
  AG.queue = [words("first"), words("second")];
  answers.agent_view = { ...page, session: { ...page.session, status: "ended" } };
  await pollAgent();
  assert.equal(input.value, "first\n\nsecond\n\nand one more");
  assert.deepEqual(AG.queue, []);
});

test("an ended session keeps the queue while no field shows", async () => {
  const page = openSession("working", true);
  show({});
  AG.queue = [words("first")];
  answers.agent_view = { ...page, session: { ...page.session, status: "ended" } };
  await pollAgent();
  assert.deepEqual(AG.queue.map(q => q.text), ["first"]);
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

test("a message queued while the turn runs keeps its file and image when it goes", async () => {
  const page = openSession("working");
  const input = field("look at this log");
  show({ "#agmsg": input });
  MN.paths = [".beep/uploads/whatever.log"];
  AG.att = [png];
  assert.equal(enter(input, true), true);
  assert.equal(input.value, "");
  assert.deepEqual(MN.paths, [], "the chip goes with the message");
  assert.deepEqual(AG.att, []);
  assert.deepEqual(steers(), [], "nothing goes while the turn runs");

  await read({ ...page, session: { ...page.session, status: "idle" } });
  const [sent] = steers() as { prompt: { text: string; images: { name: string }[] } }[];
  assert.match(sent.prompt.text, /^look at this log/);
  assert.match(sent.prompt.text, /@\.beep\/uploads\/whatever\.log/);
  assert.deepEqual(sent.prompt.images.map(i => i.name), ["shot.png"]);
});

test("Enter while the turn runs steers it, with what is attached", async () => {
  const page = openSession("working");
  const input = field("use the fixture instead");
  show({ "#agmsg": input });
  MN.paths = ["tests/data/login.json"];
  AG.att = [png];
  // The key the node names the image by.
  answers.agent_steer = ["0123456789abcdef"];
  enter(input);
  await settle();
  const [sent] = steers() as { steer: { text: string; images: { name: string }[] } }[];
  assert.match(sent.steer.text, /^use the fixture instead/);
  assert.match(sent.steer.text, /@tests\/data\/login\.json/);
  assert.equal(sent.steer.images.length, 1);
  assert.equal(AG.queue[0].state, "sent");

  // A read that the node still holds it in leaves it waiting.
  await read({ ...page, queued: 1 });
  assert.equal(AG.queue[0].state, "sent");
  // Once the node no longer holds it, it was delivered.
  await read({ ...page, queued: 0 });
  assert.equal(AG.queue[0].state, "delivered");
});

test("an image attached after a steer goes with the next message, not the steer", async () => {
  openSession("working");
  const input = field("first");
  show({ "#agmsg": input });
  answers.agent_steer = [];
  enter(input);
  AG.att.push(png);
  await settle();
  assert.deepEqual(AG.queue[0].images, []);
  assert.deepEqual(AG.att.map(a => a.name), ["shot.png"]);
});

test("Esc stops at once and sends steers not delivered with the queue", async () => {
  const page = openSession("working");
  show({ "#agmsg": field() });
  AG.queue = [{ ...words("steer me"), how: "steer", state: "sent", after: 0 }, words("then this")];
  assert.equal(interruptAgent(), true);
  assert.equal(AG.stopping, "s1");
  assert.doesNotMatch(agentHints(), /interrupt/, "already stopping");
  await settle();
  assert.deepEqual(steers(), ["cancel"]);
  assert.deepEqual(AG.queue.map(q => q.how), ["queue", "queue"]);

  calls.length = 0;
  await read({ ...page, session: { ...page.session, status: "idle" } });
  assert.equal(AG.stopping, null);
  assert.deepEqual(steers(), [{ prompt: { text: "steer me\n\nthen this" } }], "in the order typed");
});

test("replacing the shared session list does not change what a drawn row opens", async () => {
  const row = (id: string) => ({ machine_id: "m1", machine_name: "build-box", id, agent: "claude", cwd: "/srv/project", status: "idle", external: false, question: null });
  answers.agent_sessions = [row("a1"), row("a2")];
  S.view = "agents";
  AG.sel = 0;
  await loadSessions();
  drawAgents(false);
  // Another page reads a shorter list; this one is not redrawn.
  answers.agent_sessions = [row("a2")];
  await loadSessions();
  // The fake bar cannot draw the session view; what matters is which session it set to open.
  try { agentKeys({ key: "Enter", preventDefault() {} } as KeyboardEvent, false); } catch {}
  assert.equal(AG.open?.session, "a1");
});
