import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import { FirstRunScreen } from "../src/center/firstrun-view.tsx";

test("first run moves focus through the link flow and removes its fields when going back", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { render, fireEvent, cleanup } = await import("@testing-library/react");
  let completions = 0;
  const join = (done: () => void, beside: Node) => {
    const input = dom.window.document.createElement("input");
    input.setAttribute("aria-label", "Desktop ticket");
    const link = dom.window.document.createElement("button");
    link.textContent = "Link";
    link.addEventListener("click", done);
    return { input, nodes: [input, link, beside] };
  };
  try {
    const view = render(createElement(FirstRunScreen, { done: () => { completions++; }, join }));
    const start = view.getByRole("button", { name: /^Start/ });
    assert.equal(dom.window.document.activeElement, start);
    fireEvent.click(view.getByRole("button", { name: /^Link another desktop/ }));
    const field = view.getByLabelText("Desktop ticket");
    assert.equal(dom.window.document.activeElement, field);
    fireEvent.click(view.getByRole("button", { name: "Back" }));
    assert.equal(field.isConnected, false);
    assert.equal(dom.window.document.activeElement, view.getByRole("button", { name: /^Start/ }));
    fireEvent.click(view.getByRole("button", { name: /^Link another desktop/ }));
    fireEvent.click(view.getByRole("button", { name: "Link" }));
    assert.equal(completions, 1);
  } finally {
    cleanup();
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
