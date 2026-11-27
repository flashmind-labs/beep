import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { ComposerChoices, type ComposerChoicesState } from "../src/parts/project/composer-choices.tsx";

test("branch edits survive updates while focused and reconcile after blur", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { createRoot } = await import("react-dom/client");
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  const root = createRoot(host);
  const value: ComposerChoicesState = { machine: "machine", on: false, adding: false, machineDisabled: false, folder: "folder", folderSub: "", folderDisabled: false, branch: "first", showBranch: true, showClear: true, account: null };
  const actions = { machine: () => {}, folder: () => {}, account: () => {}, branch: () => {}, clear: () => {} };
  const render = () => root.render(createElement(ComposerChoices, { value: { ...value }, actions }));
  try {
    await act(render);
    const input = host.querySelector("input");
    assert.ok(input);
    assert.equal(input.value, "first");
    input.focus();
    input.value = "custom";
    value.branch = "second";
    await act(render);
    assert.equal(input.value, "custom");
    input.blur();
    await act(render);
    assert.equal(input.value, "second");
    assert.equal(host.querySelector("input"), input);
  } finally {
    await act(() => root.unmount());
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
