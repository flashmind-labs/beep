import test from "node:test";
import assert from "node:assert/strict";
import { splice, readTermMessage, describeMessage, lazyScreen, type Screen } from "../src/stores/terminals/terminal-state.ts";

test("output already on the screen is skipped, and a gap is noted", () => {
  // Fresh: everything from the start.
  assert.deepEqual(splice(0, 0, 10), { skip: 0, gap: false });
  // A reattach resent bytes 5..15 while the screen had 10.
  assert.deepEqual(splice(10, 5, 10), { skip: 5, gap: false });
  // All of it was there already.
  assert.deepEqual(splice(20, 5, 10), { skip: 10, gap: false });
  // The node no longer kept bytes 10..30.
  assert.deepEqual(splice(10, 30, 4), { skip: 0, gap: true });
});

test("channel messages read as events: output as raw bytes after its offset", () => {
  const raw = new Uint8Array([...new Uint8Array(new BigUint64Array([300n]).buffer), 0x68, 0x69]);
  // An ArrayBuffer, a typed array, or numbers when Tauri's IPC falls back to postMessage.
  for (const message of [raw.buffer, raw, [...raw]]) {
    const ev = readTermMessage(message);
    assert.ok(ev !== null && ev.kind === "output");
    assert.equal(ev.at, 300);
    assert.equal(new TextDecoder().decode(ev.data), "hi");
  }
  assert.deepEqual(readTermMessage({ kind: "ended", state: "exited", code: 0 }), { kind: "ended", state: "exited", code: 0 });
  assert.deepEqual(readTermMessage({ kind: "view_only" }), { kind: "view_only" });
});

test("messages this page does not know are not read as anything", () => {
  assert.equal(readTermMessage({ kind: "output", at: 0, data: "aGk=" }), null);
  assert.equal(readTermMessage({ something: 1 }), null);
  assert.equal(readTermMessage(new Uint8Array(3)), null);
  assert.equal(readTermMessage("text"), null);
  assert.equal(describeMessage({ kind: "x", data: 1 }), "object with keys kind, data");
  assert.equal(describeMessage([1, 2]), "array of 2");
});

test("a screen asked to focus while it loads does not take the keyboard from where it went", async t => {
  const doc = { activeElement: "row" as unknown, createElement: () => ({ className: "" }) };
  Object.assign(globalThis, { document: doc });
  t.after(() => { Reflect.deleteProperty(globalThis, "document"); });
  let focuses = 0;
  let load!: (make: (el: HTMLElement) => Screen) => void;
  const screen = lazyScreen(() => new Promise(resolve => { load = resolve; }));
  const loaded = { focus: () => { focuses++; } } as unknown as Screen;

  screen.focus();
  doc.activeElement = "other row";
  load(() => loaded);
  await Promise.resolve();
  assert.equal(focuses, 0);

  // Loaded, it focuses when asked.
  screen.focus();
  assert.equal(focuses, 1);
});
