import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import { JSDOM } from "jsdom";
import assert from "node:assert/strict";
import { ConversationHistory, conversationKey, conversationState, firstConversation, groupConversations, isConversationKey, nextConversation, retitle } from "../src/agents/conversation-state.ts";
import { initConversationSwitcher } from "../src/agents/conversation-switcher.tsx";
import type { AgentRow } from "../src/agents/agents-state.ts";

const session = (id: string, extra: Partial<AgentRow> = {}): AgentRow => ({ machine_id: "machine", machine_name: "build-box", agent: "codex", id, title: id, cwd: "/srv/project", status: "idle", external: false, last_activity: 1, ...extra });

test("conversation visits order the picker without changing while cycling", () => {
  const history = new ConversationHistory();
  const a = session("a"), b = session("b"), c = session("c", { last_activity: 50 });
  history.visit(b); history.visit(a);
  const rows = history.order([c, b, a], a);
  assert.deepEqual(rows.map(s => s.id), ["a", "b", "c"]);
  assert.equal(nextConversation(0, 3, false), 1);
  assert.equal(nextConversation(2, 3, false), 0);
  assert.equal(nextConversation(0, 3, true), 2);
  assert.equal(nextConversation(-1, 3, false), 0);
  assert.equal(nextConversation(-1, 3, true), 2);
  assert.equal(nextConversation(0, 0, false), -1);
  assert.deepEqual(history.order([c, b, a], a), rows);
  history.visit(b);
  assert.deepEqual(history.order([a, b, c], b).map(s => s.id), ["b", "a", "c"]);
});

test("latest duplicate wins, ended sessions stay out, and machine and harness identify conversations", () => {
  const history = new ConversationHistory(), a = session("a");
  const elsewhere = session("a", { machine_id: "other" });
  const harness = session("a", { agent: "claude" });
  const rows = history.order([a, elsewhere, harness, { ...a, status: "ended" }], null);
  assert.deepEqual(rows, [elsewhere, harness]);
  assert.notEqual(conversationKey(a), conversationKey(elsewhere));
  assert.notEqual(conversationKey(a), conversationKey(harness));
});

test("child conversations stay out of the picker even when current or their parent is absent", () => {
  const history = new ConversationHistory();
  const parent = session("parent");
  const child = session("child", { parent: { machine_id: "machine", id: "parent", key: "" } });
  const native = session("subagent:codex:worker");
  const orphan = session("orphan", { parent: { machine_id: "remote", id: "missing", key: "" } });
  history.visit(child);
  assert.deepEqual(history.order([child, parent, native, orphan], child), [parent]);
});

test("subagents sit under their parent in the order they started, and a known parent that is not listed heads them", () => {
  const parent = session("p", { agent: "claude" });
  const late = session("subagent:claude:p:late", { agent: "claude", status: "working", started_at: 20 });
  const early = session("subagent:claude:p:early", { agent: "claude", status: "idle", started_at: 10 });
  const other = session("o");
  const ended = session("gone", { agent: "claude", status: "ended" });
  const orphan = session("subagent:claude:gone:x", { agent: "claude", status: "working" });
  const lost = session("subagent:claude:unknown:y", { agent: "claude", status: "working" });
  const parents = new Map([[late, parent], [early, parent], [orphan, ended]]);
  const choices = groupConversations([late, other, parent, early, orphan, lost], row => parents.get(row) ?? null);
  assert.deepEqual(choices.map(c => [c.row.id, c.depth]), [
    ["p", 0], ["subagent:claude:p:early", 1], ["subagent:claude:p:late", 1], ["o", 0], ["gone", 0], ["subagent:claude:gone:x", 1], ["subagent:claude:unknown:y", 0],
  ]);
  assert.deepEqual(choices.map(c => conversationState(c.row)), ["Ready", "Done", "Working", "Ready", "Ended", "Working", "Working"]);
  const loop = groupConversations([parent, other], row => row === parent ? other : parent);
  assert.equal(loop.length, 2);
});

test("the first press passes over the current conversation's subagents", () => {
  const rows = [{ depth: 0 }, { depth: 1 }, { depth: 1 }, { depth: 0 }].map((c, i) => ({ row: session(String(i)), ...c }));
  assert.equal(firstConversation(rows, 0, false), 3);
  assert.equal(firstConversation(rows, 0, true), 3);
  assert.equal(firstConversation(rows, 1, false), 2);
  assert.equal(firstConversation(rows.slice(0, 3), 0, false), 1);
  assert.equal(firstConversation(rows, -1, false), 0);
});

test("Option+Tab accepts reverse cycling but excludes other modifiers and composition", () => {
  const key = { key: "Tab", altKey: true, ctrlKey: false, metaKey: false, isComposing: false };
  assert.ok(isConversationKey(key));
  for (const extra of [{ altKey: false }, { ctrlKey: true }, { metaKey: true }, { isComposing: true }, { key: "Enter" }]) assert.ok(!isConversationKey({ ...key, ...extra }));
});

test("holding cycles without actions, release opens once, Escape and blur cancel, and closed rows do not reopen", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  Object.defineProperty(globalThis, "window", { value: dom.window, configurable: true, writable: true });
  Object.defineProperty(globalThis, "document", { value: dom.window.document, configurable: true, writable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: dom.window.HTMLElement, configurable: true, writable: true });
  Object.defineProperty(globalThis, "Element", { value: dom.window.Element, configurable: true, writable: true });
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  const listeners = new Map<string, EventListener[]>();
  const add = (name: string, listener: EventListener) => { listeners.set(name, [...(listeners.get(name) ?? []), listener]); };
  document.addEventListener = add as typeof document.addEventListener;
  document.removeEventListener = ((name: string, listener: EventListener) => { listeners.set(name, (listeners.get(name) ?? []).filter(l => l !== listener)); }) as typeof document.removeEventListener;
  window.addEventListener = add as typeof window.addEventListener;
  const send = (name: string, values: Record<string, unknown>) => {
    let stopped = false;
    const event = { altKey: true, preventDefault() {}, stopImmediatePropagation() { stopped = true; }, ...values };
    for (const listener of [...(listeners.get(name) ?? [])]) { if (!stopped) listener(event as unknown as Event); }
  };
  answers.agent_sessions = [];
  const a = session("a"), b = session("b"), c = session("c");
  let rows = [a, b, c], current = a;
  const opened: AgentRow[] = [];
  initConversationSwitcher({ rows: () => rows, current: () => current, open: s => { opened.push(s); current = s; } });
  await new Promise(resolve => setImmediate(resolve));
  calls.length = 0;
  const tab = () => send("keydown", { key: "Tab" });
  const picked = () => document.querySelector('[aria-selected="true"]')?.textContent;
  tab();
  assert.match(picked()!, /b/);
  tab();
  assert.match(picked()!, /c/);
  assert.deepEqual(opened, []);
  assert.ok(calls.every(call => ["agent_sessions", "session_moves", "log_webview"].includes(call.cmd)));
  send("keyup", { key: "Tab" });
  assert.ok(document.querySelector('[role="dialog"]'));
  send("keyup", { key: "Alt", altKey: false });
  assert.deepEqual(opened, [c]);
  assert.equal(document.querySelector('[role="dialog"]'), null);
  send("keyup", { key: "Alt", altKey: false });
  assert.deepEqual(opened, [c]);
  tab(); send("keydown", { key: "Escape" }); send("keyup", { key: "Alt", altKey: false });
  assert.deepEqual(opened, [c]);
  tab(); send("blur", {});
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.deepEqual(opened, [c]);
  current = a;
  tab();
  rows = [a, { ...b, status: "ended" }, c];
  send("keyup", { key: "Alt", altKey: false });
  assert.deepEqual(opened, [c]);
  assert.ok(!calls.some(call => call.cmd === "session_move"));
  dom.window.close();
});

test("a saved title retitles only its own session, and a cleared one takes the agent's title back", () => {
  const open = session("a", { title: "add an issue about router actions" });
  const change = { machine_id: "machine", agent: "codex", session: "a", title: "Router actions" };
  assert.equal(retitle(open, change)?.title, "Router actions");
  assert.equal(retitle(open, { ...change, session: "b" }), null);
  assert.equal(retitle(open, { ...change, agent: "claude" }), null);
  assert.equal(retitle(open, { ...change, machine_id: "other" }), null);
  // A cleared title comes with the agent's own.
  assert.equal(retitle(open, { ...change, title: "Generated title" })?.title, "Generated title");
  assert.equal(retitle(open, { ...change, title: null })?.title, null);
});
