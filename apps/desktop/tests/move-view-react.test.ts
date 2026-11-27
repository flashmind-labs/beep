import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { MoveStatus, type MoveView } from "../src/parts/session/move-view.tsx";

test("move progress preserves its cancel button and focus across updates", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  const root = createRoot(host);
  let cancelled = 0, stopped = 0;
  const render = (view: MoveView) => root.render(createElement(MoveStatus, { view, cancel: () => cancelled++, stop: () => stopped++ }));
  try {
    await act(() => render({ kind: "progress", to: "Build box", cancelable: true, rows: [{ state: "run", text: "Copying" }], pct: null }));
    const cancel = host.querySelector<HTMLButtonElement>('[data-mvact="cancel"]');
    assert.ok(cancel); cancel.focus();
    await act(() => render({ kind: "progress", to: "Build box", cancelable: true, rows: [{ state: "done", text: "Copied" }, { state: "run", text: "Resuming" }], pct: null }));
    assert.equal(host.querySelector('[data-mvact="cancel"]'), cancel);
    assert.equal(dom.window.document.activeElement, cancel);
    await act(() => cancel.click());
    assert.equal(cancelled, 1);
    await act(() => render({ kind: "waiting", to: "Build box" }));
    const stop = host.querySelector<HTMLButtonElement>('[data-mvact="stop"]');
    assert.ok(stop); await act(() => stop.click());
    assert.equal(stopped, 1);
    await act(() => render({ kind: "progress", to: "Build box", cancelable: false, rows: [], pct: null }));
    assert.equal(host.querySelector('[data-mvact="cancel"]'), null);
  } finally {
    await act(() => root.unmount()); dom.window.close();
    for (const [key, descriptor] of previous) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); }
  }
});
