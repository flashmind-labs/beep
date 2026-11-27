import test from "node:test";
import assert from "node:assert/strict";
import { chatActivity, chatOf, chatRail, chatsAsking, threadFolders, threadsByChat } from "../src/stores/agents/threads-state.ts";
import type { AgentSession } from "../src/stores/agents/agents-state.ts";

let at = 0;
const row = (id: string, over: Partial<AgentSession> = {}): AgentSession => ({
  machine_id: "m1", machine_name: "Build Box", id, agent: "claude", title: id, cwd: "/srv/project",
  status: "idle", doing: null, started_at: ++at, last_activity: at, external: false, question: null, parent: null, assistant: false, persona: null, ...over,
});
const under = (id: string, key = "") => ({ machine_id: "m1", id, key });
const threadsOf = (rows: AgentSession[], main: AgentSession) => {
  const listed = rows.find(row => row.machine_id === main.machine_id && row.id === main.id);
  return (listed && threadsByChat(rows).get(listed)) ?? [];
};
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

test("a thread listed without its parent link is placed by its folder", () => {
  const chat = row("chat", { assistant: true, cwd: "/srv/.beep/assistant/k3f9" });
  const thread = row("thread", { cwd: "/srv/.beep/assistant/k3f9/custody", status: "ended" });
  const windows = row("win", { assistant: true, machine_id: "m2", cwd: "C:\\Users\\user\\.beep\\assistant\\p7m1" });
  const theirs = row("theirs", { machine_id: "m2", cwd: "C:\\Users\\user\\.beep\\assistant\\p7m1\\prices" });
  const rows = [
    chat, thread, windows, theirs,
    row("elsewhere", { machine_id: "m2", cwd: "/srv/.beep/assistant/k3f9/custody" }),
    row("deeper", { cwd: "/srv/.beep/assistant/k3f9/custody/data" }),
    row("project", { assistant: true, cwd: "/srv/project" }),
    row("coder", { cwd: "/srv/project/app" }),
  ];

  assert.deepEqual(ids(rows, chat), ["thread"]);
  assert.deepEqual(ids(rows, windows), ["theirs"]);
  assert.equal(chatOf(rows, thread), chat);
  assert.equal(chatOf(rows, rows[7]), null);
  assert.deepEqual(ids(rows, rows[6]), []);
});

test("a chat's rail lists the chat and its running threads, then the ended ones, newest first", () => {
  const chat = row("chat", { assistant: true, status: "working" });
  const runs = row("runs", { parent: under("chat"), status: "working" });
  const old = row("old", { parent: under("chat"), status: "ended", last_activity: 5 });
  const recent = row("recent", { parent: under("chat"), status: "ended", last_activity: 9 });
  const rows = [old, chat, recent, runs, row("other", { assistant: true })];

  const rail = chatRail(rows, runs);
  assert.equal(rail?.chat, chat);
  assert.deepEqual(rail?.running.map(one => one.id), ["chat", "runs"]);
  assert.deepEqual(rail?.past.map(one => one.id), ["recent", "old"]);
});

test("a chat that ended heads the past, and one without threads has no rail", () => {
  const chat = row("chat", { assistant: true, status: "ended" }), thread = row("thread", { parent: under("chat"), status: "ended" });

  assert.deepEqual(chatRail([chat, thread], chat)?.past.map(one => one.id), ["chat", "thread"]);
  assert.equal(chatRail([chat], chat), null);
  assert.equal(chatRail([row("plain")], row("plain")), null);
});

test("a chat's files leave out its threads' folders, and a thread's its own threads'", () => {
  const chat = row("chat", { assistant: true, cwd: "/srv/.beep/assistant/c1" });
  const one = row("one", { parent: under("chat"), cwd: "/srv/.beep/assistant/c1/pricing" });
  const deep = row("deep", { parent: under("one"), cwd: "/srv/.beep/assistant/c1/pricing/tiers" });
  const elsewhere = row("elsewhere", { parent: under("chat"), cwd: "/srv/project" });
  const rows = [chat, one, deep, elsewhere];

  assert.deepEqual([...threadFolders(rows, chat)], ["pricing"]);
  assert.deepEqual([...threadFolders(rows, one)], ["tiers"]);
  assert.deepEqual([...threadFolders(rows, deep)], []);
});

test("a chat counts threads that are working or need an answer", () => {
  const quiet = row("quiet", { assistant: true }), busy = row("busy", { assistant: true });
  const rows = [
    quiet, busy,
    row("asks", { parent: under("quiet"), status: "asking" }),
    row("works", { parent: under("quiet"), status: "working" }),
    row("done", { parent: under("quiet"), status: "ended" }),
  ];
  const threads = threadsByChat(rows);

  assert.deepEqual(chatActivity(threads.get(quiet) ?? []), { asking: 1, working: 1 });
  assert.deepEqual(chatActivity([]), { asking: 0, working: 0 });
  assert.equal(chatsAsking(rows), 1);
  assert.equal(chatsAsking([row("solo", { assistant: true, status: "asking" })]), 1);
});
