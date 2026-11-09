import test from "node:test";
import assert from "node:assert/strict";
import { frameResize } from "../src/ui/dom.ts";

test("resize layout writes are deferred and batched outside observer delivery", () => {
  const frames: FrameRequestCallback[] = [];
  let notify = () => {};
  const oldObserver = globalThis.ResizeObserver;
  const oldFrame = globalThis.requestAnimationFrame;
  const oldCancel = globalThis.cancelAnimationFrame;
  let cancelled: number | null = null;
  globalThis.ResizeObserver = class {
    constructor(callback: ResizeObserverCallback) { notify = () => callback([], this); }
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  globalThis.requestAnimationFrame = callback => frames.push(callback);
  globalThis.cancelAnimationFrame = frame => { cancelled = frame; };

  try {
    let writes = 0;
    const observer = frameResize(() => { writes++; });
    notify();
    notify();
    assert.equal(writes, 0);
    assert.equal(frames.length, 1);
    frames.shift()?.(0);
    assert.equal(writes, 1);
    notify();
    assert.equal(frames.length, 1);
    frames.shift()?.(0);
    assert.equal(writes, 2);
    notify();
    observer.disconnect();
    assert.notEqual(cancelled, null);
  } finally {
    globalThis.ResizeObserver = oldObserver;
    globalThis.requestAnimationFrame = oldFrame;
    globalThis.cancelAnimationFrame = oldCancel;
  }
});
