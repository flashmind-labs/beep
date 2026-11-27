import { bar } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createLog, disposeLog, fillLog, setUnits } from "../src/parts/session/logwindow.tsx";
import { TranscriptLog } from "../src/parts/session/log-shell-view.tsx";
import type { ActivityRow } from "../src/parts/session/activity-view.tsx";
import { createDocumentView } from "../src/ui/document-root.tsx";
import type { ChatBody } from "../src/ui/turn.ts";

globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;

/// A session body with an empty log React draws.
function mount() {
  bar.innerHTML = '<div id="agbody"></div>';
  const body = bar.querySelector<ChatBody>("#agbody"); assert.ok(body);
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 800 });
  const w = createLog(body), view = createDocumentView(body);
  flushSync(() => view.render(createElement("div", { className: "dcoderlog", ref: w.attach }, createElement(TranscriptLog, { log: w }))));
  const log = body.querySelector<HTMLElement>(".dcoderlog"); assert.ok(log);
  return { body, log, w, done: () => { disposeLog(w); view.unmount(); } };
}

test("streaming thoughts keep their element during refresh", () => {
  const { body, log, w, done } = mount();
  const row = (text: string): ActivityRow => ({ kind: "thought", id: "1", key: "session:1", at: 1, text, content: null, ms: null, live: true, open: true });
  const units = (text: string) => [{ key: "turn", rows: () => [row(text)], prompt: null }];
  setUnits(w, units("First thought"), []); fillLog(body, false);
  const thought = log.querySelector<HTMLElement>(".dthink[data-row]");
  const text = thought?.querySelector<HTMLElement>(".kk");
  assert.ok(thought && text);
  for (let i = 0; i < 5; i++) { setUnits(w, units("First thought"), []); fillLog(body, false); }
  assert.equal(log.querySelector(".dthink[data-row]"), thought);
  assert.equal(thought.querySelector(".kk"), text);
  const selected = text.firstChild; assert.ok(selected);
  assert.equal(setUnits(w, units("First thought continues"), [selected]), false);
  assert.equal(text.textContent, "First thought");
  setUnits(w, units("First thought continues"), []);
  assert.equal(log.querySelector(".dthink[data-row]"), thought);
  assert.equal(thought.querySelector(".kk"), text);
  assert.match(text.textContent ?? "", /continues/);
  done();
  assert.equal(body.querySelector(".dthink[data-row]"), null);
});


test("tool row updates preserve its focused action and defer selected text", () => {
  const { body, log, w, done } = mount();
  const row = (target: string): ActivityRow => ({ kind: "tool", id: "2", at: 2, failed: false, open: false, artifact: null, toggle: true, keyboard: true, mark: "none", media: [], body: null, pictures: "", layout: { kind: "standard", parts: { kind: "command", verb: "Running", target, detail: "", bad: false }, tip: target, since: null } });
  const units = (target: string) => [{ key: "turn", rows: () => [row(target)], prompt: null }];
  setUnits(w, units("first"), []); fillLog(body, false);
  const tool = log.querySelector<HTMLElement>('[data-row="2"]'); assert.ok(tool);
  const text = tool.querySelector("code")?.firstChild; assert.ok(text);
  tool.focus();
  assert.equal(setUnits(w, units("second"), [text]), false);
  assert.equal(text.textContent, "first");
  assert.equal(setUnits(w, units("second"), []), true);
  assert.equal(log.querySelector('[data-row="2"]'), tool);
  assert.equal(document.activeElement, tool);
  assert.equal(tool.querySelector("code")?.textContent, "second");
  done();
});

test("grouped tools retain their controls while the group changes", () => {
  const { body, log, w, done } = mount();
  const tool = (at: number) => ({ kind: "tool" as const, id: String(at), at, failed: false, open: false, artifact: null, toggle: true, keyboard: true, mark: "none" as const, media: [], body: null, pictures: "", layout: { kind: "standard" as const, parts: { kind: "read" as const, verb: "Read", target: `file${at}`, detail: "", bad: false }, tip: `file${at}`, since: null } });
  const run = (open: boolean): ActivityRow => ({ kind: "run", id: "1", at: 1, toolKind: "read", label: "Read 2 files", summary: "file1, file2", open, tools: [tool(1), { ...tool(2), pictures: '<img class="pic" alt="example">' }] });
  const units = (open: boolean) => [{ key: "turn", rows: () => [run(open)], prompt: null }];
  setUnits(w, units(false), []); fillLog(body, false);
  const group = log.querySelector<HTMLElement>(".dtrun"); assert.ok(group);
  const button = group.querySelector<HTMLButtonElement>("[data-run]"); assert.ok(button);
  button.focus();
  setUnits(w, units(true), []);
  assert.equal(log.querySelector(".dtrun"), group);
  assert.equal(document.activeElement, button);
  assert.equal(button.getAttribute("aria-expanded"), "true");
  assert.equal(group.querySelectorAll(".dcoderen.tool").length, 2);
  assert.ok(group.querySelector('.pics img[alt="example"]'));
  done();
});

test("subagent activity retains its action while status changes", () => {
  const { body, log, w, done } = mount();
  const row = (meta: string): ActivityRow => ({ kind: "subagents", id: "1", at: 1, heading: null, counts: "", rows: [{ at: 1, title: "Inspect", agent: "Explorer", meta, mark: "running", target: { kind: "child", id: "child", key: "key" }, dismiss: false, stoppable: true, result: null, prompt: { text: "Read the file", open: true } }] });
  const units = (meta: string) => [{ key: "turn", rows: () => [row(meta)], prompt: null }];
  setUnits(w, units("working"), []); fillLog(body, false);
  const button = log.querySelector<HTMLButtonElement>("[data-kid]"); assert.ok(button);
  button.focus();
  setUnits(w, units("working · 1s"), []);
  assert.equal(log.querySelector("[data-kid]"), button);
  assert.equal(document.activeElement, button);
  assert.equal(button.dataset.key, "key");
  assert.equal(log.querySelector("[data-prompt]")?.getAttribute("aria-expanded"), "true");
  assert.equal(log.querySelector(".dsubprompt pre")?.textContent, "Read the file");
  done();
});

test("made-file notes keep their controls and focus during updates", () => {
  const { body, log, w, done } = mount();
  const files = [{ path: "/srv/project/a.txt", name: "a.txt" }, { path: "/srv/project/b.txt", name: "b.txt" }];
  const row = (open: boolean): ActivityRow => ({ kind: "note", id: "made:1", at: 1, value: { kind: "made", open, files: { files, counts: "2 documents", groups: [{ group: "documents", label: "Documents", files }] } } });
  const units = (open: boolean) => [{ key: "turn", rows: () => [row(open)], prompt: null }];
  setUnits(w, units(false), []); fillLog(body, false);
  const button = log.querySelector<HTMLButtonElement>("[data-madeat]"); assert.ok(button);
  button.focus();
  setUnits(w, units(true), []);
  assert.equal(log.querySelector("[data-madeat]"), button);
  assert.equal(document.activeElement, button);
  assert.equal(button.getAttribute("aria-expanded"), "true");
  assert.equal(log.querySelectorAll("[data-made]").length, 2);
  done();
});

test("folded activity pictures retain the image when other turns update", () => {
  const { body, log, w, done } = mount();
  const picture: ActivityRow = { kind: "pictures", id: "1", at: 1, folded: true, pictures: '<img class="pic" alt="example">' };
  const units = (text: string) => [{ key: "turn", rows: () => [picture, { kind: "note" as const, id: "note", at: 2, value: { kind: "other" as const, text } }], prompt: null }];
  setUnits(w, units("before"), []); fillLog(body, false);
  const image = log.querySelector(".dfoldpics img"); assert.ok(image);
  setUnits(w, units("after"), []);
  assert.equal(log.querySelector(".dfoldpics img"), image);
  done();
});

test("parent link and older-history notice keep their nodes across updates", () => {
  const { body, log, w, done } = mount();
  const crumb: ActivityRow = { kind: "crumb", id: "up", parent: "Parent", title: "Child" };
  const units = (text: string) => [{ key: "turn", rows: () => [crumb, { kind: "older" as const, id: "older", text }], prompt: null }];
  setUnits(w, units("Loading earlier messages"), []); fillLog(body, false);
  const parent = log.querySelector<HTMLButtonElement>("#agup"); assert.ok(parent);
  parent.focus();
  setUnits(w, units("Earlier messages loaded"), []);
  assert.equal(log.querySelector("#agup"), parent);
  assert.equal(document.activeElement, parent);
  assert.match(log.textContent ?? "", /Earlier messages loaded/);
  done();
});
