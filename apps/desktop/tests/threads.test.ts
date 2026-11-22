import test from "node:test";
import assert from "node:assert/strict";
import { chatOf, threadsOf } from "../src/agents/threads-state.ts";
import type { AgentSession } from "../src/agents/agents.ts";

let at = 0;
const row = (id: string, over: Partial<AgentSession> = {}): AgentSession => ({
  machine_id: "m1", machine_name: "Build Box", id, agent: "claude", title: id, cwd: "/srv/project",
  status: "idle", doing: null, started_at: ++at, last_activity: at, external: false, question: null, parent: null, assistant: false, ...over,
});
const under = (id: string, key = "") => ({ machine_id: "m1", id, key });
const ids = (rows: AgentSession[], main: AgentSession) => threadsOf(rows, main).map(thread => thread.row.id);

test("a chat's threads are its direct children, oldest first", () => {
  const main = row("main"), late = row("late", { parent: under("main") });
  const rows = [
    main, row("other"), late,
    row("early", { parent: under("main"), started_at: 0 }),
    row("grandkid", { parent: under("late") }),
    row("theirs", { parent: under("other") }),
  ];

  assert.deepEqual(ids(rows, main), ["early", "late"]);
  assert.deepEqual(ids(rows, late), ["grandkid"]);
  assert.deepEqual(ids(rows, row("alone")), []);
});

test("reviewers and background agents are no threads", () => {
  const main = row("main");
  const rows = [main, row("reviewer", { parent: under("main"), reviewer: true }), row("quiet", { parent: under("main"), background: true }), row("kid", { parent: under("main") })];

  assert.deepEqual(ids(rows, main), ["kid"]);
});

test("a thread is found by the id its parent's node first listed it under", () => {
  const main = row("agent-uuid");

  assert.deepEqual(ids([main, row("kid", { parent: under("agent-uuid", "short1") })], main), ["kid"]);
  assert.deepEqual(ids([row("short1"), row("kid", { parent: under("agent-uuid", "short1") })], row("short1")), ["kid"]);
});

test("a thread shows its name and what it asks or came to", () => {
  const main = row("main");
  const question = { id: 1, text: "EU only, or US too?", detail: null, options: [] };
  const rows = [
    main,
    row("a", { parent: under("main"), title: "Sodium-ion cost curve", status: "working" }),
    row("b", { parent: under("main"), title: null, status: "idle", reply_html: "Pilot lines <b>2027</b>" }),
    row("c", { parent: under("main"), status: "asking", question, reply_html: "Earlier" }),
    row("d", { parent: under("main"), status: "ended" }),
    row("e", { parent: under("main"), status: "starting" }),
  ];

  assert.deepEqual(threadsOf(rows, main).map(({ name, detail }) => ({ name, detail })), [
    { name: "Sodium-ion cost curve", detail: null },
    { name: "claude", detail: { kind: "reply", html: "Pilot lines <b>2027</b>" } },
    { name: "c", detail: { kind: "question", text: "EU only, or US too?" } },
    { name: "d", detail: null },
    { name: "e", detail: null },
  ]);
});

test("only an assistant chat and its threads are part of a chat", () => {
  const chat = row("chat", { assistant: true }), thread = row("thread", { parent: under("chat") });
  const coder = row("coder"), kid = row("kid", { parent: under("coder") });
  const rows = [chat, thread, row("deep", { parent: under("thread") }), row("reviewer", { parent: under("chat"), reviewer: true }), coder, kid];

  assert.equal(chatOf(rows, chat), chat);
  assert.equal(chatOf(rows, thread), chat);
  assert.equal(chatOf(rows, rows[2]), chat);
  assert.equal(chatOf(rows, rows[3]), null);
  assert.equal(chatOf(rows, coder), null);
  assert.equal(chatOf(rows, kid), null);
});
