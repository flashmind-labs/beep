import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage, type AgentRow } from "../src/agents/agents.ts";
import { S } from "../src/bar/core.ts";
import { sendAgentMessage, steerMessage } from "../src/agents/messages.ts";
import { pollAgent } from "../src/agents/session.tsx";
import { forgetAgentFollows } from "../src/agents/session-follow.ts";
import { MN, resetMentions, currentMentions, rememberMentionImages } from "../src/agents/workdrop.ts";
import { disposeComposerUi } from "../src/agents/composer-ui.tsx";
import { disposeLogShell } from "../src/agents/log-shell.tsx";
import { showSessionBody } from "./session-frame-env.ts";

const png = { name: "shot.png", mime: "image/png", size: 8, data: "iVBORw0KGgo=", url: "data:image/png;base64,iVBORw0KGgo=", original: null };
Object.assign(globalThis, { getSelection: () => null, getComputedStyle: () => ({ rowGap: "0" }) });
function openSession(status: string): AgentPage {
  disposeComposerUi();
  resetMentions();
  const previous = bar.querySelector<HTMLElement>("#agbody");
  if (previous) disposeLogShell(previous);
  const session: AgentRow = { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "claude", cwd: "/srv/project", status, external: false, question: null };
  const page: AgentPage = { session, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, turns: 0 };
  S.view = "agent";
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

test("an image continues a session that was external before resume", async () => {
  const page = openSession("ended");
  page.session.external = true;
  field().value = "Look at this";
  AG.att = [png];
  answers.agent_resume = { session: { ...page.session, external: false, status: "starting" }, why: "" };
  answers.agent_steer = ["picture-key"];
  assert.equal(await sendAgentMessage(), true);
  assert.deepEqual(steers(), [{ steer: { text: "Look at this", images: [{ name: png.name, mime: png.mime, data: png.data }] } }]);
  assert.equal(AG.page?.session.external, false);
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
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
  answers.agent_follow = null;
  // The stream ends at once, so the session is read instead, and it still says it ended.
  answers.agent_view = page;
  await pollAgent(true);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(calls.filter(c => c.cmd === "agent_follow").length, 1);
  answers.agent_resume = { session: { ...page.session, status: "starting" }, why: "" };
  answers.agent_follow = new Promise<void>(() => {});
  t.after(() => { forgetAgentFollows(); clearTimeout(AG.timer); delete answers.agent_follow; });
  field().value = "continue";
  await sendAgentMessage();
  assert.equal(calls.filter(c => c.cmd === "agent_follow").length, 2);
});

function pendingUpload() {
  let finish = () => {};
  const ready = new Promise<void>(resolve => { finish = resolve; });
  const batch = { paths: [] as string[], failed: false, ready };
  MN.pending.add(batch);
  return {
    batch,
    finish() { MN.pending.delete(batch); finish(); },
    attach(path: string) {
      batch.paths.push(path);
      MN.paths.push(path);
      MN.files.set(path, { name: "shot.png", mime: png.mime, size: png.size, data: png.data });
      AG.att.push({ ...png, original: path });
    },
  };
}

test("Send keeps the prompt visible and locked until uploads and previews finish", async () => {
  openSession("idle");
  answers.agent_steer = ["uploaded-image"];
  const upload = pendingUpload(), input = field();
  input.value = "Describe this";
  const sending = sendAgentMessage();
  assert.deepEqual(steers(), []);
  assert.equal(input.value, "Describe this");
  assert.equal(input.readOnly, true);
  assert.equal(input.getAttribute("aria-busy"), "true");
  assert.equal(bar.querySelector<HTMLButtonElement>("#agsend")?.disabled, true);
  upload.attach("/cache/shot.png");
  await sendAgentMessage();
  assert.deepEqual(steers(), []);
  upload.finish();
  await sending;
  assert.equal(steers().length, 1);
  const sent = steers()[0] as { steer: { text: string; images: unknown[] } };
  assert.match(sent.steer.text, /Describe this/);
  assert.match(sent.steer.text, /shot.png/);
  assert.equal(sent.steer.images.length, 1);
  assert.equal(input.value, "");
  assert.equal(input.readOnly, false);
  assert.equal(bar.querySelector<HTMLButtonElement>("#agsend")?.disabled, false);
  assert.deepEqual(MN.paths, []);
  assert.deepEqual(AG.att, []);
  clearTimeout(AG.timer);
});

test("failed uploads restore the captured draft and do not send partial attachments", async () => {
  openSession("idle");
  const upload = pendingUpload();
  field().value = "Describe this";
  const sending = sendAgentMessage();
  upload.attach("/cache/shot.png");
  upload.batch.failed = true;
  assert.equal(field().readOnly, true);
  upload.finish();
  await sending;
  assert.deepEqual(steers(), []);
  assert.equal(field().value, "Describe this");
  assert.equal(field().readOnly, false);
  assert.deepEqual(MN.paths, ["/cache/shot.png"]);
  assert.equal(AG.att.length, 1);
});

test("switching sessions sends uploaded attachments to the original conversation", async () => {
  openSession("idle");
  const original = currentMentions();
  const upload = pendingUpload();
  field().value = "Original session";
  const sending = sendAgentMessage();
  upload.attach("/cache/shot.png");
  rememberMentionImages();
  openSession("idle");
  AG.open = { machine_id: "m1", agent: "claude", session: "s2" };
  answers.agent_steer = ["uploaded-image"];
  field().value = "Other conversation";
  upload.finish();
  await sending;
  const sends = calls.filter(call => call.cmd === "agent_steer");
  assert.equal(sends.length, 1);
  assert.equal((sends[0].args as { session: string }).session, "s1");
  assert.equal((sends[0].args as { steer: { steer: { images: unknown[] } } }).steer.steer.images.length, 1);
  assert.equal(field().value, "Other conversation");
  assert.deepEqual(MN.paths, []);
  assert.deepEqual(AG.att, []);
  assert.deepEqual(original.paths, []);
});

test("steers wait for attachment uploads too", async () => {
  openSession("working");
  answers.agent_steer = ["uploaded-image"];
  const upload = pendingUpload();
  const sending = steerMessage(field());
  assert.deepEqual(steers(), []);
  upload.attach("/cache/shot.png");
  upload.finish();
  await sending;
  assert.equal(steers().length, 1);
  const sent = steers()[0] as { steer: { images: unknown[] } };
  assert.equal(sent.steer.images.length, 1);
  clearTimeout(AG.timer);
});
