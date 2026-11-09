import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement, type ComponentProps } from "react";
import { ModelView } from "../src/settings/model-view.tsx";

test("model settings preserve focused options during updates and handle each action once", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { render, fireEvent, cleanup } = await import("@testing-library/react");
  const calls: string[] = [];
  const props: ComponentProps<typeof ModelView> = { model: { choices: [{ id: "a", name: "Example A", input_price: null, output_price: null, sees_images: true, default: true }, { id: "b", name: "Example B", input_price: null, output_price: null, sees_images: false, default: false }], selected: "a", thinking: "medium", auto: false, auto_available: true }, selected: 0, agents: true, agentNames: ["Example"], error: "", hints: { items: [], window: false, model: null, link: null }, thinkingLevels: ["off", "low", "medium", "high"], price: () => "No price listed", capitalize: value => value,
    pick: index => { calls.push(`pick:${index}`); }, setThinking: level => { calls.push(`level:${level}`); }, setAuto: () => { calls.push("auto"); }, setAgents: () => { calls.push("agents"); } };
  try {
    const view = render(createElement(ModelView, props));
    let bubbled = 0;
    dom.window.document.body.addEventListener("click", () => { bubbled++; });
    const row = view.getByRole("option", { name: /Example B/ });
    row.focus();
    view.rerender(createElement(ModelView, { ...props, selected: 1, error: "Temporary error" }));
    assert.equal(view.getByRole("option", { name: /Example B/ }), row);
    assert.equal(dom.window.document.activeElement, row);
    assert.equal(row.getAttribute("aria-selected"), "true");
    fireEvent.click(row);
    fireEvent.click(view.getByRole("switch", { name: "Auto" }));
    fireEvent.click(view.getByRole("switch", { name: "Agents" }));
    fireEvent.click(view.getByRole("button", { name: "high" }));
    assert.deepEqual(calls, ["pick:1", "auto", "agents", "level:high"]);
    assert.equal(bubbled, 0, "legacy bar click delegation must not repeat React actions");
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
