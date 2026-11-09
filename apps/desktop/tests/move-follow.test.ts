import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage, type AgentRow } from "../src/agents/agents.ts";
import { S } from "../src/bar/core.ts";
import { drawMove } from "../src/agents/move.ts";
import { MV } from "../src/agents/move-store.ts";
import { userImages } from "../src/agents/session.ts";

Object.assign(globalThis, { getSelection: () => null, getComputedStyle: () => ({ rowGap: "0" }) });
const settle = () => new Promise<void>(resolve => setImmediate(resolve));

test("the open conversation follows a completed move even when its target lists it later", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const source: AgentRow = { machine_id: "laptop", machine_name: "laptop", id: "s1", agent: "codex", cwd: "/srv/project", status: "idle", external: false };
  const target = { ...source, machine_id: "studio", machine_name: "studio" };
  const page: AgentPage = { session: source, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, turns: 0 };
  const record = { session: "s1", agent: "codex", from: "laptop", to: "studio", project: "acme/app", at: Date.now() };
  const progress = { answer: "known" as const, phase: "done" as const, copied: 0, total: 0, files_left: 0, source_away: false, reason: null };
  S.view = "agent";
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
  // This check draws the header; the composer needs native textarea accessors.
  const query = bar.querySelector.bind(bar);
  t.mock.method(bar, "querySelector", (selector: string) => selector === "#agmsg" || selector === "#agmovebox" || selector === "#agmodel" ? null : query(selector));
  const byId = document.getElementById.bind(document);
  t.mock.method(document, "getElementById", (id: string) => id === "agshell" || id.startsWith("agpane-") ? null : byId(id));
  Object.assign(AG, { open: { machine_id: source.machine_id, agent: source.agent, session: source.id }, page, inflight: null });
  Object.assign(answers, { agent_sessions: [source], session_moves: [record], session_move_progress: progress, agent_view: { ...page, session: target }, agent_hosts: [], agent_tasks: [], editors: [] });
  MV.records.set(source.id, record);
  MV.progress.set(source.id, progress);
  bar.innerHTML = '<div id="aghere"></div>';
  calls.length = 0;
  drawMove();
  await settle();
  const stream = calls.find(c => c.cmd === "mesh_watch")?.args as { onEvent: { onmessage(event: unknown): void } };
  assert.ok(stream);
  const notify = async () => {
    stream.onEvent.onmessage({ machine_id: "studio", topic: "moves.*", revision: 1, data: {} });
    t.mock.timers.tick(100);
    await settle();
  };
  await notify();
  assert.ok(calls.some(c => c.cmd === "agent_sessions"));
  assert.equal(AG.open?.machine_id, source.machine_id);
  answers.agent_sessions = [source, target];
  await notify();
  assert.equal(AG.open?.machine_id, target.machine_id);
  assert.equal(AG.page?.session.machine_name, target.machine_name);
  assert.equal(bar.querySelector("#aghere")?.textContent, target.machine_name);
  clearTimeout(AG.timer);
});

test("replayed user images are shown even when more prompt text follows them", () => {
  const a = "image-0123456789abcdef", b = "image-fedcba9876543210";
  assert.deepEqual(userImages(`Look at this\n[image: ${a}]\nAnd this\n[image: ${b}]\nMore detail`), {
    text: "Look at this\nAnd this\nMore detail", names: [a, b],
  });
});
