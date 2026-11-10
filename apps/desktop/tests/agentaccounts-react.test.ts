import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import { AgentAccountsView, type AgentAccountActions } from "../src/settings/agentaccounts-view.tsx";
import type { AgentAccountsState } from "../src/settings/agentaccounts.ts";

test("agent account updates retain the secret input and dispatch its save", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key)); Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { render, fireEvent, cleanup } = await import("@testing-library/react");
  let secret = "";
  const noop = () => {};
  const actions: AgentAccountActions = { edit: noop, cancel: noop, save: value => { secret = value; }, revoke: noop, claudeSign: noop, claudeCancel: noop, codexSign: noop, codexCancel: noop, open: noop };
  const state: AgentAccountsState = { edit: "claude", data: { claude: null, codex_key: null, machines: [] } };
  try {
    const view = render(createElement(AgentAccountsView, { state, actions, ago: () => "today", now: 0 }));
    const input = view.getByLabelText("Claude Code token") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "example-secret" } });
    input.setSelectionRange(2, 5);
    view.rerender(createElement(AgentAccountsView, { state: { ...state, claudeError: "Retry" }, actions, ago: () => "today", now: 1 }));
    assert.equal(view.getByLabelText("Claude Code token"), input);
    assert.equal(input.value, "example-secret");
    assert.equal(dom.window.document.activeElement, input);
    assert.equal(input.selectionStart, 2); assert.equal(input.selectionEnd, 5);
    fireEvent.keyDown(input, { key: "Enter" });
    assert.equal(secret, "example-secret");
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); }
  }
});
