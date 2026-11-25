import { answers, bar, calls } from "./bar-env.ts";
import test, { type TestContext } from "node:test";
import { JSDOM } from "jsdom";
import assert from "node:assert/strict";
import { flushSync } from "react-dom";
import "../src/bar/bar-app.tsx";
import { openBarPage } from "../src/bar/bar-page.ts";
import { S, setView, connectWindow, windowHooks, type BackTo, type AgentReturn, type WindowHooks } from "../src/bar/core.ts";
import { AG, type AgentPage, type AgentRow, agentBack, agentKeys, agentsList, drawAgents, usageLine } from "../src/agents/agents.ts";
import { closeSession, visibleRunning } from "../src/agents/closing.ts";
import { loadSessions } from "../src/agents/sessions.ts";
import { closeOpenSession, pollAgent, sessionPicks } from "../src/agents/session.tsx";
import { agentHints } from "../src/agents/session-head.tsx";
import { forgetAgentFollows } from "../src/agents/session-follow.ts";
import { interruptAgent, sendAgentMessage } from "../src/agents/messages.ts";
import { drawModelPill, switchSessionModel } from "../src/agents/modelpill.tsx";
import { disposeComposerUi, FieldControl } from "../src/agents/composer-ui.tsx";
import { createElement } from "react";
import { createDocumentView } from "../src/ui/document-root.tsx";
import { MN } from "../src/agents/workdrop.ts";
import { agentMessage } from "../src/agents/session-frame.tsx";
import type { TranscriptUpdate } from "../src/agents/transcript-cache-state.ts";

/// Polls follow the session only while the page shows.
const visible = (on: boolean) => { (globalThis.document as { visibilityState?: string }).visibilityState = on ? "visible" : "hidden"; };
visible(true);

/// Frames wait until a test runs them.
const frames = new Map<number, FrameRequestCallback>();
let frameId = 0;
Object.assign(globalThis, {
  requestAnimationFrame: (callback: FrameRequestCallback) => { frames.set(++frameId, callback); return frameId; },
  cancelAnimationFrame: (id: number) => { frames.delete(id); },
});
function runFrames() {
  const due = [...frames.values()];
  frames.clear();
  for (const callback of due) callback(performance.now());
}

/// The bar shows only `els`, by selector, and the session's message field when given.
function show(els: Record<string, unknown>, message: ReturnType<typeof field> | null = null) {
  Object.assign(bar, { querySelector: (sel: string) => els[sel] ?? null });
  agentMessage.current = message as HTMLTextAreaElement | null;
}

const field = (value = "") => ({ value, placeholder: "", disabled: false, focus() {} });

/// Opens session s1 in `status`; polls stay off unless `polls`.
function openSession(status: string, polls = false): AgentPage {
  forgetAgentFollows();
  const session: AgentRow = { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "claude", cwd: "/srv/project", status, external: false, question: null };
  const page: AgentPage = { session, plan: [], entries: [], earlier: 0, error: null, queued: 0, turns: 0, usage: null };
  setView("agent");
  Object.assign(AG, { open: { machine_id: "m1", agent: "claude", session: "s1" }, page, queue: [], note: "", att: [], inflight: null, stopping: null });
  visible(polls);
  Object.assign(MN, { paths: [], uploads: [], ask: null });
  MN.files.clear();
  calls.length = 0;
  delete answers.agent_steer;
  return page;
}


const png = { name: "shot.png", mime: "image/png", size: 8, data: "iVBORw0KGgo=", url: "data:image/png;base64,iVBORw0KGgo=", original: null };

/// Enter in the session's field, with Option when `alt`.
function enter(input: unknown, alt = false) {
  const e = { key: "Enter", altKey: alt, shiftKey: false, isComposing: false, target: input, preventDefault() {} };
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
    forgetAgentFollows();
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
  runFrames();
  finish({ ...page, entries: [{ kind: "agent", text: "stale" }] });
  await polling;
  assert.deepEqual(AG.page?.entries.map(entry => entry.text), ["hello", "finished"]);
  AG.changed = Date.now() - 10 * 60_000;
  calls.length = 0;
  await pollAgent();
  assert.ok(!calls.some(c => ["agent_view", "agent_modes", "agent_tasks"].includes(c.cmd)));
});

test("a transcript burst draws once per frame and ends with the full text", async t => {
  const page = openSession("working", true);
  show({});
  answers.agent_follow = new Promise<void>(() => {});
  t.after(() => {
    forgetAgentFollows();
    visible(false);
    delete answers.agent_follow;
  });
  await pollAgent(true);
  await settle();
  const args = calls.find(c => c.cmd === "agent_follow")?.args as {
    onEvent: { onmessage(update: TranscriptUpdate<AgentPage> & { modes: null; tasks: [] }): void };
  };
  assert.ok(args);
  args.onEvent.onmessage({ kind: "snapshot", cursor: { start: 0, next: 1, fingerprint: "initial" }, resumed: false, page: { ...page, entries: [{ kind: "user", text: "hello" }] }, modes: null, tasks: [] });
  runFrames();
  assert.deepEqual(AG.page?.entries.map(entry => entry.text), ["hello"]);
  // Each delta carries the latest usage with the transcript.
  let text = "";

  for (let i = 1; i <= 30; i++) {
    text += `word${i} `;
    const entries = i % 10 ? [{ kind: "agent", text }] : [{ kind: "agent", text }, { kind: "tool", text: `Read: file-${i}.ts [completed]` }];
    const usage = { context: null, tokens: i, cost_usd: null };
    args.onEvent.onmessage({ kind: "delta", cursor: { start: 0, next: 1 + entries.length, fingerprint: `delta-${i}` }, resumed: false, page: { ...page, turns: i, earlier: 1, entries, usage }, modes: null, tasks: [] });
  }

  assert.deepEqual(AG.page?.entries.map(entry => entry.text), ["hello"], "nothing draws before the frame");
  assert.equal(frames.size, 1);
  runFrames();
  assert.equal(AG.page?.usage?.tokens, 30);
  assert.deepEqual(AG.page?.entries.map(entry => entry.text), ["hello", text, "Read: file-30.ts [completed]"]);
  assert.equal(AG.page?.turns, 30);
  assert.equal(frames.size, 0);
});

for (const [when, settled] of [["before", false], ["after", true]] as const) {
  test(`a delta held for a frame joins before a later one when the view is left ${when} its listeners run`, async t => {
    const page = openSession("working", true);
    show({});
    answers.agent_follow = new Promise<void>(() => {});
    t.after(() => {
      forgetAgentFollows();
      visible(false);
      frames.clear();
      delete answers.agent_follow;
    });
    await pollAgent(true);
    await settle();
    const args = calls.find(c => c.cmd === "agent_follow")?.args as {
      onEvent: { onmessage(update: TranscriptUpdate<AgentPage> & { modes: null; tasks: [] }): void };
    };
    assert.ok(args);
    const send = (kind: "snapshot" | "delta", earlier: number, text: string, status: string) => args.onEvent.onmessage({
      kind, cursor: { start: 0, next: earlier + 1, fingerprint: text }, resumed: false,
      page: { ...page, session: { ...page.session, status }, earlier, entries: [{ kind: earlier ? "agent" : "user", text }] }, modes: null, tasks: [],
    });
    send("snapshot", 0, "hello", "working");
    runFrames();

    // The first delta waits for its frame; the view is left before it comes.
    send("delta", 1, "first", "working");
    setView("waiting");
    if (settled) await settle();
    send("delta", 2, "second", "idle");
    runFrames();
    setView("agent");
    await pollAgent(true);

    assert.deepEqual(AG.page?.entries.map(entry => entry.text), ["hello", "first", "second"]);
    assert.equal(AG.page?.earlier, 0);
    assert.equal(AG.page?.session.status, "idle");
  });
}

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
  // The page shows without waiting for the tasks.
  await polling;
  assert.equal(AG.inflight, null);
  assert.ok(AG.sig);
  finish([]);
});

test("a text message that fails to send goes back in the field under the error", async () => {
  openSession("idle");
  const input = field("fix the build");
  show({}, input);
  refuse("agent_steer", "bad request: the machine is offline");
  await sendAgentMessage();
  assert.equal(input.value, "fix the build");
  assert.equal(AG.note, "the machine is offline");
  assert.deepEqual(AG.page?.entries, []);
});

for (const status of ["idle", "working", "ended"]) {
  test(`a refused ${status} message keeps attachment bytes for a later stash`, async t => {
    openSession(status);
    const input = field("check the attachment"), path = ".beep/uploads/sample.bin";
    const file = { name: "sample.bin", mime: "application/octet-stream", size: 4, data: "AAH+/w==" };
    show({}, input);
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

function fieldControl(t: TestContext, slot: "model" | "tasks" = "model") {
  const dom = new JSDOM("<!doctype html><span id='pill'></span>", { url: "https://app.test" });
  Object.defineProperty(dom.window.document, "visibilityState", { value: "visible", configurable: true, writable: true });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ document: dom.window.document, window: dom.window, Event: dom.window.Event, MutationObserver: dom.window.MutationObserver })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const pill = dom.window.document.getElementById("pill");
  assert.ok(pill);
  // The session field draws the pill from the composer's state.
  disposeComposerUi();
  const view = createDocumentView(pill);
  flushSync(() => view.render(createElement(FieldControl, { slot })));
  t.after(async () => {
    view.unmount(); disposeComposerUi(); pill.remove(); await new Promise(resolve => setImmediate(resolve)); dom.window.close();
    for (const [key, descriptor] of previous) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); }
  });
  return pill;
}

test("the model pill does not display the harness default for an unknown session model", t => {
  const page = openSession("idle");
  sessionPicks.delete(page.session.id);
  const pill = fieldControl(t);
  show({});
  drawModelPill();
  assert.match(pill.innerHTML, /Model unknown/);
  sessionPicks.set(page.session.id, { model: "selected-model", effort: null, fast: null });
  drawModelPill();
  assert.match(pill.innerHTML, /selected-model/);
  sessionPicks.delete(page.session.id);
});

test("the model pill uses the model reported by the session", t => {
  const page = openSession("idle");
  const pill = fieldControl(t);
  show({});
  sessionPicks.set(page.session.id, { model: "previous-model", effort: null, fast: null });
  page.model = "current-model";
  drawModelPill();
  const badge = pill.querySelector(".cc-ag");
  drawModelPill();
  assert.equal(pill.querySelector(".cc-ag"), badge, "unchanged refreshes preserve the harness badge");
  assert.match(pill.innerHTML, /current-model/);
  assert.doesNotMatch(pill.innerHTML, /previous-model/);
  sessionPicks.delete(page.session.id);
});

test("the model pill returns when its composer owner is recreated", t => {
  openSession("idle");
  const pill = fieldControl(t);
  show({});
  drawModelPill();
  const first = pill.querySelector("#agmodelb"); assert.ok(first);
  drawModelPill();
  assert.equal(pill.querySelector("#agmodelb"), first);
  disposeComposerUi();
  assert.equal(pill.querySelector("#agmodelb"), null);
  drawModelPill();
  assert.ok(pill.querySelector("#agmodelb"));
});

test("a session refresh updates the model pill", async t => {
  const page = openSession("idle");
  const pill = fieldControl(t);
  show({});
  await read({ ...page, model: "reported-model" });
  assert.match(pill.innerHTML, /reported-model/);
});

test("the model pill keeps the model that runs", async t => {
  const page = openSession("idle");
  const pill = fieldControl(t);
  show({});
  const s = page.session;

  let finishSwitch!: (keys: string[]) => void;
  answers.agent_steer = new Promise<string[]>(resolve => { finishSwitch = resolve; });
  const switching = switchSessionModel(s, { model: "opus-x", effort: null, fast: null }, "model");
  assert.match(pill.innerHTML, /opus-x/, "updates optimistically before a slow node answers");
  finishSwitch([]);
  await switching;
  delete answers.agent_steer;
  assert.match(pill.innerHTML, /opus-x/);

  // Default cannot be sent to a running session, so opus-x keeps running.
  const sent = calls.length;
  await switchSessionModel(s, { model: "", effort: null, fast: null }, "model");
  assert.equal(calls.length, sent);
  assert.match(pill.innerHTML, /opus-x/);
  assert.match(AG.note, /Default/);

  refuse("agent_steer", "bad request: the agent is still starting");
  await switchSessionModel(s, { model: "sonnet-y", effort: null, fast: null }, "model");
  assert.match(pill.innerHTML, /opus-x/);
  assert.doesNotMatch(pill.innerHTML, /sonnet-y/);
});

test("flsh can switch its model and thinking from a working session", async t => {
  const page = openSession("working");
  page.session.agent = "flsh";
  AG.open!.agent = "flsh";
  const pill = fieldControl(t);
  show({});
  drawModelPill();
  assert.equal(pill.querySelector("button")?.disabled, false);
  await switchSessionModel(page.session, { model: "ollama:example", effort: null, fast: null }, "model");
  await switchSessionModel(page.session, { model: "ollama:example", effort: "high", fast: null }, "effort");
  assert.deepEqual(steers(), [
    { configure: { model: "ollama:example", effort: null, fast: null } },
    { configure: { model: null, effort: "high", fast: null } },
  ]);
  assert.match(pill.innerHTML, /ollama:example/);
  assert.match(pill.innerHTML, /High/);
});

test("Option-Enter steers immediately with its files and images", async () => {
  openSession("working");
  answers.agent_steer = [];
  const input = field("look at this log");
  show({}, input);
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
  openSession("working");
  const input = field("use the fixture instead");
  show({}, input);
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
});

test("an image attached after a steer goes with the next message, not the steer", async () => {
  openSession("working");
  const input = field("first");
  show({}, input);
  answers.agent_steer = [];
  enter(input);
  AG.att.push(png);
  await settle();
  const [sent] = steers() as { steer: { images?: unknown[] } }[];
  assert.equal(sent.steer.images, undefined);
  assert.deepEqual(AG.att.map(a => a.name), ["shot.png"]);
});

test("Stopping leaves pending steers with the node (#378)", async () => {
  const page = openSession("working");
  show({}, field());
  page.queued = 1;
  page.queued_messages = ["steer me"];
  assert.equal(interruptAgent(), true);
  assert.deepEqual(AG.stopping, { id: "s1", turns: 0 });
  assert.doesNotMatch(agentHints().value.items.flat().join(" "), /interrupt/, "already stopping");
  await settle();
  assert.deepEqual(steers(), ["cancel"]);
  assert.deepEqual(page.queued_messages, ["steer me"]);

  // The node sends a steer it holds once the turn stops, or it had just
  // delivered it: sent from here too, the agent would get it twice.
  calls.length = 0;
  await read({ ...page, session: { ...page.session, status: "idle" } });
  assert.equal(AG.stopping, null);
  assert.deepEqual(steers(), []);
});

function agentsListDom(t: TestContext) {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  Object.defineProperty(dom.window.document, "visibilityState", { value: "visible", configurable: true, writable: true });
  // JSDOM does no layout, so it has no scrolling into view.
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  const actual = dom.window.document.getElementById("root"); assert.ok(actual);
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ document: dom.window.document, window: dom.window, Event: dom.window.Event, MutationObserver: dom.window.MutationObserver })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const originalHtml = Object.getOwnPropertyDescriptor(bar, "innerHTML");
  Object.defineProperty(bar, "innerHTML", { get: () => actual.innerHTML, set: (value: string) => { actual.innerHTML = value; }, configurable: true });
  // A box placed here reads as the bar's child until the test ends, then as detached, so the next test mounts its own.
  const placed = new Set<HTMLElement>();
  Object.defineProperty(bar, "replaceChildren", { value: (...nodes: HTMLElement[]) => { actual.replaceChildren(...nodes); for (const node of nodes) { placed.add(node); Object.defineProperty(node, "parentElement", { get: () => node.parentNode === actual ? bar : node.parentNode, configurable: true }); } }, configurable: true });
  t.after(async () => {
    // Leaving releases the page; the next test's document gets a box of its own.
    flushSync(() => openBarPage(() => {}).show({ view: "none" }));
    actual.replaceChildren(); for (const node of placed) Reflect.deleteProperty(node, "parentElement");
    await new Promise(resolve => setImmediate(resolve)); dom.window.close();
    if (originalHtml) Object.defineProperty(bar, "innerHTML", originalHtml);
    Reflect.deleteProperty(bar, "replaceChildren");
    for (const [key, descriptor] of previous) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); }
  });
}

test("replacing the shared session list does not change what a drawn row opens", async t => {
  agentsListDom(t);
  const row = (id: string) => ({ machine_id: "m1", machine_name: "build-box", id, agent: "claude", cwd: "/srv/project", status: "idle", external: false, question: null });
  answers.agent_sessions = [row("a1"), row("a2")];
  setView("agents");
  agentsList.setState({ selected: 0 });
  await loadSessions();
  drawAgents(false);
  // Another page reads a shorter list; this one is not redrawn.
  answers.agent_sessions = [row("a2")];
  await loadSessions();
  // The fake bar cannot draw the session view; what matters is which session it set to open.
  try { agentKeys({ key: "Enter", preventDefault() {} } as KeyboardEvent, false); } catch {}
  assert.equal(AG.open?.session, "a1");
});


test("a session refresh records the title returned by its machine", async () => {
  const page = openSession("idle");
  const title = { textContent: "", title: "", querySelector: () => null };
  show({ "#agtitle": title });
  await read({ ...page, session: { ...page.session, title: "Fix the retry" } });
  assert.equal(AG.page?.session.title, "Fix the retry");
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

for (const back of ["agents", { view: "projects", arg: { project: "acme/widget", tile: "running" } }] satisfies AgentReturn[]) {
  test(`Esc returns to ${typeof back === "string" ? "Running agents" : "the project's Running section"} before the project overview`, async t => {
    agentsListDom(t);
    const previous = windowHooks;
    t.mock.timers.enable({ apis: ["setInterval"] });
    t.after(() => { connectWindow(previous!); setView("agent"); });
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

// Keep navigation real while replacing the window's destination renderer: a closed
// session opens its project, whatever page it was opened from.
function closeDestination(t: test.TestContext, from: BackTo = { view: "home", arg: null }) {
  const previous = windowHooks;
  connectWindow({
    goBack() { assert.fail("should open the session's project"); },
    sessionProject() { return { label: "widget", name: "acme/widget", open() { setView("agents"); }, afterClose() { setView("agents"); } }; },
  } as unknown as WindowHooks);
  AG.back = from;
  t.after(() => connectWindow(previous!));
}

test("a refused background close shows an error without reopening the session", async t => {
  agentsListDom(t);
  openSession("idle");
  closeDestination(t);
  show({});
  refuse("agent_steer", "Machine is offline");
  const closing = closeOpenSession();
  assert.equal(S.view, "agents", "leaves before the refusal arrives");
  await closing;
  assert.deepEqual(steers(), ["close"]);
  assert.equal(S.view, "agents");
  assert.match(document.body.textContent ?? "", /Could not close session: Machine is offline/);
  const alert = document.querySelector<HTMLElement>('[role="alert"]'); assert.ok(alert);
  const dismiss = alert.querySelector<HTMLButtonElement>('[aria-label="Dismiss"]'); assert.ok(dismiss); dismiss.click();
  assert.equal(document.body.contains(alert), false);
});

for (const from of [{ view: "home", arg: null }, { view: "projects", arg: { project: "acme/widget", tile: "running" } }, { view: "pane", arg: "agents" }] satisfies BackTo[]) {
  test(`Cmd+Shift+W from a session opened from ${from.view} opens its project at once, and a late reply leaves another page alone`, async t => {
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
    setView("waiting");
    finish(null);
    await settle();
    assert.equal(S.view, "waiting");
  });
}

test("closing a child session opens its project instead of its parent", async t => {
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

test("closing removes list rows and counts before the reply, survives stale reads, and restores on refusal", async t => {
  agentsListDom(t);
  const row = (machine_id: string) => ({ machine_id, machine_name: "build-box", id: "close-list", agent: "claude", title: machine_id, cwd: "/srv/project", status: "idle", external: false, question: null, doing: null, started_at: 0, last_activity: 0 });
  const closing = row("first-machine"), other = row("second-machine");
  const running = [{ ...closing, kind: "agent" as const }, { ...other, kind: "agent" as const }];
  answers.agent_sessions = [closing, other];
  await loadSessions();
  setView("agents");
  agentsList.setState({ selected: 1 });
  show({});
  drawAgents(false);
  assert.match(bar.innerHTML, /2 live on 2 machines/);
  let refuseClose!: (reason: string) => void;
  answers.agent_steer = new Promise((_, reject) => { refuseClose = reject; });
  const sent = closeSession(closing);
  assert.doesNotMatch(bar.innerHTML, /first-machine/);
  assert.match(bar.innerHTML, /second-machine/);
  assert.match(bar.innerHTML, /1 live on 1 machine/);
  assert.equal(agentsList.getState().selected, 0, "selection stays on a row after the list shrinks");
  assert.deepEqual(visibleRunning(running), [running[1]]);
  // A cached response must not bring back a row while its close is pending.
  await loadSessions();
  drawAgents(false);
  assert.doesNotMatch(bar.innerHTML, /first-machine/);
  refuseClose("Machine is offline");
  await assert.rejects(sent, reason => reason === "Machine is offline");
  assert.match(bar.innerHTML, /first-machine/);
  assert.match(bar.innerHTML, /2 live on 2 machines/);
  assert.deepEqual(visibleRunning(running), running);
  delete answers.agent_steer;
});


test("an image sent without typed text gets a default prompt", async () => {
  openSession("idle");
  const input = field("  ");
  show({}, input);
  AG.att = [png];
  answers.agent_steer = ["picture-key"];
  await sendAgentMessage();
  assert.deepEqual(steers(), [{ steer: { text: "Check this image.", images: [{ name: png.name, mime: png.mime, data: png.data }] } }]);
  assert.equal(input.value, "");
  assert.deepEqual(AG.att, []);
  assert.equal(AG.page?.entries[0].text, "Check this image.\n[image: shot.png]");
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

test("session model choices survive a window reload", async t => {
  const saved = new Map<string, string>();
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  t.after(() => {
    if (descriptor) Object.defineProperty(globalThis, "localStorage", descriptor);
    else Reflect.deleteProperty(globalThis, "localStorage");
  });
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
    get length() { return saved.size; },
    clear: () => saved.clear(),
    key: (index: number) => [...saved.keys()][index] ?? null,
    removeItem: (key: string) => { saved.delete(key); },
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => { saved.set(key, value); },
  } });
  const { SessionPicks } = await import("../src/agents/agentmodels.ts");
  const choices = new SessionPicks();
  choices.set("saved-model-session", { model: "model-sol", effort: "high", fast: null });
  assert.deepEqual(new SessionPicks().get("saved-model-session"), { model: "model-sol", effort: "high", fast: null });
  choices.delete("saved-model-session");
  assert.equal(new SessionPicks().has("saved-model-session"), false);
});
