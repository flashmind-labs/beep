import test, { mock } from "node:test";
import assert from "node:assert/strict";
import { clockWarning, loadClocks, takeClocks } from "../src/machines/clock.ts";

type Invoke = Parameters<typeof loadClocks>[0];

mock.method(console, "warn", () => {});

let calls = 0;
let fail = true;
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

test("reads at the same time share one request, and a failure leaves the clocks unknown", async () => {
  const reads = [loadClocks(invoke), loadClocks(invoke), loadClocks(invoke)];
  await Promise.resolve();
  answer?.();
  answer = null;
  await Promise.all(reads);
  assert.equal(calls, 1);
  assert.equal(clockWarning("m1", null), null);
});

test("the first read answers until a push does, and nothing reads again after", async () => {
  fail = false;
  await read();
  assert.equal(calls, 2);
  assert.equal(clockWarning("m1", null)?.text, "Clock is 2 min ahead");

  await read();
  assert.equal(calls, 2);
});

test("a push replaces what is known", () => {
  takeClocks([{ id: "m2", offset_ms: -120_000 }]);
  assert.equal(clockWarning("m1", null), null);
  assert.equal(clockWarning("m2", null)?.text, "Clock is 2 min behind");
});
