import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import { AgentAccountsView, type AgentAccountActions } from "../src/screens/settings/agentaccounts-view.tsx";
import type { AgentAccountsState } from "../src/stores/settings/agentaccounts.ts";

test("the phone offers a pasted Claude Code token, not a local sign-in", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  Object.assign(dom.window, { __BEEP_PHONE__: true });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key)); Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { render, cleanup } = await import("@testing-library/react");
  const noop = () => {};
  const actions: AgentAccountActions = { add: noop, addSave: noop, addSign: noop, removeNamed: noop, edit: noop, cancel: noop, save: noop, revoke: noop, claudeSign: noop, claudeCancel: noop, machineSign: noop, codexSign: noop, codexCancel: noop, open: noop };
  const state: AgentAccountsState = { edit: null, data: { claude: null, codex_key: null, machines: [], named: [] } };
  try {
    const view = render(createElement(AgentAccountsView, { state, actions, ago: () => "today", now: 0 }));
    assert.equal(view.queryByText("Sign in"), null);
    assert.ok(view.getByText("Paste a token"));
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); }
  }
});
