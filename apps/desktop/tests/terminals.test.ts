import test from "node:test";
import assert from "node:assert/strict";
import { splice, readTermMessage } from "../src/terminals/terminals.ts";

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
  const ev = readTermMessage(raw.buffer);
  assert.equal(ev.kind, "output");
  if (ev.kind !== "output") return;
  assert.equal(ev.at, 300);
  assert.equal(new TextDecoder().decode(ev.data), "hi");
  assert.deepEqual(readTermMessage({ kind: "ended", state: "exited", code: 0 }), { kind: "ended", state: "exited", code: 0 });
});
