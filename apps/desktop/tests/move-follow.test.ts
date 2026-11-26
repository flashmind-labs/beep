import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage, type AgentRow } from "../src/agents/agents-state.ts";
import { setView } from "../src/bar/core.ts";
import { drawMove } from "../src/agents/move.ts";
import { moveStore } from "../src/agents/move-store.ts";
import { drawAgentFrame } from "../src/agents/session.tsx";
import { forgetAgentFollows } from "../src/agents/session-follow.ts";
import { imageLines } from "../src/agents/agentlog.ts";
import { disposeQueue } from "../src/agents/composer-ui.tsx";
import type { ChatBody } from "../src/ui/turn.ts";
import { fillLog } from "../src/agents/logwindow.tsx";
import "../src/bar/bar-app.tsx";

Object.assign(globalThis, { getSelection: () => null, getComputedStyle: () => ({ rowGap: "0" }) });
const settle = () => new Promise<void>(resolve => setImmediate(resolve));

test("the open conversation follows a completed move even when its target lists it later", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const source: AgentRow = { machine_id: "laptop", machine_name: "laptop", id: "s1", agent: "codex", cwd: "/srv/project", status: "idle", external: false };
  const target = { ...source, machine_id: "studio", machine_name: "studio" };
  const page: AgentPage = { session: source, plan: [], entries: [], earlier: 0, error: null, queued: 0, turns: 0, usage: null };
  const record = { session: "s1", agent: "codex", from: "laptop", to: "studio", project: "acme/app", at: Date.now() };
  const progress = { answer: "known" as const, phase: "done" as const, copied: 0, total: 0, source_away: false, reason: null, worktree: null, worktree_at: 0, worktrees: 0, edits_left: false, files_copied: 0, files_total: 0 };
  setView("agent");
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
  // This check draws the header; the composer needs native textarea accessors.
  const query = bar.querySelector.bind(bar);
  t.mock.method(bar, "querySelector", (selector: string) => selector === "#agmovebox" || selector === "#agmodel" ? null : query(selector));
  const byId = document.getElementById.bind(document);
  t.mock.method(document, "getElementById", (id: string) => id === "agshell" || id.startsWith("agpane-") ? null : byId(id));
  Object.assign(AG, { open: { machine_id: source.machine_id, agent: source.agent, session: source.id }, page, inflight: null });
  Object.assign(answers, { agent_sessions: [source], session_moves: [record], session_move_progress: progress, agent_view: { ...page, session: target }, agent_hosts: [], agent_tasks: [], editors: [] });
  moveStore.setState({ records: new Map([[source.id, record]]), progress: new Map([[source.id, progress]]) });
  bar.innerHTML = '<div id="aghere"></div>';
  calls.length = 0;
  drawMove();
  await settle();
  const stream = calls.find(c => c.cmd === "mesh_watch")?.args as { onEvent: { onmessage(event: unknown): void } };
  assert.ok(stream);
  let revision = 0;
  const push = async (list: AgentRow[]) => {
    stream.onEvent.onmessage({ machine_id: "studio", topic: "home.*", revision: ++revision, data: { views: [{ command: "agent_sessions", args: {}, value: list }] } });
    await settle();
  };
  calls.length = 0;
  await push([source]);
  assert.equal(AG.open?.machine_id, source.machine_id);
  await push([source, target]);
  assert.equal(AG.open?.machine_id, target.machine_id);
  assert.equal(AG.page?.session.machine_name, target.machine_name);
  assert.equal(bar.querySelector("#aghere")?.textContent, target.machine_name);
  assert.equal(calls.some(c => c.cmd === "agent_sessions"), false);
  clearTimeout(AG.timer);
});

test("replayed user images are shown even when more prompt text follows them", () => {
  const a = "image-0123456789abcdef", b = "image-fedcba9876543210";
  assert.deepEqual(imageLines(`Look at this\n[image: ${a}]\nAnd this\n[image: ${b}]\nMore detail`), {
    text: "Look at this\nAnd this\nMore detail", names: [a, b],
  });
});

test("taking over an external session retains the transcript and dock", t => {
  const session: AgentRow = { machine_id: "machine", machine_name: "Machine", id: "external", agent: "codex", cwd: "/srv/project", status: "working", external: true };
  const page: AgentPage = { session, plan: [], entries: [], earlier: 0, error: null, queued: 0, turns: 0, usage: null };
  Object.assign(AG, { open: { machine_id: session.machine_id, agent: session.agent, session: session.id }, page, modes: null, tasks: [], queue: [] });
  Object.assign(answers, { agent_tasks: [], editors: [], agent_hosts: [], session_moves: [] });
  setView("agent");
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  drawAgentFrame();
  const body = bar.querySelector<ChatBody>("#agbody"), dock = bar.querySelector("#agpane-right");
  assert.ok(body && dock); assert.ok(bar.querySelector(".dbanner"));
  body.stuck = false; body.top = 42; body.scrollTop = 42;
  page.session = { ...session, status: "ended" };
  drawAgentFrame(true, body);
  assert.equal(bar.querySelector("#agbody"), body); assert.equal(bar.querySelector("#agpane-right"), dock);
  assert.equal(body.scrollTop, 42);
  assert.ok(bar.querySelector("#agmsg")); assert.equal(bar.querySelector(".dbanner"), null);
});

test("a cached transcript survives reattachment and is released when replaced", t => {
  const session: AgentRow = { machine_id: "machine", machine_name: "Machine", id: "retained", agent: "codex", cwd: "/srv/project", status: "working", external: false };
  const page: AgentPage = { session, plan: [], entries: [{ kind: "user", text: "Check" }, { kind: "agent", text: "Answer" }], earlier: 0, error: null, queued: 0, turns: 0, usage: null };
  Object.assign(AG, { open: { machine_id: session.machine_id, agent: session.agent, session: session.id }, page, modes: null, tasks: [], queue: [] });
  Object.assign(answers, { agent_tasks: [], editors: [], agent_hosts: [], session_moves: [] });
  setView("agent");
  t.after(() => { disposeQueue(); forgetAgentFollows(); });
  drawAgentFrame();
  const body = bar.querySelector<ChatBody>("#agbody"); assert.ok(body);
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 800 });
  fillLog(body, false);
  const reply = body.querySelector(".dcoderen.agent"); assert.ok(reply);
  body.remove();
  drawAgentFrame(false, body);
  assert.equal(body.querySelector(".dcoderen.agent"), reply);
  drawAgentFrame(false);
  assert.equal(body.querySelector(".dcoderen.agent"), null);
});
