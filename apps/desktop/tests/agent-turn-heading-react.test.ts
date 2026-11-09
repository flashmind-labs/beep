import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { AgentTurnHeading, type TurnHeading } from "../src/agents/turn-heading-view.tsx";

test("React turn headings retain existing markup for folded, open and completed turns", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  Object.defineProperty(dom.window, "__TAURI__", { value: { event: { listen: async () => () => {} }, core: { invoke: async () => null, Channel: class {} } } });
  const { createRoot } = await import("react-dom/client");
  const { turnHeadHtml } = await import("../src/agents/turnrows.ts");
  const host = dom.window.document.getElementById("root"); assert.ok(host);
  const root = createRoot(host);
  try {
    for (const original of [
      { state: "working" as const, start: null, ms: null, stats: "3 steps", key: 7, opened: false },
      { state: "working" as const, start: 1000, ms: null, stats: "", key: 7, opened: true },
      { state: "done" as const, start: 1000, ms: 65000, stats: "2 calls", key: null, opened: false },
    ]) {
      const html = turnHeadHtml(original);
      const value: TurnHeading = { kind: "heading", id: "turn", html, state: original.state, start: original.start, ms: original.ms, stats: original.stats, turn: original.key, opened: original.opened };
      await act(() => root.render(createElement(AgentTurnHeading, { value })));
      const expected = dom.window.document.createElement("div"); expected.innerHTML = html;
      assert.equal(host.innerHTML, expected.innerHTML);
    }
  } finally {
    await act(() => root.unmount()); dom.window.close();
    for (const [key, descriptor] of previous) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); }
  }
});
