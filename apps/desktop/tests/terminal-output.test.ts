import test from "node:test";
import assert from "node:assert/strict";
import { outputBatch } from "../src/terminals/output-batch.ts";

test("groups shell echo and suggestion redraw without losing their order", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const writes: (string | Uint8Array)[] = [];
  const output = outputBatch(data => writes.push(data));
  const echo = new Uint8Array([103]);
  const suggestion = "\bgi\x1b[1D";
  output.write(echo);
  t.mock.timers.tick(5);
  output.write(suggestion);
  assert.deepEqual(writes, []);
  t.mock.timers.tick(7);
  assert.deepEqual(writes, [echo, suggestion]);
  output.dispose();
});

test("bounds a busy stream and cancels queued output on disposal", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const writes: (string | Uint8Array)[] = [];
  const output = outputBatch(data => writes.push(data));
  const large = new Uint8Array(64 * 1024);
  output.write(large);
  assert.deepEqual(writes, [large]);
  output.write("late");
  output.dispose();
  t.mock.timers.tick(20);
  assert.deepEqual(writes, [large]);
});
