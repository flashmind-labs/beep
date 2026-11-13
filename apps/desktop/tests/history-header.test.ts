import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

test("Copied goes back to the label after a moment", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  Object.defineProperty(dom.window.navigator, "clipboard", { value: { writeText: async () => {} }, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: dom.window.navigator, configurable: true });
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async () => null, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { act, fireEvent, screen, cleanup } = await import("@testing-library/react");
  const { createElement } = await import("react");
  const { HistoryHeader } = await import("../src/review/history-header.tsx");
  const { render } = await import("@testing-library/react");

  try {
    render(createElement(HistoryHeader, { breadcrumb: "b", title: "t", metadata: "m", copyValue: "abc", copyLabel: "Copy hash", tab: "changes", onTab() {} }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Copy hash" })); });
    assert.ok(screen.getByRole("button", { name: "Copied" }));
    await act(async () => { t.mock.timers.tick(2000); });
    assert.ok(screen.getByRole("button", { name: "Copy hash" }));
  } finally {
    cleanup();
  }
});
