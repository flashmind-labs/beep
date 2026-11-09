import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import { JSDOM } from "jsdom";
import assert from "node:assert/strict";
import { ConversationHistory, conversationKey, isConversationKey, nextConversation } from "../src/agents/conversation-state.ts";
import { initConversationSwitcher } from "../src/agents/conversation-switcher.ts";
import type { AgentRow } from "../src/agents/agents.ts";

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
  assert.ok(calls.every(call => ["agent_sessions", "session_moves", "session_copies", "log_webview"].includes(call.cmd)));
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
