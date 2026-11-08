import { answers, bar, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { S, connectWindow, windowHooks, type BackTo, type WindowHooks } from "../src/bar/core.ts";
import { AG, type AgentPage, type AgentRow, agentBack, agentKeys, drawAgents, usageLine } from "../src/agents/agents.ts";
import { closeSession, visibleRunning } from "../src/agents/closing.ts";
import { loadSessions } from "../src/agents/sessions.ts";
import { agentHints, closeOpenSession, pollAgent, stopAgentFollow } from "../src/agents/session.ts";
import { interruptAgent, sendAgentMessage } from "../src/agents/messages.ts";
import { drawModelPill, switchSessionModel } from "../src/agents/modelpill.ts";
import type { Waiting } from "../src/agents/agentqueue.ts";
import { MN } from "../src/agents/workdrop.ts";
import type { TranscriptUpdate } from "../src/agents/transcript-cache-state.ts";

/// Polls follow the session only while the page shows.
const visible = (on: boolean) => { (globalThis.document as { visibilityState?: string }).visibilityState = on ? "visible" : "hidden"; };
visible(true);

/// The bar shows only `els`, by selector.
function show(els: Record<string, unknown>) {
  Object.assign(bar, { querySelector: (sel: string) => els[sel] ?? null });
}

const field = (value = "") => ({ value, placeholder: "", disabled: false, focus() {} });

/// Opens session s1 in `status`; polls stay off unless `polls`.
function openSession(status: string, polls = false): AgentPage {
  stopAgentFollow();
  const session: AgentRow = { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "claude", cwd: "/srv/project", status, external: false, question: null };
  const page: AgentPage = { session, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, turns: 0 };
  S.view = "agent";
  Object.assign(AG, { open: { machine_id: "m1", agent: "claude", session: "s1" }, page, queue: [], note: "", att: [], inflight: null, stopping: null });
  visible(polls);
  Object.assign(MN, { paths: [], uploads: [], ask: null });
  MN.files.clear();
  calls.length = 0;
  delete answers.agent_steer;
  return page;
}

/// A pending steer of plain words.
const words = (text: string): Waiting => ({ how: "steer", typed: text, text, paths: [], images: [], state: null, after: 0 });

const png = { name: "shot.png", mime: "image/png", size: 8, data: "iVBORw0KGgo=", url: "data:image/png;base64,iVBORw0KGgo=" };

/// Enter in the session's field, with Option when `alt`.
function enter(input: unknown, alt = false) {
  const e = { key: "Enter", altKey: alt, shiftKey: false, isComposing: false, target: Object.assign(input as object, { id: "agmsg" }), preventDefault() {} };
  return agentKeys(e as unknown as KeyboardEvent, false);
}

/// Lets the sends a poll started run.
const settle = () => new Promise(resolve => setImmediate(resolve));

async function readLegacy() {
  await pollAgent();
  await settle();
  // The fixture's unsupported stream has closed, so the finite fallback can read.
  await pollAgent();
}

/// The `agent_steer` calls made, by what each sent.
const steers = () => calls.filter(c => c.cmd === "agent_steer").map(c => (c.args as { steer: unknown }).steer);

/// Reads the session once as `page` says, without following it after.
async function read(page: AgentPage) {
  answers.agent_view = page;
  visible(true);
  await readLegacy();
  visible(false);
  clearTimeout(AG.timer);
  await settle();
}

/// `cmd` fails with `why`, as Tauri rejects, until the next `openSession`.
function refuse(cmd: string, why: string) {
  Object.defineProperty(answers, cmd, { get() { throw why; }, configurable: true, enumerable: true });
}

test("opening a session reads it once even when the webview reports hidden", async () => {
  const page = openSession("starting");
  show({});
  answers.agent_view = { ...page, session: { ...page.session, status: "working" }, entries: [{ kind: "agent", text: "Reading the project" }] };
  await pollAgent(true);
  assert.equal(AG.page?.session.status, "working");
  assert.equal(AG.page?.entries[0].text, "Reading the project");
  assert.equal(calls.filter(c => c.cmd === "agent_view").length, 1);
  const logs = calls.filter(c => c.cmd === "log_webview").map(c => c.args as { message: string; info?: boolean });
  const applied = logs.find(l => l.message.includes('"stage":"read-applied"'))!;
  assert.equal(applied.info, true);
  assert.match(applied.message, /"visibility":"hidden"/);
  assert.match(applied.message, /"windowHidden":null/);
  assert.match(applied.message, /"paneOff":false/);
  assert.match(applied.message, /"focused":false/);
  assert.ok(logs.some(l => l.message.includes('"stage":"read-settled"')));
  assert.ok(logs.every(l => !l.message.includes("Reading the project")), "diagnostics omit message contents");
  await pollAgent();
  assert.equal(calls.filter(c => c.cmd === "agent_view").length, 1, "background polling still waits for the window");
  assert.ok(calls.some(c => c.cmd === "log_webview" && (c.args as { message: string }).message.includes('"reason":"hidden"')));
});

test("live suffixes preserve history and a late fallback read cannot replace them", async t => {
  const page = openSession("idle", true);
  show({});
  answers.agent_follow = new Promise<void>(() => {});
  let finish!: (page: AgentPage) => void;
  answers.agent_view = new Promise<AgentPage>(resolve => { finish = resolve; });
  t.after(() => {
    stopAgentFollow();
    finish(page);
    visible(false);
    delete answers.agent_follow;
    delete answers.agent_view;
  });
  await pollAgent(true);
  await settle();
  assert.ok(!calls.some(c => c.cmd === "agent_view"), "opening a stream avoids a duplicate transcript read");
  const now = Date.now();
  t.mock.method(Date, "now", () => now + 3000);
  const polling = pollAgent(true);
  const args = calls.find(c => c.cmd === "agent_follow")?.args as {
    onEvent: { onmessage(update: TranscriptUpdate<AgentPage> & { modes: null; tasks: [] }): void };
  };
  assert.ok(args);
  args.onEvent.onmessage({ kind: "snapshot", cursor: { start: 0, next: 2, fingerprint: "initial" }, resumed: false, page: { ...page, entries: [{ kind: "user", text: "hello" }, { kind: "agent", text: "working" }] }, modes: null, tasks: [] });
  args.onEvent.onmessage({ kind: "delta", cursor: { start: 0, next: 2, fingerprint: "updated" }, resumed: false, page: { ...page, earlier: 1, entries: [{ kind: "agent", text: "finished" }] }, modes: null, tasks: [] });
  finish({ ...page, entries: [{ kind: "agent", text: "stale" }] });
  await polling;
  assert.deepEqual(AG.page?.entries.map(entry => entry.text), ["hello", "finished"]);
  AG.changed = Date.now() - 10 * 60_000;
  calls.length = 0;
  await pollAgent();
  assert.ok(!calls.some(c => ["agent_view", "agent_modes", "agent_tasks"].includes(c.cmd)));
});

test("a stalled first read logs the operation it is waiting for", async t => {
  const page = openSession("starting");
  show({});
  answers.agent_view = page;
  let finish!: (tasks: []) => void;
  answers.agent_tasks = new Promise<[]>(resolve => { finish = resolve; });
  t.after(() => { finish([]); delete answers.agent_tasks; });
  const polling = pollAgent(true);
  await settle();
  const logs = calls.filter(c => c.cmd === "log_webview").map(c => (c.args as { message: string }).message);
  assert.ok(logs.some(l => l.includes('"stage":"read-settled"') && l.includes('"pending":["tasks"]')));
  assert.equal(AG.inflight, AG.open);
  finish([]);
  await polling;
  assert.equal(AG.inflight, null);
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

for (const status of ["idle", "working", "ended"]) {
  test(`a refused ${status} message keeps attachment bytes for a later stash`, async t => {
    openSession(status);
    const input = field("check the attachment"), path = ".beep/uploads/sample.bin";
    const file = { name: "sample.bin", mime: "application/octet-stream", size: 4, data: "AAH+/w==" };
    show({ "#agmsg": input });
    MN.paths = [path];
    MN.files.set(path, file);
    AG.att = [png];
    refuse("agent_steer", "bad request: refused");
    refuse("agent_resume", "bad request: refused");
    t.after(() => { delete answers.agent_resume; });
    if (status === "working") { enter(input); await settle(); }
    else await sendAgentMessage();
    assert.equal(input.value, "check the attachment");
    assert.deepEqual(MN.paths, [path]);
    assert.deepEqual(MN.files.get(path), file);
    assert.deepEqual(AG.att, [png]);
  });
}

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

test("flsh can switch its model and thinking from a working session", async () => {
  const page = openSession("working");
  page.session.agent = "flsh";
  AG.open!.agent = "flsh";
  const pill = { innerHTML: "" };
  show({ "#agmodel": pill });
  drawModelPill();
  assert.doesNotMatch(pill.innerHTML, / disabled/);
  await switchSessionModel(page.session, { model: "ollama:example", effort: null }, "model");
  await switchSessionModel(page.session, { model: "ollama:example", effort: "high" }, "effort");
  assert.deepEqual(steers(), [
    { configure: { model: "ollama:example", effort: null } },
    { configure: { model: null, effort: "high" } },
  ]);
  assert.match(pill.innerHTML, /ollama:example/);
  assert.match(pill.innerHTML, /High/);
});

test("Option-Enter steers immediately with its files and images", async () => {
  openSession("working");
  answers.agent_steer = [];
  const input = field("look at this log");
  show({ "#agmsg": input });
  MN.paths = [".beep/uploads/whatever.log"];
  AG.att = [png];
  assert.equal(enter(input, true), true);
  await settle();
  const [sent] = steers() as { steer: { text: string; images: { name: string }[] } }[];
  assert.match(sent.steer.text, /look at this log/);
  assert.match(sent.steer.text, /@\.beep\/uploads\/whatever\.log/);
  assert.deepEqual(sent.steer.images.map(i => i.name), ["shot.png"]);
  assert.equal(input.value, "");
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

test("Stopping leaves pending steers with the node (#378)", async () => {
  const page = openSession("working");
  show({ "#agmsg": field() });
  AG.queue = [{ ...words("steer me"), how: "steer", state: "sent", after: 0 }];
  assert.equal(interruptAgent(), true);
  assert.deepEqual(AG.stopping, { id: "s1", turns: 0 });
  assert.doesNotMatch(agentHints(), /interrupt/, "already stopping");
  await settle();
  assert.deepEqual(steers(), ["cancel"]);
  assert.deepEqual(AG.queue.map(q => q.how), ["steer"]);

  // The node sends a steer it holds once the turn stops, or it had just
  // delivered it: sent from here too, the agent would get it twice.
  calls.length = 0;
  await read({ ...page, session: { ...page.session, status: "idle" } });
  assert.equal(AG.stopping, null);
  assert.deepEqual(steers(), []);
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


test("the open session title follows the title returned by its machine", async () => {
  const page = openSession("idle");
  const title = { textContent: "", title: "", querySelector: () => null };
  show({ "#agtitle": title });
  await read({ ...page, session: { ...page.session, title: "Fix the retry" } });
  assert.equal(title.textContent, "Fix the retry");
  assert.equal(title.title, "Fix the retry\nDouble-click to rename (Enter)");
});

test("a session refresh preserves a title being edited", async () => {
  const page = openSession("idle");
  const input = { value: "My unfinished title" };
  const title = { textContent: "", title: "", querySelector: () => input };
  show({ "#agtitle": title });
  await read({ ...page, session: { ...page.session, title: "New generated title" } });
  assert.equal(title.textContent, "");
  assert.equal(input.value, "My unfinished title");
  assert.equal(AG.page?.session.title, "New generated title");
});

test("close refuses ended and external sessions and empty drafts", async () => {
  for (const state of ["ended", "external", "draft"]) {
    const page = openSession(state === "ended" ? "ended" : "idle");
    page.session.external = state === "external";
    AG.draft = state === "draft" ? "sample-project" : null;
    show({});
    await closeOpenSession();
    assert.deepEqual(steers(), []);
  }
  AG.draft = null;
});

for (const back of ["agents", { view: "projects", arg: { project: "acme/widget", tile: "running" } }] satisfies (string | BackTo)[]) {
  test(`Esc returns to ${typeof back === "string" ? "Running agents" : "the project's Running section"} before the project overview`, async t => {
    const previous = windowHooks;
    t.mock.timers.enable({ apis: ["setInterval"] });
    t.after(() => { connectWindow(previous!); S.view = "agent"; });
    openSession("idle");
    Object.assign(AG, { back, draft: null, child: null, modelOpen: false });
    show({});
    answers.agent_sessions = [];
    let projectOpened = false;
    let returned: BackTo | undefined;
    connectWindow({
      goBack(from: BackTo) { returned = from; },
      sessionProject() { return { label: "widget", name: "acme/widget", open() { projectOpened = true; } }; },
    } as unknown as WindowHooks);

    await agentBack();

    assert.equal(projectOpened, false);
    if (typeof back === "string") assert.equal(S.view, "agents");
    else assert.deepEqual(returned, back);
  });
}

// Keep navigation real while replacing the window's destination renderer.
function closeDestination(t: test.TestContext, from: BackTo = { view: "home" }) {
  const previous = windowHooks;
  connectWindow({
    goBack(target: BackTo) { assert.deepEqual(target, from); S.view = "agents"; },
    sessionProject() { return { label: "widget", name: "acme/widget", open() { assert.fail("should return to the previous page"); } }; },
  } as unknown as WindowHooks);
  AG.back = from;
  t.after(() => connectWindow(previous!));
}

test("a refused background close shows an error without reopening the session", async t => {
  openSession("idle");
  closeDestination(t);
  show({});
  const messages: unknown[] = [];
  const create = document.createElement, body = document.body;
  Object.defineProperty(document, "body", { value: { append() { messages.push("banner"); } }, configurable: true });
  document.createElement = (() => ({
    style: {}, append(...kids: unknown[]) { messages.push(...kids); },
    setAttribute() {}, addEventListener() {}, remove() {},
  })) as unknown as typeof create;
  t.after(() => { document.createElement = create; Object.defineProperty(document, "body", { value: body, configurable: true }); });
  refuse("agent_steer", "Machine is offline");
  const closing = closeOpenSession();
  assert.equal(S.view, "agents", "leaves before the refusal arrives");
  await closing;
  assert.deepEqual(steers(), ["close"]);
  assert.equal(S.view, "agents");
  assert.ok(messages.includes("Could not close session: Machine is offline"));
  assert.ok(messages.includes("banner"));
});

for (const from of [{ view: "home" }, { view: "projects", arg: { project: "acme/widget", tile: "running" } }, { view: "pane", arg: "agents" }] satisfies BackTo[]) {
  test(`Cmd+Shift+W returns to ${from.view} immediately and a late reply leaves another page alone`, async t => {
    const page = openSession("idle");
    page.session.id = AG.open!.session = `close-from-${from.view}`;
    closeDestination(t, from);
    AG.modelOpen = true;
    t.after(() => { AG.modelOpen = false; });
    show({});
    let finish!: (value: null) => void;
    answers.agent_steer = new Promise(resolve => { finish = resolve; });
    let prevented = 0;
    const event = { key: "W", shiftKey: true, altKey: false, ctrlKey: false, isComposing: false, repeat: false, preventDefault() { prevented++; } } as KeyboardEvent;
    assert.equal(agentKeys({ ...event, shiftKey: false } as KeyboardEvent, true), false);
    assert.equal(agentKeys(event, true), true);
    assert.equal(S.view, "agents", "leaves while the machine's reply is pending, even with a picker open");
    assert.equal(agentKeys(event, true), false);
    assert.deepEqual(steers(), ["close"]);
    assert.equal(prevented, 1);
    S.view = "chat";
    finish(null);
    await settle();
    assert.equal(S.view, "chat");
  });
}

test("closing a child session returns to its previous page instead of opening its parent", async t => {
  const page = openSession("idle");
  page.session.id = AG.open!.session = "close-child";
  closeDestination(t, { view: "projects", arg: { project: "acme/widget", tile: "running" } });
  show({});
  answers.agent_sessions = [
    { ...page.session, id: "parent" },
    { ...page.session, parent: { machine_id: "m1", id: "parent", key: "" } },
  ];
  await loadSessions();
  answers.agent_steer = null;
  await closeOpenSession();
  assert.equal(S.view, "agents");
  assert.deepEqual(steers(), ["close"]);
});

test("closing removes list rows and counts before the reply, survives stale reads, and restores on refusal", async () => {
  const row = (machine_id: string) => ({ machine_id, machine_name: "build-box", id: "close-list", agent: "claude", title: machine_id, cwd: "/srv/project", status: "idle", external: false, question: null, doing: null, started_at: 0, last_activity: 0 });
  const closing = row("first-machine"), other = row("second-machine");
  const research = { kind: "research" as const, session: "r1", topic: "Research", status: "working", round: 1, sources: 0, started_at: 0 };
  const running = [{ ...closing, kind: "agent" as const }, { ...other, kind: "agent" as const }, research];
  answers.agent_sessions = [closing, other];
  await loadSessions();
  S.view = "agents";
  AG.sel = 1;
  show({});
  drawAgents(false);
  assert.match(bar.innerHTML, /2 on 2 machines/);
  let refuseClose!: (reason: string) => void;
  answers.agent_steer = new Promise((_, reject) => { refuseClose = reject; });
  const sent = closeSession(closing);
  assert.doesNotMatch(bar.innerHTML, /first-machine/);
  assert.match(bar.innerHTML, /second-machine/);
  assert.match(bar.innerHTML, /1 on 1 machine/);
  assert.equal(AG.sel, 0, "selection stays on a row after the list shrinks");
  assert.deepEqual(visibleRunning(running), [running[1], research]);
  // A cached response must not bring back a row while its close is pending.
  await loadSessions();
  drawAgents(false);
  assert.doesNotMatch(bar.innerHTML, /first-machine/);
  refuseClose("Machine is offline");
  await assert.rejects(sent, reason => reason === "Machine is offline");
  assert.match(bar.innerHTML, /first-machine/);
  assert.match(bar.innerHTML, /2 on 2 machines/);
  assert.deepEqual(visibleRunning(running), running);
  delete answers.agent_steer;
});


test("an image can be sent with no prompt", async () => {
  const page = openSession("idle");
  const input = field("  ");
  show({ "#agmsg": input });
  AG.att = [png];
  answers.agent_steer = ["picture-key"];
  await sendAgentMessage();
  assert.deepEqual(steers(), [{ steer: { text: "", images: [{ name: png.name, mime: png.mime, data: png.data }] } }]);
  assert.equal(input.value, "");
  assert.deepEqual(AG.att, []);
  assert.equal(page.entries[0].text, "[image: shot.png]");
});

test("session usage shows cumulative tokens and spend apart from the remaining key budget", () => {
  const credit = { provider: "openrouter", scope: "api_key", unit: "usd", remaining: "7.25", total: "10", used: "2.75", resets: "monthly" };
  const usage = { context: { used: 441_000, size: 1_050_000 }, tokens: 182_000, cost_usd: 1.27, credits: [credit] };
  assert.equal(usageLine(usage), "42%/1.05M · 182k tokens · $1.27 / $7.25");
  assert.equal(usageLine({ ...usage, credits: [{ ...credit, remaining: null }] }), "42%/1.05M · 182k tokens · $1.27 / ?");
  assert.equal(usageLine({ ...usage, credits: [{ ...credit, scope: "account" }] }), "42%/1.05M · 182k tokens · $1.27 / $7.25");
  assert.equal(usageLine({ ...usage, credits: [{ ...credit, remaining: "0" }] }), "42%/1.05M · 182k tokens · $1.27 / $0.00");
  assert.equal(usageLine({ context: null, tokens: 0, cost_usd: null }), "");
});
