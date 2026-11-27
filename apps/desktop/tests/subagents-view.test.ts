import { JSDOM } from "jsdom";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import test from "node:test";
import assert from "node:assert/strict";
import { SubagentsView, type SubagentRow } from "../src/parts/session/subagents-view.tsx";

test("subagent updates keep focused rows and their actions", () => {
  const dom = new JSDOM("<body><div id='root'></div></body>");
  Object.assign(globalThis, { window: dom.window, document: dom.window.document });
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  const root = createRoot(host);
  const row: SubagentRow = { at: 1, title: "Check files", agent: "Explorer", meta: "working", mark: "none", target: { kind: "child", id: "child", key: "key" }, dismiss: false, stoppable: true, result: null, prompt: null };
  flushSync(() => root.render(createElement(SubagentsView, { rows: [row] })));
  const button = host.querySelector("button");
  assert.ok(button);
  button.focus();
  flushSync(() => root.render(createElement(SubagentsView, { rows: [{ ...row, meta: "working · 1s" }] })));
  assert.equal(host.querySelector("button"), button);
  assert.equal(dom.window.document.activeElement, button);
  assert.equal(button.dataset.kid, "child");
  assert.ok(button.querySelector("[data-kidstop]"));
  root.unmount();
  dom.window.close();
});
