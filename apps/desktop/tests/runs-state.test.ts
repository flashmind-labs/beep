import test from "node:test";
import assert from "node:assert/strict";
import {
  Bounded, DetailCache, POLL_MS, ahead, detailDue, detailKey, type ListedRun,
} from "../src/stores/project/runs-state.ts";

const NOW = 1_800_000_000_000;
const run = (id: number, state: string, o: Partial<ListedRun> = {}): ListedRun => ({ id, host: "github.com", state, finished_at: state === "running" ? null : NOW - 1000, ...o });

test("details are kept per project and run, and the oldest go first", () => {
  const cache = new DetailCache<string>(2);
  cache.put(detailKey("p1", run(1, "passed")), "a", NOW);
  cache.put(detailKey("p2", run(1, "passed")), "b", NOW);
  assert.notEqual(detailKey("p1", run(1, "passed")), detailKey("p2", run(1, "passed")));
  assert.notEqual(detailKey("p1", run(1, "passed")), detailKey("p1", run(1, "passed", { host: "forge.example.com" })));
  // Reading one makes it the newest, so the other goes.
  cache.get(detailKey("p1", run(1, "passed")));
  cache.put(detailKey("p1", run(2, "passed")), "c", NOW);
  assert.equal(cache.size, 2);
  assert.ok(cache.has(detailKey("p1", run(1, "passed"))));
  assert.ok(!cache.has(detailKey("p2", run(1, "passed"))));
});

test("a finished run kept is never read again; a running one is once a poll old", () => {
  const done = { data: { run: run(1, "failed") }, at: NOW - 3_600_000 };
  assert.equal(detailDue(done, run(1, "failed"), NOW), null);
  assert.deepEqual(detailDue(undefined, run(1, "failed"), NOW), { refresh: false });
  const goes = { data: { run: run(2, "running") }, at: NOW - POLL_MS + 1 };
  assert.equal(detailDue(goes, run(2, "running"), NOW), null);
  assert.deepEqual(detailDue(goes, run(2, "running"), NOW + 1), { refresh: false });
  // The rate limit holds: no poll.
  assert.equal(detailDue(goes, run(2, "running"), NOW + 1, NOW + 60_000), null);
});

test("a run the list shows changed is read past the app's copy", () => {
  const kept = { data: { run: run(2, "running") }, at: NOW };
  assert.deepEqual(detailDue(kept, run(2, "passed"), NOW), { refresh: true });
  // A re-run: same state, a new finish.
  const done = { data: { run: run(3, "failed") }, at: NOW };
  assert.deepEqual(detailDue(done, run(3, "failed", { finished_at: NOW + 5 }), NOW, NOW + 60_000), { refresh: true });
});

test("the newest runs and the newest failed one are read ahead, unless kept", () => {
  const runs = [run(9, "running"), run(8, "passed"), run(7, "passed"), run(6, "passed"), run(5, "failed"), run(4, "failed")];
  assert.deepEqual(ahead(runs, () => false).map(r => r.id), [9, 8, 7, 5]);
  assert.deepEqual(ahead(runs, r => r.id === 8 || r.id === 5).map(r => r.id), [9, 7]);
  assert.deepEqual(ahead([run(2, "failed"), run(1, "passed")], () => false).map(r => r.id), [2, 1]);
});

test("reads ahead run a few at a time and once per key", async () => {
  const q = new Bounded(2);
  const started: string[] = [];
  const ends = new Map<string, () => void>();
  const read = (key: string) => () => new Promise<void>(done => { started.push(key); ends.set(key, done); });
  q.add("a", read("a"));
  q.add("b", read("b"));
  q.add("c", read("c"));
  q.add("a", read("a"));
  assert.deepEqual(started, ["a", "b"]);
  assert.ok(q.has("c"));
  ends.get("a")!();
  // Every promise callback runs before the next turn of the event loop.
  await new Promise(r => setImmediate(r));
  assert.deepEqual(started, ["a", "b", "c"]);
  assert.equal(q.busy, 2);
});
