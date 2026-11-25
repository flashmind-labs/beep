import test from "node:test";
import assert from "node:assert/strict";
import { serial } from "../src/ui/active.ts";

test("a burst of calls leaves the run under way alone and ends with one that began after the last", async () => {
  const finish: (() => void)[] = [];
  let started = 0;
  const run = serial(() => { started++; return new Promise<void>(resolve => { finish.push(resolve); }); });

  const first = run();
  const second = run();
  const third = run();
  assert.equal(started, 1, "the second and third wait for the first");
  assert.equal(second, third, "and share one run");

  finish[0]();
  await first;
  await Promise.resolve();
  assert.equal(started, 2);
  finish[1]();
  await third;
  assert.equal(started, 2);

  // Once idle, the next call runs at once.
  void run();
  assert.equal(started, 3);
});

test("a run that fails does not stop the ones waiting for it", async () => {
  let started = 0;
  const run = serial(async () => { if (++started === 1) throw new Error("no"); });

  const first = run();
  const second = run();
  await assert.rejects(first);
  await second;
  assert.equal(started, 2);
});
