import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { lazyRows, resetRows } from "../src/ui/lazyrows.ts";

test("overview spacer measures the connected group and loads later groups", () => {
  const dom = new JSDOM("<!doctype html><div id='view'></div>");
  const old = new Map<string, PropertyDescriptor | undefined>();
  let notify: (entries: { target: Element; isIntersecting: boolean }[]) => void = () => {};
  const frames: (() => void)[] = [];
  class Observer {
    constructor(callback: (entries: { target: Element; isIntersecting: boolean }[]) => void) { notify = callback; }
    observe() {}
    unobserve() {}
  }
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, IntersectionObserver: Observer, getComputedStyle: () => ({ overflowY: "visible" }), requestAnimationFrame: (cb: () => void) => { frames.push(cb); return 1; } })) {
    old.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  try {
    const view = dom.window.document.getElementById("view");
    assert.ok(view);
    const slots = new WeakMap<Element, Element>();
    let first = true;
    const nodes = lazyRows("overview-test", "", 10, (from, to) => Array.from({ length: to - from }, (_, i) => {
      const host = dom.window.document.createElement("section");
      host.textContent = `Group ${from + i}`;
      host.getBoundingClientRect = () => ({ top: 500 } as DOMRect);
      if (!first) return host;
      const slot = dom.window.document.createElement("div");
      slots.set(slot, host);
      return slot;
    }), 0, 1, row => slots.get(row) ?? row);
    first = false;
    const slot = nodes[0];
    assert.ok(slot instanceof dom.window.Element);
    const host = slots.get(slot);
    assert.ok(host);
    const spacer = nodes[1];
    assert.ok(spacer instanceof dom.window.HTMLElement);
    spacer.getBoundingClientRect = () => ({ top: 600 } as DOMRect);
    view.append(host, spacer);
    for (const frame of frames) frame();
    notify([{ target: spacer, isIntersecting: false }]);
    assert.equal(spacer.style.height, "900px");
    notify([{ target: spacer, isIntersecting: true }]);
    assert.equal(spacer.style.height, "800px");
    assert.deepEqual([...view.querySelectorAll("section")].map(row => row.textContent), ["Group 0", "Group 1"]);
  } finally {
    resetRows("overview-test");
    dom.window.close();
    for (const [key, descriptor] of old) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
