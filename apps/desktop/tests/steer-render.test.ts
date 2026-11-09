import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { S } from "../src/bar/core.ts";
import { AG, type AgentPage } from "../src/agents/agents.ts";
import * as agentlog from "../src/agents/agentlog.ts";
import { drawAgentBody } from "../src/agents/logview.ts";
import { disposeQueue, drawQueue, editQueued, steerMessage } from "../src/agents/messages.ts";
import { pollAgent, stopAgentFollow } from "../src/agents/session.ts";
import { askUpload, MN } from "../src/agents/workdrop.ts";
import { drawTasks } from "../src/agents/tasks.ts";
import { drawAgentAttached } from "../src/agents/images.ts";

function setup() {
  globalThis.getSelection = () => null;
  globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
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
  Object.defineProperty(bar.querySelector("#agbody")!, "clientHeight", { configurable: true, value: 800 });
  const input = bar.querySelector("#agmsg")!;
  assert.ok(input instanceof HTMLTextAreaElement);
  input.value = "Use the existing fixture";
  calls.length = 0;
  return { page, input };
}

const settle = () => new Promise(resolve => setImmediate(resolve));

test("attachment updates preserve chips and upload choices", async t => {
  const { input } = setup();
  MN.redraw = drawAgentAttached;
  t.after(() => { disposeQueue(); MN.paths = []; MN.uploads = []; MN.ask = null; MN.redraw = () => {}; });
  MN.paths = ["src/first.ts"]; drawAgentAttached();
  const first = bar.querySelector<HTMLElement>('[data-agment="0"]'); assert.ok(first); first.focus();
  MN.paths.push("src/second.ts"); drawAgentAttached();
  assert.equal(bar.querySelector('[data-agment="0"]'), first);
  assert.equal(document.activeElement, first);
  askUpload([new File(["notes"], "notes.txt")], "src", "/srv/project");
  await settle();
  const there = bar.querySelector<HTMLButtonElement>('[data-agup="there"]'); assert.ok(there);
  assert.equal(there.textContent, "Into src/");
  assert.equal(document.activeElement, there);
  assert.match(bar.querySelector('[data-agup="default"]')?.getAttribute("title") ?? "", /Deleted when this session is closed/);
  bar.querySelector<HTMLButtonElement>('[data-agup="cancel"]')?.click();
  assert.equal(MN.ask, null);
  first.click(); assert.deepEqual(MN.paths, ["src/second.ts"]); assert.equal(document.activeElement, input);
});

test("background task updates preserve existing controls and focus", t => {
  setup();
  t.after(() => { disposeQueue(); AG.tasks = []; });
  bar.querySelector("#agfield")?.insertAdjacentHTML("beforebegin", '<div id="agtasks"></div>');
  const task = { id: "first", name: "Build", description: "Build the app", state: "running", can_stop: true, output_file: null, summary: null, entry: null };
  AG.tasks = [task]; drawTasks();
  const button = bar.querySelector<HTMLButtonElement>('[data-agtask="first"]'); assert.ok(button); button.focus();
  AG.tasks = [{ ...task }, { ...task, id: "second", name: "Test" }]; drawTasks();
  assert.equal(bar.querySelector('[data-agtask="first"]'), button);
  assert.equal(document.activeElement, button);
  AG.tasks = [{ ...task, state: "completed" }, { ...task, id: "second", name: "Test" }]; drawTasks();
  assert.equal(bar.querySelector('[data-agtask="first"]'), null);
  assert.ok(bar.querySelector('[data-agtask="second"]'));
});

test("steers appear only in the composer until recorded in the transcript", async t => {
  const { page, input } = setup();
  let acknowledge!: (keys: string[]) => void;
  answers.agent_steer = new Promise<string[]>(resolve => { acknowledge = resolve; });
  t.after(() => { acknowledge([]); stopAgentFollow(); delete answers.agent_steer; });
  steerMessage(input);
  const queued = () => bar.querySelector("#agqueue");
  const transcript = () => bar.querySelector("#agbody")?.textContent ?? "";
  assert.equal(input.value, "");
  assert.ok(queued()?.textContent.includes("Use the existing fixture"));
  assert.equal(transcript().includes("Use the existing fixture"), false);
  assert.ok(queued()?.textContent.includes("sending"));
  assert.deepEqual(page.entries.map(e => e.text), ["Check the build", "Compacting the conversation"], "pending rows do not change transcript positions");
  acknowledge([]);
  await settle();
  assert.ok(queued()?.textContent.includes("after the current step"));
  page.queued = 1;
  drawAgentBody(undefined, true);
  assert.ok(queued(), "reads during compaction preserve the steering row");
  assert.equal(transcript().includes("Use the existing fixture"), false);
  page.entries.push({ kind: "user", text: "Use the existing fixture" });
  AG.queue[0].state = "delivered";
  drawAgentBody();
  assert.equal(queued(), null);
  assert.equal([...bar.querySelectorAll(".dcoderen.user")].filter(e => e.textContent.includes("Use the existing fixture")).length, 1);
});

test("a refused steer removes its steering row and restores the draft", async t => {
  const { input } = setup();
  let refuse!: (error: string) => void;
  answers.agent_steer = new Promise((_resolve, reject) => { refuse = reject; });
  t.after(() => { stopAgentFollow(); delete answers.agent_steer; });
  steerMessage(input);
  assert.ok(bar.querySelector("#agqueue"));
  refuse("bad request: message refused");
  await settle();
  assert.equal(bar.querySelector("#agqueue"), null);
  assert.equal(input.value, "Use the existing fixture");
  assert.ok(bar.textContent.includes("message refused"));
});

test("editing a pending steer removes its steering row and restores its text", async t => {
  const { input } = setup();
  answers.agent_steer = [];
  t.after(() => { stopAgentFollow(); delete answers.agent_steer; });
  steerMessage(input);
  await settle();
  assert.ok(bar.querySelector("#agqueue"));
  await editQueued(0, true);
  assert.equal(bar.querySelector("#agqueue"), null);
  assert.equal(input.value, "Use the existing fixture");
});

test("a pending steer returns after switching conversations without sending it again", async t => {
  const { page, input } = setup();
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
  assert.equal(bar.querySelector("#agqueue"), null);
  answers.agent_view = waiting;
  await visit(waiting);
  assert.ok(bar.querySelector("#agqueue")?.textContent.includes("Use the existing fixture"));
  assert.equal(bar.querySelector("#agbody")?.textContent.includes("Use the existing fixture"), false);
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


test("stream updates keep queued message controls and focus", t => {
  setup();
  t.after(() => { disposeQueue(); stopAgentFollow(); });
  const item = (text: string) => ({ how: "steer" as const, typed: text, text, paths: [], images: [], state: "sending" as "sending" | "sent", after: 0 });
  const first = item("First"), second = item("Second");
  AG.queue = [first, second];
  drawQueue();
  const edit = bar.querySelector<HTMLButtonElement>('[data-agq="1"][data-act="edit"]');
  assert.ok(edit);
  edit.focus();
  first.state = "sent";
  drawQueue();
  assert.equal(bar.querySelector('[data-agq="1"][data-act="edit"]'), edit);
  assert.equal(document.activeElement, edit);
  AG.queue.splice(0, 1);
  drawQueue();
  assert.equal(bar.querySelector('[data-agq="0"][data-act="edit"]'), edit);
  assert.equal(document.activeElement, edit);
});


test("leaving the agent disposes its queue view but keeps pending messages", async t => {
  setup();
  t.after(() => { disposeQueue(); stopAgentFollow(); });
  const item = { how: "steer" as const, typed: "Pending", text: "Pending", paths: [], images: [], state: "sent" as const, after: 0 };
  AG.queue = [item];
  drawQueue();
  assert.ok(bar.querySelector("#agqueue"));
  S.view = "chat";
  await settle();
  assert.equal(bar.querySelector("#agqueue"), null);
  assert.equal(AG.queue[0], item);
  S.view = "agent";
  drawQueue();
  assert.ok(bar.querySelector("#agqueue")?.textContent.includes("Pending"));
});


test("drawing with no agent field leaves the queued messages unchanged", t => {
  setup();
  t.after(() => { disposeQueue(); stopAgentFollow(); });
  const delivered = { how: "steer" as const, typed: "Delivered", text: "Delivered", paths: [], images: [], state: "delivered" as const, after: 0 };
  AG.queue = [delivered];
  bar.querySelector("#agfield")?.remove();
  drawQueue();
  assert.equal(AG.queue.length, 1);
  assert.equal(AG.queue[0], delivered);
});

test("suggestion updates preserve queued controls and focus", async t => {
  const { input } = setup();
  window.HTMLElement.prototype.scrollIntoView = () => {};
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
  const { bindMentionMenu } = await import("../src/agents/mentionmenu.ts");
  answers.agent_commands = [{ name: "review", description: "Review work", hint: "" }, { name: "run", description: "Run checks", hint: "" }];
  t.after(() => { disposeQueue(); delete answers.agent_commands; });
  AG.queue.push({ how: "steer", typed: "Queued", text: "Queued", paths: [], images: [], state: null, after: 0 });
  drawQueue();
  const button = bar.querySelector<HTMLButtonElement>('[data-agq="0"][data-act="edit"]'); assert.ok(button); button.focus();
  bindMentionMenu(input);
  input.value = "/"; input.setSelectionRange(1, 1); input.dispatchEvent(new window.Event("input", { bubbles: true }));
  await settle();
  assert.equal(bar.querySelectorAll('.cm-row').length, 2);
  assert.equal(bar.querySelector('[data-agq="0"][data-act="edit"]'), button);
  assert.equal(document.activeElement, button);
});

test("mode and plan controls share the queue owner without losing focus", async t => {
  const { page } = setup();
  const field = bar.querySelector("#agfield"); assert.ok(field);
  field.insertAdjacentHTML("beforeend", '<span id="agmode"></span>');
  field.insertAdjacentHTML("beforebegin", '<div id="agplan"></div>');
  page.session.question = { id: 7, text: "Approve?", detail: null, options: [{ id: "yes", name: "Approve", allow: true }, { id: "no", name: "Reject", allow: false }] };
  AG.modes = { current: "plan", modes: [{ id: "plan", name: "Plan" }, { id: "edit", name: "Edit" }], plan: "<p>Plan text</p>" };
  const { drawModes } = await import("../src/agents/modepill.ts");
  t.after(() => { disposeQueue(); AG.modes = null; });
  drawModes();
  const mode = bar.querySelector<HTMLButtonElement>("#agmodeb"), approve = bar.querySelector<HTMLButtonElement>('[data-agopt="0"]');
  assert.ok(mode && approve); approve.focus();
  AG.queue.push({ how: "steer", typed: "Queued", text: "Queued", paths: [], images: [], state: null, after: 0 });
  drawQueue();
  assert.equal(bar.querySelector("#agmodeb"), mode);
  assert.equal(bar.querySelector('[data-agopt="0"]'), approve);
  assert.equal(document.activeElement, approve);
  AG.answering = 7; drawModes();
  assert.equal(bar.querySelector('[data-agopt="0"]'), approve);
  assert.equal(approve.disabled, true);
  AG.answering = null;
});
