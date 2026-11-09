import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { createStore } from "zustand/vanilla";
import { useStore } from "zustand";
import { TestInput } from "./react-foundation-fixture.tsx";

test("React preserves a mounted input across unrelated store updates and cleans up", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  const store = createStore<{ label: string; count: number }>(() => ({ label: "History", count: 0 }));
  let mounts = 0, cleanups = 0, renders = 0;
  function Input() {
    renders++;
    const label = useStore(store, state => state.label);
    useEffect(() => { mounts++; return () => { cleanups++; }; }, []);
    return createElement(TestInput, { label });
  }
  const root = createRoot(host);

  try {
    await act(() => { root.render(createElement(Input)); });
    const input = host.querySelector("input");
    assert.ok(input);
    input.value = "edited";
    await act(() => { store.setState({ count: 1 }); });
    assert.equal(host.querySelector("input"), input);
    assert.equal(input.value, "edited");
    assert.equal(renders, 1);
    await act(() => { store.setState({ label: "Commits" }); });
    assert.equal(host.querySelector("input"), input);
    assert.equal(input.getAttribute("aria-label"), "Commits");
    assert.equal(mounts, 1);
  } finally {
    await act(() => { root.unmount(); });
    assert.equal(cleanups, 1);
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
