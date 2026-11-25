import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage, type AgentRow } from "../src/agents/agents.ts";
import { setView } from "../src/bar/core.ts";
import { sendAgentMessage, steerMessage, stopAllWork } from "../src/agents/messages.ts";
import { MN, activateMentions, resetMentions, currentMentions } from "../src/agents/workdrop.ts";
import { disposeComposerUi } from "../src/agents/composer-ui.tsx";
import { staysInline } from "../src/agents/images.tsx";
import { disposeLogShell } from "../src/agents/log-shell.tsx";
import { showSessionBody } from "./session-frame-env.ts";
import { conversationKey } from "../src/agents/conversation-state.ts";
import type { Attached } from "../src/agents/agentqueue.ts";

const png = { name: "shot.png", mime: "image/png", size: 8, data: "iVBORw0KGgo=", url: "data:image/png;base64,iVBORw0KGgo=", original: null };
Object.assign(globalThis, { getSelection: () => null, getComputedStyle: () => ({ rowGap: "0" }) });
function openSession(status: string): AgentPage {
  disposeComposerUi();
  resetMentions();
  const previous = bar.querySelector<HTMLElement>("#agbody");
  if (previous) disposeLogShell(previous);
  const session: AgentRow = { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "claude", cwd: "/srv/project", status, external: false, question: null };
  const page: AgentPage = { session, plan: [], entries: [], earlier: 0, error: null, queued: 0, turns: 0, usage: null };
  setView("agent");
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
  Object.assign(AG, { open: { machine_id: "m1", agent: "claude", session: "s1" }, page, queue: [], note: "", att: [], flushing: false, inflight: null, stopping: null, tasks: [] });
  Object.assign(MN, { paths: [], uploads: [], ask: null });
  answers.agent_view = { ...page, session: { ...session, status: "idle" } };
  answers.agent_tasks = [];
  calls.length = 0;
  delete answers.agent_steer;
  showSessionBody(true);
  return page;
}
const field = () => bar.querySelector<HTMLTextAreaElement>("#agmsg")!;
const steers = () => calls.filter(c => c.cmd === "agent_steer").map(c => (c.args as { steer: unknown }).steer);
function refuse(cmd: string, why: string) { Object.defineProperty(answers, cmd, { get() { throw why; }, configurable: true }); }

/// What the app's `agent_steer` answers for a session that ended: the outbox has it, resumes it, and says so.
function waitsInOutbox() {
  Object.defineProperty(answers, "agent_steer", { configurable: true, get() { throw { kind: "waiting", message: "Waiting for the connection" }; } });
}

test("a message to an ended session goes to the outbox with its resume and the window awaits nothing", async () => {
  openSession("ended");
  const input = field();
  AG.att = [png];
  waitsInOutbox();
  await sendAgentMessage();
  assert.equal(calls.filter(c => c.cmd === "agent_resume").length, 0);
  const sent = calls.filter(c => c.cmd === "agent_steer").map(c => c.args as { steer: unknown; resume: unknown });
  assert.deepEqual(sent.map(call => call.steer), [{ steer: { text: "Check this image.", images: [{ name: png.name, mime: png.mime, data: png.data }] } }]);
  assert.deepEqual(sent[0].resume, { model: null, pick: null });
  assert.deepEqual(AG.att, []);
  assert.equal(input.disabled, false);
  clearTimeout(AG.timer);
});

test("a session that was external before resume still takes the message", async () => {
  const page = openSession("ended");
  page.session.external = true;
  field().value = "Look at this";
  AG.att = [png];
  waitsInOutbox();
  assert.equal(await sendAgentMessage(), true);
  const sent = calls.filter(c => c.cmd === "agent_steer").map(c => c.args as { steer: unknown; resume: unknown });
  assert.equal(sent.length, 1);
  assert.deepEqual(sent[0].steer, { steer: { text: "Look at this", images: [{ name: png.name, mime: png.mime, data: png.data }] } });
  assert.notEqual(sent[0].resume, undefined);
  clearTimeout(AG.timer);
});

test("a refused message stays in the field with its image when the session ended", async () => {
  openSession("ended");
  const input = field();
  AG.att = [png];
  refuse("agent_steer", "Images unavailable");
  assert.equal(await sendAgentMessage(), false);
  assert.deepEqual(AG.att, [png]);
  assert.equal(input.disabled, false);
  clearTimeout(AG.timer);
});

function pendingRead() {
  let finish = () => {};
  const ready = new Promise<void>(resolve => { finish = resolve; });
  const batch = { local: true, paths: [] as string[], images: [] as Attached[], ready, size: 0, done: 0 };
  MN.pending.add(batch);
  return {
    finish() { MN.pending.delete(batch); finish(); },
    picture(image: typeof png) { batch.images.push(image); AG.att.push(image); },
    attach(path: string) {
      batch.paths.push(path);
      MN.paths.push(path);
      MN.files.set(path, { name: "report.txt", mime: "text/plain", size: 5, data: "bm90ZXM=" });
    },
  };
}

test("Send waits for files still being read, then hands them to the outbox with the words", async () => {
  openSession("idle");
  answers.agent_steer = ["uploaded-image"];
  const read = pendingRead(), input = field();
  input.value = "Describe this";
  const sending = sendAgentMessage();
  assert.deepEqual(steers(), []);
  read.attach("attachment:1:report.txt");
  read.picture(png);
  read.finish();
  await sending;
  const sent = calls.filter(c => c.cmd === "agent_steer").map(c => c.args as { steer: { steer: { text: string; images: unknown[] } }; attach: unknown });
  assert.equal(sent.length, 1);
  // The words alone: the outbox adds the mention once the file is on the machine.
  assert.equal(sent[0].steer.steer.text, "Describe this");
  assert.equal(sent[0].steer.steer.images.length, 1);
  assert.deepEqual(sent[0].attach, { root: "", files: [{ name: "report.txt", mime: "text/plain", data: "bm90ZXM=" }], mentions: [] });
  assert.equal(input.value, "");
  assert.deepEqual(MN.paths, []);
  assert.deepEqual(AG.att, []);
  assert.equal(calls.filter(c => c.cmd === "agent_tree_upload").length, 0);
  clearTimeout(AG.timer);
});

test("Enter pressed again while a file is read takes neither the file nor an empty message, and a later one goes after", async () => {
  openSession("idle");
  answers.agent_steer = [];
  const read = pendingRead();
  field().value = "First";
  const first = sendAgentMessage();
  // Nothing typed and the read is claimed: no message at all.
  const again = sendAgentMessage();
  if (again) read.finish();
  assert.equal(again, undefined);
  field().value = "Second";
  const second = sendAgentMessage();
  read.attach("attachment:7:report.txt");
  read.finish();
  await Promise.all([first, second]);
  const sent = calls.filter(call => call.cmd === "agent_steer").map(call => call.args as { steer: { steer: { text: string } }; attach?: { files: unknown[] } });
  assert.deepEqual(sent.map(call => call.steer.steer.text), ["First", "Second"]);
  assert.equal(sent[0].attach?.files.length, 1);
  assert.equal(sent[1].attach, undefined);
  assert.equal(JSON.stringify(sent).includes("attachment:"), false);
});

test("leaving the conversation while its files are read still sends the message there", async () => {
  openSession("idle");
  const read = pendingRead();
  field().value = "Original session";
  const sending = sendAgentMessage();
  read.attach("attachment:2:report.txt");
  AG.open = { machine_id: "m1", agent: "claude", session: "s2" };
  answers.agent_steer = [];
  field().value = "Other conversation";
  read.finish();
  await sending;
  const sends = calls.filter(call => call.cmd === "agent_steer");
  assert.equal(sends.length, 1);
  assert.equal((sends[0].args as { session: string }).session, "s1");
  assert.equal((sends[0].args as { attach: { files: unknown[] } }).attach.files.length, 1);
  assert.equal(field().value, "Other conversation");
  assert.deepEqual(MN.paths, []);
});

test("Stop all returns text and images to its session when navigation interrupts the drain", async t => {
  const storage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: window.localStorage });
  t.after(() => {
    if (storage) Object.defineProperty(globalThis, "localStorage", storage);
    else Reflect.deleteProperty(globalThis, "localStorage");
    delete answers.outbox_drain;
  });
  const { session } = openSession("working");
  activateMentions(session);
  const original = currentMentions();
  const drafts = [{ text: "Recover this draft", images: [png], files: [], mentions: [] }];
  let finish = (_drafts: typeof drafts) => {};
  answers.outbox_drain = new Promise<typeof drafts>(resolve => { finish = resolve; });
  const stopped = stopAllWork(session);

  openSession("idle");
  AG.open = { machine_id: "m1", agent: "claude", session: "s2" };
  field().value = "Other conversation";
  finish(drafts);
  await stopped;

  const sent = calls.filter(call => call.cmd === "agent_steer");
  assert.deepEqual(sent.map(call => call.args), [{ machineId: "m1", agent: "claude", session: "s1", steer: "stop_all", seen: null }]);
  assert.equal(localStorage.getItem(`beep.promptDraft.v1.${conversationKey(session)}`), "Recover this draft");
  assert.deepEqual(original.images.map(image => image.name), [png.name]);
  assert.equal(field().value, "Other conversation");
  assert.deepEqual(AG.att, []);
});

test("Stop all preserves the outbox draft when the node refuses the stop", async t => {
  const { session } = openSession("working");
  const input = field();
  input.value = "Next draft";
  answers.outbox_drain = [{ text: "Recover this draft", images: [png], files: [], mentions: [] }];
  refuse("agent_steer", "The stop was refused");
  t.after(() => { delete answers.outbox_drain; delete answers.agent_steer; clearTimeout(AG.timer); });

  await stopAllWork(session);

  assert.equal(input.value, "Recover this draft\nNext draft");
  assert.deepEqual(AG.att.map(image => image.name), [png.name]);
  assert.equal(AG.stopping, null);
});

test("Stop all does not restore a steer delivered while the stop was pending", async t => {
  const { session } = openSession("working");
  const input = field();
  input.value = "Next draft";
  const page = AG.page!;
  page.queued = 1;
  page.queued_messages = ["Already sent"];
  let finish = () => {};
  answers.agent_steer = new Promise<void>(resolve => { finish = resolve; });
  t.after(() => { finish(); delete answers.agent_steer; clearTimeout(AG.timer); });

  const stopping = stopAllWork(session);
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(steers(), ["stop_all"]);
  // The node delivers it before it answers the stop.
  page.queued = 0;
  page.queued_messages = [];
  finish();
  await stopping;

  assert.equal(input.value, "Next draft");
});

test("steers wait for the files being read too", async () => {
  openSession("working");
  answers.agent_steer = ["uploaded-image"];
  const read = pendingRead();
  const sending = steerMessage(field());
  assert.deepEqual(steers(), []);
  read.attach("attachment:3:report.txt");
  read.picture(png);
  read.finish();
  await sending;
  assert.equal(steers().length, 1);
  const sent = steers()[0] as { steer: { images: unknown[] } };
  assert.equal(sent.steer.images.length, 1);
  clearTimeout(AG.timer);
});

test("only a small supported picture goes inline without a file copy", () => {
  const file = (size: number, type: string) => ({ size, type }) as File;
  assert.equal(staysInline(file(160 * 1024, "image/png")), true);
  assert.equal(staysInline(file(2 * 1024 * 1024, "image/png")), false);
  assert.equal(staysInline(file(1024, "image/heic")), false);
  assert.equal(staysInline(file(1024, "text/plain")), false);
});
