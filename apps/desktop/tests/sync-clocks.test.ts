import test, { mock } from "node:test";
import assert from "node:assert/strict";
import { CLOCK_MAX_AGE_MS, clockWarning, loadClocks } from "../src/machines/clock.ts";
import { RETRY_MS } from "../src/agents/reconnect-state.ts";

type Invoke = Parameters<typeof loadClocks>[0];

mock.timers.enable({ apis: ["Date"], now: 1_000_000 });
mock.method(console, "warn", () => {});

let calls = 0;
let fail = false;
let answer: (() => void) | null = null;
const invoke = (async () => {
  calls += 1;
  await new Promise<void>(done => { answer = done; });
  if (fail) throw new Error("offline");
  return [{ id: "m1", offset_ms: 120_000 }];
}) as Invoke;

/// Starts a read and answers whatever request it made.
async function read() {
  const run = loadClocks(invoke);
  await Promise.resolve();
  answer?.();
  answer = null;
  await run;
}

test("reads at the same time share one request", async () => {
  const reads = [loadClocks(invoke), loadClocks(invoke), loadClocks(invoke)];
  await Promise.resolve();
  answer?.();
  answer = null;
  await Promise.all(reads);
  assert.equal(calls, 1);
  assert.equal(clockWarning("m1", null)?.text, "Clock is 2 min ahead");
});

test("a read stays good for 30 seconds", async () => {
  await read();
  assert.equal(calls, 1);
  mock.timers.tick(CLOCK_MAX_AGE_MS + 1);
  await read();
  assert.equal(calls, 2);
});

test("a failure keeps the known warnings and reads again every RETRY_MS", async () => {
  mock.timers.tick(CLOCK_MAX_AGE_MS + 1);
  fail = true;
  await read();
  assert.equal(calls, 3);
  assert.equal(clockWarning("m1", null)?.text, "Clock is 2 min ahead");

  mock.timers.tick(RETRY_MS - 1);
  await read();
  assert.equal(calls, 3);
  mock.timers.tick(2);
  await read();
  assert.equal(calls, 4);

  mock.timers.tick(RETRY_MS + 1);
  fail = false;
  await read();
  assert.equal(calls, 5);

  mock.timers.tick(RETRY_MS);
  await read();
  assert.equal(calls, 5);
});
