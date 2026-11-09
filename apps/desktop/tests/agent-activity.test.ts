import { bar } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { disposeLog, fillLog, setUnits } from "../src/agents/logwindow.ts";
import type { ActivityRow } from "../src/agents/activity-view.tsx";
import type { ChatBody } from "../src/ui/turn.ts";

globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;

test("streaming thoughts keep their element during refresh", () => {
  bar.innerHTML = '<div id="agbody"></div>';
  const body = bar.querySelector<ChatBody>("#agbody"); assert.ok(body);
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 800 });
  body.innerHTML = '<div class="dcoderlog"></div>';
  const log = body.querySelector<HTMLElement>(".dcoderlog"); assert.ok(log);
  const row = (text: string): ActivityRow => ({ kind: "thought", id: "1", key: "session:1", at: 1, text, content: null, ms: null, live: true, open: true, html: text });
  const units = (text: string) => [{ key: "turn", rows: () => [row(text)] }];
  setUnits(log, units("First thought"), []); fillLog(body, false);
  const thought = log.querySelector<HTMLElement>(".dthink[data-row]");
  const text = thought?.querySelector<HTMLElement>(".kk");
  assert.ok(thought && text);
  const selected = text.firstChild; assert.ok(selected);
  assert.equal(setUnits(log, units("First thought continues"), [selected]), false);
  assert.equal(text.textContent, "First thought");
  setUnits(log, units("First thought continues"), []);
  assert.equal(log.querySelector(".dthink[data-row]"), thought);
  assert.equal(thought.querySelector(".kk"), text);
  assert.match(text.textContent ?? "", /continues/);
  log.remove();
  body.append(log);
  assert.equal(log.querySelector(".dthink[data-row]"), thought);
  disposeLog(log);
  assert.equal(log.querySelector(".dthink[data-row]"), null);
});


test("tool row updates preserve its focused action and defer selected text", () => {
  bar.innerHTML = '<div id="agbody"><div class="dcoderlog"></div></div>';
  const body = bar.querySelector<ChatBody>("#agbody"); assert.ok(body);
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 800 });
  const log = body.querySelector<HTMLElement>(".dcoderlog"); assert.ok(log);
  const row = (target: string): ActivityRow => ({ kind: "tool", id: "2", html: target, at: 2, failed: false, open: false, artifact: null, toggle: true, keyboard: true, mark: "", after: "", layout: { kind: "standard", parts: { kind: "command", verb: "Running", target, detail: "", bad: false }, tip: target, since: null } });
  const units = (target: string) => [{ key: "turn", rows: () => [row(target)] }];
  setUnits(log, units("first"), []); fillLog(body, false);
  const tool = log.querySelector<HTMLElement>('[data-row="2"]'); assert.ok(tool);
  const text = tool.querySelector("code")?.firstChild; assert.ok(text);
  tool.focus();
  assert.equal(setUnits(log, units("second"), [text]), false);
  assert.equal(text.textContent, "first");
  assert.equal(setUnits(log, units("second"), []), true);
  assert.equal(log.querySelector('[data-row="2"]'), tool);
  assert.equal(document.activeElement, tool);
  assert.equal(tool.querySelector("code")?.textContent, "second");
  disposeLog(log);
});

test("grouped tools retain their controls while the group changes", () => {
  bar.innerHTML = '<div id="agbody"><div class="dcoderlog"></div></div>';
  const body = bar.querySelector<ChatBody>("#agbody"); assert.ok(body);
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 800 });
  const log = body.querySelector<HTMLElement>(".dcoderlog"); assert.ok(log);
  const tool = (at: number) => ({ kind: "tool" as const, id: String(at), html: `tool ${at}`, at, failed: false, open: false, artifact: null, toggle: true, keyboard: true, mark: "", after: "", layout: { kind: "standard" as const, parts: { kind: "read" as const, verb: "Read", target: `file${at}`, detail: "", bad: false }, tip: `file${at}`, since: null } });
  const run = (open: boolean): ActivityRow => ({ kind: "run", id: "1", html: String(open), at: 1, toolKind: "read", label: "Read 2 files", summary: "file1, file2", open, tools: [tool(1), { ...tool(2), after: '<span class="pics"><img class="pic" alt="example"></span>' }] });
  const units = (open: boolean) => [{ key: "turn", rows: () => [run(open)] }];
  setUnits(log, units(false), []); fillLog(body, false);
  const group = log.querySelector<HTMLElement>(".dtrun"); assert.ok(group);
  const button = group.querySelector<HTMLButtonElement>("[data-run]"); assert.ok(button);
  button.focus();
  setUnits(log, units(true), []);
  assert.equal(log.querySelector(".dtrun"), group);
  assert.equal(document.activeElement, button);
  assert.equal(button.getAttribute("aria-expanded"), "true");
  assert.equal(group.querySelectorAll(".dcoderen.tool").length, 2);
  assert.ok(group.querySelector('.pics img[alt="example"]'));
  disposeLog(log);
});

test("subagent activity retains its action while status changes", () => {
  bar.innerHTML = '<div id="agbody"><div class="dcoderlog"></div></div>';
  const body = bar.querySelector<ChatBody>("#agbody"); assert.ok(body);
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 800 });
  const log = body.querySelector<HTMLElement>(".dcoderlog"); assert.ok(log);
  const row = (meta: string): ActivityRow => ({ kind: "subagents", id: "1", at: 1, html: meta, heading: null, counts: "", rows: [{ at: 1, title: "Inspect", agent: "Explorer", meta, mark: "", target: { kind: "child", id: "child", key: "key" }, dismiss: false, stoppable: true, result: null, prompt: { text: "Read the file", open: true } }] });
  const units = (meta: string) => [{ key: "turn", rows: () => [row(meta)] }];
  setUnits(log, units("working"), []); fillLog(body, false);
  const button = log.querySelector<HTMLButtonElement>("[data-kid]"); assert.ok(button);
  button.focus();
  setUnits(log, units("working · 1s"), []);
  assert.equal(log.querySelector("[data-kid]"), button);
  assert.equal(document.activeElement, button);
  assert.equal(button.dataset.key, "key");
  assert.equal(log.querySelector("[data-prompt]")?.getAttribute("aria-expanded"), "true");
  assert.equal(log.querySelector(".dsubprompt pre")?.textContent, "Read the file");
  disposeLog(log);
});
