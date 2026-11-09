import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { ResearchPage } from "../src/research/page-view.tsx";
import type { ResearchRow } from "../src/research/research-list.ts";

test("research updates keep the field, scroll container, and unchanged rows mounted", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  const root = createRoot(host);
  const first: ResearchRow = { session: "first", topic: "First topic", status: "researching", round: 1, sources: 1, started_at: 1, finished_at: null };
  const second: ResearchRow = { ...first, session: "second", topic: "Second topic" };
  let opened = "", starts = 0;
  const props = { list: [first, second], error: "", starting: false, draft: "question", onDraft() {}, onStart() { starts++; }, onOpen: (session: string) => { opened = session; } };
  try {
    await act(() => { root.render(createElement(ResearchPage, props)); });
    const view = host.querySelector<HTMLElement>(".cc-view"), input = host.querySelector<HTMLInputElement>("input");
    const row = [...host.querySelectorAll("button")].find(button => button.textContent === "First topic");
    assert.ok(view && input && row);
    view.scrollTop = 200;
    Object.defineProperty(input, "attachEvent", { value() {} });
    input.focus();
    input.setSelectionRange(2, 4);
    await act(() => { root.render(createElement(ResearchPage, { ...props, list: [first, { ...second, sources: 2 }] })); });
    assert.equal(host.querySelector(".cc-view"), view);
    assert.equal(view.scrollTop, 200);
    assert.equal(host.querySelector("input"), input);
    assert.equal(dom.window.document.activeElement, input);
    assert.equal(input.selectionStart, 2);
    assert.equal(input.selectionEnd, 4);
    input.value = "typed while waiting";
    input.setSelectionRange(5, 5);
    await act(() => { root.render(createElement(ResearchPage, { ...props, draft: "typed while waiting" })); });
    assert.equal(input.value, "typed while waiting");
    assert.equal(input.selectionStart, 5);
    await act(() => { root.render(createElement(ResearchPage, { ...props, draft: "" })); });
    assert.equal(input.value, "");
    assert.equal([...host.querySelectorAll("button")].find(button => button.textContent === "First topic"), row);
    await act(() => { row.click(); });
    assert.equal(opened, "first");
    await act(() => { input.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Enter", bubbles: true })); });
    assert.equal(starts, 1);
    const start = [...host.querySelectorAll("button")].find(button => button.textContent === "Research");
    assert.ok(start);
    await act(() => { start.click(); });
    assert.equal(starts, 2);
  } finally {
    await act(() => { root.unmount(); });
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key);
    }
  }
});
