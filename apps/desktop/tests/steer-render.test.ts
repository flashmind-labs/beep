import { answers, bar, calls, emit } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { setView } from "../src/stores/bar/core.ts";
import { AG, type AgentModes, type AgentPage, type AgentTask, type OpenAgent } from "../src/stores/agents/agents-state.ts";
import { drawAgentBody } from "../src/parts/session/logview.tsx";
import { disposeLogShell } from "../src/parts/session/log-shell.tsx";
import { answerAgentQuestion, answerFirst, drawQueue, editQueued, steerMessage } from "../src/parts/session/messages.ts";
import { disposeQueue } from "../src/parts/session/composer-ui.tsx";
import { questionKey } from "../src/parts/session/open-state.ts";
import { pollAgent } from "../src/parts/session/session-follow.ts";
import { forgetAgentFollows } from "../src/parts/session/session-follow.ts";
import { askUpload, MN } from "../src/stores/agents/mentions.ts";
import { drawTasks } from "../src/parts/session/tasks.tsx";
import { transcriptKey, transcripts } from "../src/stores/agents/transcript-cache-state.ts";
import { drawAgentAttached } from "../src/parts/session/attachments-view.tsx";
import { createElement } from "react";
import { FieldControl, showAgentControl } from "../src/parts/session/composer-ui.tsx";
import { flushSync } from "react-dom";
import { createDocumentView } from "../src/ui/document-root.tsx";
import { AgentPlan } from "../src/parts/session/mode-view.tsx";
import { showSessionBody } from "./session-frame-env.ts";

function setup() {
  const previous = bar.querySelector<HTMLElement>("#agbody");
  if (previous) disposeLogShell(previous);
  globalThis.getSelection = () => null;
  globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
  setView("agent");
  const page: AgentPage = {
    session: { machine_id: "m1", machine_name: "build-box", id: "steer-render", agent: "claude", cwd: "/srv/project", status: "working", external: false, question: null },
    entries: [{ kind: "user", text: "Check the build" }, { kind: "other", text: "Compacting the conversation" }],
    earlier: 0, plan: [], error: null, queued: 0, turns: 0, usage: null,
  };
  Object.assign(AG, {
    open: { machine_id: "m1", agent: "claude", session: "steer-render" }, page,
    outbox: [], att: [], note: "", child: null, held: 0, stopping: null,
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

function opened(): OpenAgent {
  assert.ok(AG.open);
  return AG.open;
}

const setTasks = (tasks: AgentTask[]) => transcripts.setTasks(transcriptKey(opened()), tasks);
const setModes = (modes: AgentModes | null) => transcripts.setModes(transcriptKey(opened()), modes);

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

test("session status shows the latest state for writable and read-only sessions", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  bar.insertAdjacentHTML("afterbegin", '<span id="agstate"></span>');
  const stateHost = bar.querySelector<HTMLElement>("#agstate"); assert.ok(stateHost);
  const head = createDocumentView(stateHost);
  t.after(() => head.unmount());
  flushSync(() => head.render(createElement(FieldControl, { slot: "state" })));
  drawAgentBody();
  assert.equal(stateHost.textContent, "Working");
  page.session.status = "asking";
  drawAgentBody(undefined, true);
  assert.equal(stateHost.textContent, "Waiting for you");

  bar.querySelector("#agfield")?.remove();
  page.session.external = true;
  page.session.status = "ended";
  drawAgentBody();
  assert.equal(stateHost.textContent, "Ended");
});

test("each prompt keeps its final reply copy control as the session grows", t => {
  const { page } = setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  page.session.status = "idle";
  page.entries = [{ kind: "user", text: "Explain" }, { kind: "agent", text: "First" }];
  drawAgentBody();
  const first = bar.querySelector(".dcoderen.agent");
  assert.ok(first);
  const firstCopy = first.querySelector(".reply-copy");
  assert.ok(firstCopy);

  page.entries.push({ kind: "user", text: "Continue" });
  drawAgentBody(undefined, true);
  assert.ok(first.querySelector(".reply-copy"));

  page.entries.push({ kind: "agent", text: "Second" });
  drawAgentBody(undefined, true);
  assert.equal(bar.querySelector(".dcoderen.agent"), first);
  assert.equal(first.querySelector(".reply-copy"), firstCopy);
  assert.equal(bar.querySelectorAll(".reply-copy").length, 2);
  assert.equal(bar.querySelectorAll(".reply-copy")[1]?.closest(".dcoderen.agent")?.textContent, "Second");
});

test("attachment updates preserve the file-count control and upload choices", async t => {
  setup();
  MN.redraw = drawAgentAttached;
  t.after(() => { disposeQueue(); MN.paths = []; MN.uploads = []; MN.ask = null; MN.redraw = () => {}; });
  MN.paths = ["src/first.ts"]; drawAgentAttached();
  const first = bar.querySelector<HTMLElement>(".attachment-count"); assert.ok(first); first.focus();
  assert.equal(first.textContent, "1 file");
  MN.paths.push("src/second.ts"); drawAgentAttached();
  assert.equal(bar.querySelector(".attachment-count"), first);
  assert.equal(first.textContent, "2 files");
  assert.equal(document.activeElement, first);
  askUpload([new File(["notes"], "notes.txt")], "src", "/srv/project");
  await settle();
  const there = bar.querySelector<HTMLButtonElement>('[data-agup="there"]'); assert.ok(there);
  assert.equal(there.textContent, "Into src/");
  assert.equal(document.activeElement, there);
  assert.match(bar.querySelector('[data-agup="default"]')?.getAttribute("title") ?? "", /Deleted when this session is closed/);
  bar.querySelector<HTMLButtonElement>('[data-agup="cancel"]')?.click();
  assert.equal(MN.ask, null);
  assert.deepEqual(MN.paths, ["src/first.ts", "src/second.ts"]);
});

test("background commands show as one line that keeps its focus as the count changes", t => {
  setup();
  t.after(() => { disposeQueue(); setTasks([]); });
  const task = { id: "first", name: "Build", description: "Build the app", state: "running", can_stop: true, output_file: null, summary: null, entry: null };
  setTasks([task]); drawTasks();
  const line = bar.querySelector<HTMLButtonElement>(".ag-tasks"); assert.ok(line); line.focus();
  assert.equal(line.textContent, "1 command running");
  assert.equal(bar.querySelector("[data-agtask]"), null, "rows with Stop live in the Running tab");
  setTasks([{ ...task }, { ...task, id: "second", name: "Test" }]); drawTasks();
  assert.equal(bar.querySelector(".ag-tasks"), line);
  assert.equal(line.textContent, "2 commands running");
  assert.equal(document.activeElement, line);
  setTasks([{ ...task, state: "completed" }, { ...task, id: "second", name: "Test" }]); drawTasks();
  assert.equal(line.textContent, "1 command running");
  setTasks([]); drawTasks();
  assert.equal(bar.querySelector(".ag-tasks"), null);
  setTasks([{ ...task, id: "third", name: "Lint" }]); drawTasks();
  assert.equal(bar.querySelector(".ag-tasks")?.textContent, "1 command running");
});

test("a steer shows in the queue on its way and while the node holds it, and not in the transcript", async t => {
  const { page, input } = setup();
  answers.agent_steer = [];
  t.after(() => { forgetAgentFollows(); delete answers.agent_steer; });
  steerMessage(input);
  await settle();
  const queued = () => bar.querySelector("#agqueue");
  const transcript = () => bar.querySelector("#agbody")?.textContent ?? "";
  assert.equal(input.value, "");
  assert.equal(transcript().includes("Use the existing fixture"), false);
  assert.deepEqual(page.entries.map(e => e.text), ["Check the build", "Compacting the conversation"], "pending rows do not change transcript positions");

  // The outbox has it, and tells the window: sending, then sent.
  const row = { id: 0, how: "steer" as const, text: "Use the existing fixture", images: [], reason: null, keys: [] };
  AG.outbox = [{ ...row, state: "sending" }];
  drawQueue();
  assert.ok(queued()?.textContent.includes("Use the existing fixture"));
  assert.ok(queued()?.textContent.includes("sending"));
  AG.outbox = [{ ...row, state: "sent" }];
  drawQueue();
  assert.ok(queued()?.textContent.includes("after the current step"));
  assert.equal(bar.querySelectorAll("#agqueue .it").length, 1);

  // The node lists it: its own row stands in for the sent one, which the outbox drops.
  page.queued = 1;
  page.queued_messages = ["Use the existing fixture"];
  AG.outbox = [];
  drawAgentBody(undefined, true);
  assert.ok(queued(), "reads during compaction preserve the steering row");
  assert.equal(bar.querySelectorAll("#agqueue .it.steer").length, 1);
  assert.equal(transcript().includes("Use the existing fixture"), false);

  // Delivered: the log has it once and the queue is empty.
  page.queued = 0;
  page.queued_messages = [];
  page.entries.push({ kind: "user", text: "Use the existing fixture" });
  drawAgentBody();
  assert.equal(queued(), null);
  assert.equal([...bar.querySelectorAll(".dcoderen.user")].filter(e => e.textContent.includes("Use the existing fixture")).length, 1);
});

test("a refused steer restores the draft", async t => {
  const { input } = setup();
  let refuse!: (error: string) => void;
  answers.agent_steer = new Promise((_resolve, reject) => { refuse = reject; });
  t.after(() => { forgetAgentFollows(); delete answers.agent_steer; });
  steerMessage(input);
  assert.equal(input.value, "");
  refuse("bad request: message refused");
  await settle();
  assert.equal(bar.querySelector("#agqueue"), null);
  assert.equal(input.value, "Use the existing fixture");
  assert.ok(bar.textContent.includes("message refused"));
});

test("editing a steer the node holds drops the node's steers, sends the others again and restores its text", async t => {
  const { page, input } = setup();
  input.value = "";
  answers.agent_steer = [];
  t.after(() => { forgetAgentFollows(); delete answers.agent_steer; });
  page.queued = 3;
  page.queued_messages = ["First one", "Use the existing fixture\n[Image: shot.png]", "Third one"];
  const sent = (id: number, text: string, images: string[]) => ({ id, how: "steer", text, images, keys: images, state: "sent", reason: null });
  emit("outbox-changed", { machine_id: "m1", agent: "claude", session: "steer-render", revision: 1, rows: [], sent: [sent(1, "First one", []), sent(2, "Use the existing fixture", ["shot.png"]), sent(3, "Third one", [])], resumed: null, pick: null });
  drawQueue();
  assert.equal(bar.querySelectorAll("#agqueue .it.steer").length, 3);
  calls.length = 0;
  await editQueued(1, true);
  assert.deepEqual(calls.filter(call => call.cmd === "agent_steer").map(call => JSON.stringify((call.args as { steer: unknown }).steer)), [
    JSON.stringify("drop_steers"),
    JSON.stringify({ steer: { text: "First one" } }),
    JSON.stringify({ steer: { text: "Third one" } }),
  ]);
  assert.equal(input.value, "Use the existing fixture");
});

test("editing a queued steer sends the others again with their own pictures, though the pictures share a name", async t => {
  const { page, input } = setup();
  const picture = (data: string) => ({ name: "image.jpg", mime: "image/jpeg", size: 8, data, url: `data:image/jpeg;base64,${data}`, original: null });
  answers.agent_steer = [];
  t.after(() => { forgetAgentFollows(); delete answers.agent_steer; AG.att = []; });
  const sentAs = () => calls.filter(call => call.cmd === "agent_steer").map(call => (call.args as { steer: { steer?: { text: string; images?: { name: string; data: string }[] } } }).steer.steer);
  const where = { machine_id: "m1", agent: "claude", session: "steer-render" };

  for (const data of ["AAAA", "BBBB", "CCCC"]) {
    input.value = "look";
    AG.att = [picture(data)];
    steerMessage(input);
    await settle();
  }

  // The outbox hands them over in order, each under its own key; the node lists those.
  const [first] = sentAs();
  const row = (id: number, key: string) => ({ id, how: "steer", text: first?.text ?? "", images: ["image.jpg"], keys: [key], state: "sent", reason: null });
  emit("outbox-changed", { ...where, revision: 1, rows: [], sent: [row(1, "k1"), row(2, "k2"), row(3, "k3")], resumed: null, pick: null });
  page.queued_messages = [1, 2, 3].map(n => `${first?.text}\n[Image: k${n}]`);
  page.queued = 3;
  AG.att = [];
  calls.length = 0;
  await editQueued(1, true);

  const again = sentAs().filter(Boolean);
  assert.deepEqual(again.map(one => one?.images?.[0]?.data), ["AAAA", "CCCC"]);
  assert.equal(input.value, "look");
  assert.deepEqual(AG.att.map(image => image.data), ["BBBB"]);
});

test("an entry the node lists that this app did not send as a steer is not edited or sent again", async t => {
  setup();
  answers.agent_steer = [];
  t.after(() => { forgetAgentFollows(); delete answers.agent_steer; });
  hold("A queued prompt");
  calls.length = 0;
  await editQueued(0, true);
  assert.equal(calls.filter(call => call.cmd === "agent_steer").length, 0);
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
      page: { ...next, queued: 0, queued_messages: [] }, sig: "", rest: "",
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

/// The steers the node holds, as the open session's last update lists them.
const hold = (...texts: string[]) => { AG.page!.queued = texts.length; AG.page!.queued_messages = texts; };

test("stream updates keep queued message controls and focus", t => {
  setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  hold("First", "Second");
  drawQueue();
  const edit = bar.querySelector<HTMLButtonElement>('[data-agq="1"][data-act="edit"]');
  assert.ok(edit);
  edit.focus();
  hold("First", "Second");
  drawQueue();
  assert.equal(bar.querySelector('[data-agq="1"][data-act="edit"]'), edit);
  assert.equal(document.activeElement, edit);
  hold("Second");
  drawQueue();
  assert.equal(bar.querySelector('[data-agq="0"][data-act="edit"]'), edit);
  assert.equal(document.activeElement, edit);
});

test("unchanged queue snapshots keep focused controls, then show changed content", t => {
  setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  hold("First");
  drawQueue();
  const edit = bar.querySelector<HTMLButtonElement>('[data-agq="0"][data-act="edit"]'); assert.ok(edit);
  edit.focus();
  hold("First");
  drawQueue();
  assert.equal(bar.querySelector('[data-agq="0"][data-act="edit"]'), edit);
  assert.equal(document.activeElement, edit);
  hold("Updated");
  drawQueue();
  assert.match(bar.querySelector("#agqueue")?.textContent ?? "", /Updated/);
});


test("leaving the agent disposes its queue view but keeps the node's steers", async t => {
  setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  hold("Pending");
  drawQueue();
  assert.ok(bar.querySelector("#agqueue"));
  setView("waiting");
  await settle();
  assert.equal(bar.querySelector("#agqueue"), null);
  assert.deepEqual(AG.page?.queued_messages, ["Pending"]);
  setView("agent");
  drawQueue();
  assert.ok(bar.querySelector("#agqueue")?.textContent.includes("Pending"));
});


test("drawing with no agent field leaves the queued messages unchanged", t => {
  setup();
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  hold("Held");
  bar.querySelector("#agfield")?.remove();
  drawQueue();
  assert.deepEqual(AG.page?.queued_messages, ["Held"]);
});

test("suggestion updates preserve queued controls and focus", async t => {
  const { input } = setup();
  window.HTMLElement.prototype.scrollIntoView = () => {};
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
  const { bindMentionMenu } = await import("../src/parts/session/mentionmenu.ts");
  answers.agent_commands = [{ name: "review", description: "Review work", hint: "" }, { name: "run", description: "Run checks", hint: "" }];
  t.after(() => { disposeQueue(); delete answers.agent_commands; });
  AG.page!.queued_messages = ["Queued"];
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
  setModes({ current: "plan", modes: [{ id: "plan", name: "Plan" }, { id: "edit", name: "Edit" }], plan: "<p>Plan text</p>" });
  const { drawModes } = await import("../src/parts/session/modepill.tsx");
  t.after(() => { disposeQueue(); setModes(null); });
  drawModes();
  const mode = bar.querySelector<HTMLButtonElement>("#agmodeb"), approve = bar.querySelector<HTMLButtonElement>('[data-agopt="0"]');
  assert.ok(mode && approve); approve.focus();
  AG.page!.queued_messages = ["Queued"];
  drawQueue();
  assert.equal(bar.querySelector("#agmodeb"), mode);
  assert.equal(bar.querySelector('[data-agopt="0"]'), approve);
  assert.equal(document.activeElement, approve);
  AG.answering = 7; drawModes();
  assert.equal(bar.querySelector('[data-agopt="0"]'), approve);
  assert.equal(approve.disabled, true);
  AG.answering = null;
});

test("mode pill hides only in each harness's own full-access mode", async t => {
  const { page } = setup();
  const { drawModes } = await import("../src/parts/session/modepill.tsx");
  t.after(() => { disposeQueue(); setModes(null); });

  page.session.agent = "agy";
  AG.open = { machine_id: "m1", agent: "agy", session: "steer-render" };
  setModes({ current: "default", modes: [{ id: "default", name: "Default" }, { id: "accept-edits", name: "Accept edits" }, { id: "plan", name: "Plan" }], plan: null });
  drawModes();
  assert.equal(bar.querySelector("#agmodeb"), null);

  setModes({ current: "plan", modes: [{ id: "default", name: "Default" }, { id: "accept-edits", name: "Accept edits" }, { id: "plan", name: "Plan" }], plan: null });
  drawModes();
  assert.equal(bar.querySelector("#agmodeb")?.textContent, "Plan");

  page.session.agent = "hermes";
  AG.open = { machine_id: "m1", agent: "hermes", session: "steer-render" };
  setModes({ current: "dont_ask", modes: [{ id: "dont_ask", name: "Full access" }, { id: "accept_edits", name: "Accept edits" }], plan: null });
  drawModes();
  assert.equal(bar.querySelector("#agmodeb"), null);

  page.session.agent = "claude";
  AG.open = { machine_id: "m1", agent: "claude", session: "steer-render" };
  setModes({ current: "bypassPermissions", modes: [{ id: "bypassPermissions", name: "Full access" }, { id: "default", name: "Default" }], plan: null });
  drawModes();
  assert.equal(bar.querySelector("#agmodeb"), null);

  setModes({ current: "default", modes: [{ id: "bypassPermissions", name: "Full access" }, { id: "default", name: "Default" }], plan: null });
  drawModes();
  assert.equal(bar.querySelector("#agmodeb")?.textContent, "Default");
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
  showAgentControl("plan", createElement(AgentPlan, { harness: "Agent", plan: "Plan", question: first, approvalKey: questionKey(first, opened()), answering: null }));
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
  AG.page!.queued_messages = ["Queued"];
  MN.paths = ["src/file.ts"];
  drawQueue(); drawAgentAttached();
  const queue = bar.querySelector("#agqueue"), attachments = bar.querySelector("#agatt"), field = bar.querySelector("#agfield");
  assert.ok(queue && attachments && field);
  assert.ok(queue.compareDocumentPosition(attachments) & Node.DOCUMENT_POSITION_FOLLOWING);
  assert.ok(field.contains(attachments));
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
  assert.equal(bar.querySelector(".dcoderen.agent .reply-body")?.textContent, "The fixes are committed.");
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

test("a queued message from another session shows its sender and words", () => {
  setup();
  const text = '<from-session id="p1" name="Review login">\nCheck the tests.\n</from-session>';
  AG.page!.queued_messages = [text];
  drawQueue();
  const row = bar.querySelector("#agqueue .it.from");
  assert.ok(row);
  assert.equal(row.querySelector(".lbl")?.textContent, "Review login");
  assert.equal(row.querySelector(".t")?.textContent, "Check the tests.");
  assert.equal(row.textContent?.includes("<from-session"), false);
  AG.page!.queued_messages = []; drawQueue();
});
