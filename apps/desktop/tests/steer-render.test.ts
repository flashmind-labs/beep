import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { S } from "../src/bar/core.ts";
import { AG, type AgentPage } from "../src/agents/agents.ts";
import { drawAgentBody } from "../src/agents/logview.ts";
import { disposeLogShell } from "../src/agents/log-shell.tsx";
import { answerAgentQuestion, answerFirst, questionKey, disposeQueue, drawQueue, editQueued, steerMessage } from "../src/agents/messages.ts";
import { pollAgent } from "../src/agents/session.ts";
import { forgetAgentFollows } from "../src/agents/session-follow.ts";
import { askUpload, MN } from "../src/agents/workdrop.ts";
import { drawTasks } from "../src/agents/tasks.ts";
import { drawAgentAttached } from "../src/agents/images.ts";
import { createElement } from "react";
import { FieldControl, showAgentControl } from "../src/agents/composer-ui.tsx";
import { flushSync } from "react-dom";
import { createDocumentView } from "../src/ui/document-root.tsx";
import { AgentPlan } from "../src/agents/mode-view.tsx";
import { showSessionBody } from "./session-frame-env.ts";

function setup() {
  const previous = bar.querySelector<HTMLElement>("#agbody");
  if (previous) disposeLogShell(previous);
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
    open: { machine_id: "m1", agent: "claude", session: "steer-render" }, page,
    queue: [], att: [], note: "", child: null, held: 0, stopping: null,
  });
  Object.assign(MN, { paths: [], uploads: [] });
  showSessionBody(true);
  const input = bar.querySelector("#agmsg")!;
  assert.ok(input instanceof HTMLTextAreaElement);
  input.value = "Use the existing fixture";
  calls.length = 0;
  return { page, input };
}

const settle = () => new Promise(resolve => setImmediate(resolve));

test("agent prompt keeps context before the user and retains its element", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  page.entries = [{ kind: "user", text: '<beep-context>\nContext\n</beep-context>\n<from-session id="one" name="Example">\nRead this\n</from-session>\nHello', html: "<p>Hello</p>" }, { kind: "agent", text: "Reply" }];
  drawAgentBody();
  const prompt = bar.querySelector<HTMLElement>(".dcoderen.user.md"), context = bar.querySelector(".beepctx");
  assert.ok(prompt && context); assert.equal(context.compareDocumentPosition(prompt) & Node.DOCUMENT_POSITION_FOLLOWING, Node.DOCUMENT_POSITION_FOLLOWING);
  const message = bar.querySelector(".fromsession"); assert.ok(message); assert.match(message.textContent ?? "", /Example.*Read this/);
  assert.equal(context.compareDocumentPosition(message) & Node.DOCUMENT_POSITION_FOLLOWING, Node.DOCUMENT_POSITION_FOLLOWING);
  drawAgentBody(undefined, true); assert.equal(bar.querySelector(".dcoderen.user.md"), prompt);
});

test("child replies keep their action and message during refresh", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  page.entries = [{ kind: "user", text: '<beep-child id="child@box" name="Helper" state="asking">\nPlease check this\n</beep-child>\nContinue' }, { kind: "agent", text: "Reply" }];
  drawAgentBody();
  const card = bar.querySelector<HTMLElement>(".kidreply"), action = card?.querySelector<HTMLButtonElement>('[data-kid="child@box"]');
  assert.ok(card && action); assert.match(card.textContent ?? "", /Helper.*Waiting on you.*Please check this/);
  action.focus(); drawAgentBody(undefined, true);
  assert.equal(bar.querySelector(".kidreply"), card); assert.equal(document.activeElement, action);
});

test("child message headers unwrap session prompts and preserve formatted replies", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  page.entries = [{ kind: "user", text: '<beep-child id="child@build-box" name="&lt;from-session id=&quot;source&quot; name=&quot;Review login&quot;&gt;Check tests&lt;/from-session&gt;" state="finished">Passed</beep-child>', child_html: ['<p><strong>Passed</strong>.</p><table><tbody><tr><td>Login</td><td>12 passed</td></tr></tbody></table>'] }];
  drawAgentBody();
  const card = bar.querySelector(".kidreply"); assert.ok(card);
  const action = card.querySelector("[data-kid]"); assert.ok(action);
  assert.equal(action.querySelector("b")?.textContent, "Review login");
  assert.equal(action.querySelector(".kl")?.textContent, "Session reply");
  assert.equal(action.querySelector(".ks")?.textContent, "Done");
  assert.equal(action.getAttribute("title"), "Open Review login");
  assert.equal(action.textContent.includes("<from-session"), false);
  assert.equal(card.querySelector(".sessionbody strong")?.textContent, "Passed");
  assert.equal(card.querySelector("table td")?.textContent, "Login");
});

test("session status updates preserve the status element for writable and read-only sessions", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  bar.insertAdjacentHTML("afterbegin", '<span id="agstate"></span>');
  const stateHost = bar.querySelector<HTMLElement>("#agstate"); assert.ok(stateHost);
  const head = createDocumentView(stateHost);
  t.after(() => head.unmount());
  flushSync(() => head.render(createElement(FieldControl, { slot: "state" })));
  const status = () => bar.querySelector("#agstate .dstate");
  drawAgentBody();
  const first = status(); assert.ok(first); assert.equal(first.textContent, "Working");
  drawAgentBody(undefined, true); assert.equal(status(), first);
  page.session.status = "asking"; drawAgentBody(undefined, true);
  assert.equal(status(), first); assert.equal(first.textContent, "Waiting for you");
  bar.querySelector("#agfield")?.remove();
  page.session.external = true; drawAgentBody();
  const readonly = status(); assert.ok(readonly); drawAgentBody(undefined, true);
  assert.equal(status(), readonly);
});

test("unchanged primitive control props skip rendering without hiding later updates", t => {
  setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  const host = bar.querySelector<HTMLElement>("#agmode"); assert.ok(host);
  let renders = 0;
  const Probe = ({ value }: { value: string }) => { renders++; return createElement("span", null, value); };
  showAgentControl("mode", createElement(Probe, { value: "same" }));
  showAgentControl("mode", createElement(Probe, { value: "same" }));
  assert.equal(renders, 1);
  showAgentControl("mode", createElement(Probe, { value: "changed" }));
  assert.equal(renders, 2);
  assert.equal(host.textContent, "changed");
  const data = { value: "before" };
  const Mutable = ({ item }: { item: { value: string } }) => createElement("span", null, item.value);
  showAgentControl("mode", createElement(Mutable, { item: data }));
  data.value = "after";
  showAgentControl("mode", createElement(Mutable, { item: data }));
  assert.equal(host.textContent, "after");
});

test("streaming agent replies keep their element across updates", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  page.entries = [{ kind: "user", text: "Explain" }, { kind: "agent", text: "First", html: "<p>First</p>" }];
  drawAgentBody();
  const reply = bar.querySelector<HTMLElement>(".dcoderen.agent"); assert.ok(reply);
  page.entries[1] = { kind: "agent", text: "First update", html: "<p>First update</p>" };
  drawAgentBody(undefined, true);
  assert.equal(bar.querySelector(".dcoderen.agent"), reply);
  assert.match(reply.textContent ?? "", /First update/);
});

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

test("background commands show as one line that keeps its focus as the count changes", t => {
  setup();
  t.after(() => { disposeQueue(); AG.tasks = []; });
  const task = { id: "first", name: "Build", description: "Build the app", state: "running", can_stop: true, output_file: null, summary: null, entry: null };
  AG.tasks = [task]; drawTasks();
  const line = bar.querySelector<HTMLButtonElement>(".ag-tasks"); assert.ok(line); line.focus();
  assert.equal(line.textContent, "1 command running");
  assert.equal(bar.querySelector("[data-agtask]"), null, "rows with Stop live in the Running tab");
  AG.tasks = [{ ...task }, { ...task, id: "second", name: "Test" }]; drawTasks();
  assert.equal(bar.querySelector(".ag-tasks"), line);
  assert.equal(line.textContent, "2 commands running");
  assert.equal(document.activeElement, line);
  AG.tasks = [{ ...task, state: "completed" }, { ...task, id: "second", name: "Test" }]; drawTasks();
  assert.equal(line.textContent, "1 command running");
  AG.tasks = []; drawTasks();
  assert.equal(bar.querySelector(".ag-tasks"), null);
});

test("steers appear only in the composer until recorded in the transcript", async t => {
  const { page, input } = setup();
  let acknowledge!: (keys: string[]) => void;
  answers.agent_steer = new Promise<string[]>(resolve => { acknowledge = resolve; });
  t.after(() => { acknowledge([]); forgetAgentFollows(); delete answers.agent_steer; });
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
  t.after(() => { forgetAgentFollows(); delete answers.agent_steer; });
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
  t.after(() => { forgetAgentFollows(); delete answers.agent_steer; });
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
    forgetAgentFollows();
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
  t.after(() => { acknowledge([]); forgetAgentFollows(); delete answers.agent_steer; });
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
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  const item = (text: string) => ({ typed: text, text, paths: [], images: [], state: "sending" as "sending" | "sent", after: 0 });
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

test("unchanged queue snapshots keep focused controls, then show changed content", t => {
  setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  const item = { typed: "First", text: "First", paths: [], images: [], state: "sent" as const, after: 0 };
  AG.queue = [item];
  drawQueue();
  const edit = bar.querySelector<HTMLButtonElement>('[data-agq="0"][data-act="edit"]'); assert.ok(edit);
  edit.focus();
  AG.queue = [{ ...item }];
  drawQueue();
  assert.equal(bar.querySelector('[data-agq="0"][data-act="edit"]'), edit);
  assert.equal(document.activeElement, edit);
  AG.queue = [{ ...item, typed: "Updated", text: "Updated" }];
  drawQueue();
  assert.match(bar.querySelector("#agqueue")?.textContent ?? "", /Updated/);
});


test("leaving the agent disposes its queue view but keeps pending messages", async t => {
  setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  const item = { typed: "Pending", text: "Pending", paths: [], images: [], state: "sent" as const, after: 0 };
  AG.queue = [item];
  drawQueue();
  assert.ok(bar.querySelector("#agqueue"));
  S.view = "waiting";
  await settle();
  assert.equal(bar.querySelector("#agqueue"), null);
  assert.equal(AG.queue[0], item);
  S.view = "agent";
  drawQueue();
  assert.ok(bar.querySelector("#agqueue")?.textContent.includes("Pending"));
});


test("drawing with no agent field leaves the queued messages unchanged", t => {
  setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  const delivered = { typed: "Delivered", text: "Delivered", paths: [], images: [], state: "delivered" as const, after: 0 };
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
  AG.queue.push({ typed: "Queued", text: "Queued", paths: [], images: [], state: null, after: 0 });
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
  AG.queue.push({ typed: "Queued", text: "Queued", paths: [], images: [], state: null, after: 0 });
  drawQueue();
  assert.equal(bar.querySelector("#agmodeb"), mode);
  assert.equal(bar.querySelector('[data-agopt="0"]'), approve);
  assert.equal(document.activeElement, approve);
  AG.answering = 7; drawModes();
  assert.equal(bar.querySelector('[data-agopt="0"]'), approve);
  assert.equal(approve.disabled, true);
  AG.answering = null;
});

test("log shell retains the transcript and defers a focused approval update", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  page.session.question = { id: 1, text: "Run this?", detail: "command", options: [{ id: "yes", name: "Allow", allow: true }, { id: "no", name: "Deny", allow: false }] };
  drawAgentBody();
  const body = bar.querySelector<HTMLElement>("#agbody"), log = bar.querySelector(".dcoderlog");
  const button = bar.querySelector<HTMLButtonElement>("[data-agopt='0']");
  assert.ok(body && log && button); button.focus();
  page.session.question.text = "Run the updated command?";
  page.error = "A separate error";
  drawAgentBody(undefined, true);
  assert.match(bar.querySelector(".q")?.textContent ?? "", /Run this\?/);
  assert.equal(document.activeElement, button);
  assert.match(bar.querySelector(".derrcard")?.textContent ?? "", /A separate error/);
  button.blur(); drawAgentBody(undefined, true);
  assert.match(bar.querySelector(".q")?.textContent ?? "", /Run the updated command\?/);
  assert.equal(bar.querySelector(".dcoderlog"), log);
  body.remove(); bar.prepend(body); drawAgentBody(undefined, true);
  assert.equal(bar.querySelector(".dcoderlog"), log);
  disposeLogShell(body);
  assert.equal(body.querySelector(".dagmain"), null);
});

test("replacing a transcript with a starting state clears virtualized rows", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  drawAgentBody();
  assert.ok(bar.querySelector(".dcoderen"));
  page.entries = []; page.session.status = "starting";
  drawAgentBody();
  assert.equal(bar.querySelector(".dcoderlog"), null);
  assert.equal(bar.querySelector(".dcoderen"), null);
  assert.match(bar.querySelector(".dagmain")?.textContent ?? "", /Starting the agent/);
  page.entries = [{ kind: "user", text: "New" }]; drawAgentBody();
  assert.match(bar.querySelector(".dcoderlog")?.textContent ?? "", /New/);
});


test("stale displayed approvals cannot answer a replacement question or changed options", async t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  page.session.question = { id: 1, text: "Run this?", detail: null, options: [{ id: "yes", name: "Allow", allow: true }] };
  drawAgentBody();
  const old = bar.querySelector<HTMLButtonElement>("[data-agopt='0']"); assert.ok(old); old.focus();
  page.session.question = { id: 2, text: "Delete this?", detail: null, options: [{ id: "delete", name: "Delete", allow: true }] };
  drawAgentBody(undefined, true);
  await answerAgentQuestion(0, old.dataset.agquestion ?? "");
  assert.equal(calls.filter(call => call.cmd === "center_answer_agent").length, 0);
  const current = bar.querySelector<HTMLButtonElement>("[data-agopt='0']"); assert.ok(current); current.focus();
  page.session.question.options[0] = { id: "other", name: "Other", allow: true };
  drawAgentBody(undefined, true);
  assert.equal(answerFirst(true), true);
  assert.equal(calls.filter(call => call.cmd === "center_answer_agent").length, 0);
  const updated = bar.querySelector<HTMLButtonElement>("[data-agopt='0']"); assert.ok(updated);
  await answerAgentQuestion(0, updated.dataset.agquestion ?? "");
  const sent = calls.filter(call => call.cmd === "center_answer_agent");
  assert.equal(sent.length, 1);
  assert.equal((sent[0].args as { question: number; reply: string }).question, 2);
  assert.equal((sent[0].args as { question: number; reply: string }).reply, "other");
});


test("plan approval shortcuts validate the displayed question", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  const first = { id: 4, text: "Approve plan?", detail: null, options: [{ id: "allow", name: "Allow", allow: true }] };
  page.session.question = first;
  showAgentControl("plan", createElement(AgentPlan, { harness: "Agent", plan: "Plan", question: first, approvalKey: questionKey(first, AG.open!), answering: null }));
  page.session.question = { ...first, id: 5 };
  assert.equal(answerFirst(true), true);
  assert.equal(calls.filter(call => call.cmd === "center_answer_agent").length, 0);
});

test("approval actions use the active session and ignore cached controls", async t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  const question = { id: 7, text: "Continue?", detail: null, options: [{ id: "yes", name: "Allow", allow: true }] };
  page.session.question = question;
  drawAgentBody();
  const old = bar.querySelector<HTMLButtonElement>("[data-agopt='0']"); assert.ok(old);
  const oldKey = old.dataset.agquestion ?? "";
  const hidden = document.createElement("div"); hidden.hidden = true; hidden.append(old.cloneNode(true)); bar.prepend(hidden);
  AG.open = { machine_id: "m2", agent: "claude", session: "other" };
  page.session.machine_id = "m2"; page.session.id = "other";
  await answerAgentQuestion(0, oldKey);
  assert.equal(calls.filter(call => call.cmd === "center_answer_agent").length, 0);
  assert.equal(answerFirst(true), true);
  const sent = calls.filter(call => call.cmd === "center_answer_agent");
  assert.equal(sent.length, 1);
  assert.equal((sent[0].args as { machineId: string }).machineId, "m2");
});

test("composer slots keep queue and attachments ordered and survive disposal", t => {
  setup();
  t.after(() => { disposeQueue(); MN.paths = []; });
  AG.queue = [{ typed: "Queued", text: "Queued", paths: [], images: [], state: "sent", after: 0 }];
  MN.paths = ["src/file.ts"];
  drawQueue(); drawAgentAttached();
  const queue = bar.querySelector("#agqueue"), attachments = bar.querySelector("#agatt"), field = bar.querySelector("#agfield");
  assert.ok(queue && attachments && field);
  assert.ok(queue.compareDocumentPosition(attachments) & Node.DOCUMENT_POSITION_FOLLOWING);
  assert.ok(attachments.compareDocumentPosition(field) & Node.DOCUMENT_POSITION_FOLLOWING);
  disposeQueue();
  assert.equal(bar.querySelector("#agqueue"), null);
  assert.equal(bar.querySelector("#agatt"), null);
  assert.ok(bar.querySelector("[data-agent-queue-slot]"));
  assert.ok(bar.querySelector("[data-agent-attachment-slot]"));
  drawQueue(); drawAgentAttached();
  assert.ok(bar.querySelector("#agqueue"));
  assert.ok(bar.querySelector("#agatt"));
});

const safetyBlock = "This request was blocked by our safety systems. Reason: Potentially unintended activity.";

test("conversation blocks stay separate from formatted partial output and survive redraws", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  Object.assign(globalThis, { NodeFilter: window.NodeFilter, Text: window.Text });
  page.session.status = "idle";
  page.entries = [{ kind: "user", text: "Push the changes" }, { kind: "agent", text: `The fixes are committed.${safetyBlock}`, html: `<p>The fixes are <strong>committed</strong>.${safetyBlock}</p>` }];
  drawAgentBody();
  assert.equal(bar.querySelector(".dcoderen.agent strong")?.textContent, "committed");
  assert.equal(bar.querySelector(".dcoderen.agent")?.textContent, "The fixes are committed.");
  assert.equal(bar.querySelector('[role="alert"] b')?.textContent, "Conversation blocked");
  assert.equal(bar.querySelector('[role="alert"] span')?.textContent, safetyBlock);
  drawAgentBody(undefined, true);
  assert.equal(bar.querySelectorAll('[role="alert"]').length, 1);
});

test("standalone provider blocks use an error card without an empty assistant reply", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  Object.assign(globalThis, { NodeFilter: window.NodeFilter, Text: window.Text });
  page.entries = [{ kind: "user", text: "Continue" }, { kind: "agent", text: safetyBlock }];
  drawAgentBody();
  assert.equal(bar.querySelector(".dcoderen.agent"), null);
  assert.equal(bar.querySelector('[role="alert"] span')?.textContent, safetyBlock);
});

test("quoted safety notices remain assistant content", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  Object.assign(globalThis, { NodeFilter: window.NodeFilter, Text: window.Text });
  for (const html of [`<blockquote><p>${safetyBlock}</p></blockquote>`, `<pre><code>${safetyBlock}</code></pre>`, `<p>Example: <code>${safetyBlock}</code></p>`]) {
    page.entries = [{ kind: "user", text: "Explain this error" }, { kind: "agent", text: safetyBlock, html }];
    drawAgentBody(undefined, true);
    assert.equal(bar.querySelector('[role="alert"]'), null);
    assert.match(bar.querySelector(".dcoderen.agent")?.textContent ?? "", /Potentially unintended activity/);
  }
});

test("structured conversation blocks and other failed turns use accessible errors", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  page.error = "misalignment_policy_violation";
  drawAgentBody();
  assert.equal(bar.querySelector('[role="alert"] b')?.textContent, "Conversation blocked");
  page.error = "Connection lost";
  drawAgentBody(undefined, true);
  assert.equal(bar.querySelector('[role="alert"] b')?.textContent, "The turn did not finish");
});
