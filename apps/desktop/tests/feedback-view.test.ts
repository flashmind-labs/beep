import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { FeedbackControl, FeedbackNote, type FeedbackState } from "../src/parts/review/feedback-view.tsx";

test("feedback control keeps focus and its picker across updates", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  const root = createRoot(host);
  let button: HTMLButtonElement | null = null, slot: HTMLDivElement | null = null, clicked = 0, dismissed = 0;
  const setButton = (value: HTMLButtonElement | null) => { button = value; };
  const setSlot = (value: HTMLDivElement | null) => { slot = value; };
  const props = { setButton, setSlot, onClick: () => { clicked++; }, picker: null };
  const render = (state: FeedbackState) => root.render(createElement(FeedbackControl, { ...props, state }));
  try {
    await act(() => { render({ kind: "ready", label: "Review with agent", hint: "Pick a model", reason: "Machine offline", open: false }); });
    const control = host.querySelector<HTMLButtonElement>("button");
    assert.ok(control);
    assert.equal(control.disabled, true);
    assert.equal(control.title, "Machine offline");
    await act(() => { render({ kind: "ready", label: "Review with agent", hint: "Pick a model", reason: "", open: true }); });
    assert.equal(host.querySelector("button"), control);
    assert.equal(control.getAttribute("aria-expanded"), "true");
    control.focus();
    await act(() => { control.click(); });
    assert.equal(clicked, 1);
    const pickerSlot = host.querySelector("div");
    assert.ok(pickerSlot);
    const picker = dom.window.document.createElement("input");
    pickerSlot.append(picker);
    await act(() => { render({ kind: "running", label: "Reviewing", agent: "Example", hint: "Open the review" }); });
    assert.equal(host.querySelector("button"), control);
    assert.equal(dom.window.document.activeElement, control);
    assert.equal(pickerSlot.firstElementChild, picker);
    assert.equal(control.textContent, "Reviewing Example");
    assert.equal(control.getAttribute("aria-expanded"), null);
    await act(() => { render({ kind: "starting" }); });
    assert.equal(control.disabled, true);
    await act(() => { root.render(createElement(FeedbackNote, { text: "Review failed", onDismiss: () => { dismissed++; } })); });
    assert.equal(host.querySelector("pre")?.textContent, "Review failed");
    await act(() => { host.querySelector<HTMLButtonElement>("button")?.click(); });
    assert.equal(dismissed, 1);
    assert.equal(button, null);
    assert.equal(slot, null);
  } finally {
    await act(() => { root.unmount(); });
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key);
    }
  }
});
