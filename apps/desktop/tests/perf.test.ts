import { test } from "node:test";
import assert from "node:assert/strict";
import { PERF_FLUSH_MS, PERF_SLOW_FLUSH_MS, PerfAggregate, PerfRecorder, bucketOf, inputOf, perfViewOf, type PerfBatch, type PerfClock } from "../src/ui/perf-state.ts";

class FakeClock implements PerfClock {
  at = 0;
  timers = new Map<number, { due: number; run: () => void }>();
  private next = 1;

  now() { return this.at; }
  later(run: () => void, ms: number) { const id = this.next++; this.timers.set(id, { due: this.at + ms, run }); return id; }
  cancel(timer: number) { this.timers.delete(timer); }

  advance(ms: number) {
    const until = this.at + ms;

    for (;;) {
      const due = [...this.timers].filter(([, t]) => t.due <= until).sort((a, b) => a[1].due - b[1].due)[0];
      if (!due) break;
      this.timers.delete(due[0]);
      this.at = due[1].due;
      due[1].run();
    }

    this.at = until;
  }
}

function recorder(shown = true) {
  const clock = new FakeClock(), sent: PerfBatch[] = [];
  const r = new PerfRecorder("main", "home", shown, clock, batch => sent.push(batch));
  return { clock, sent, r };
}

test("buckets samples by fixed bounds and keeps only fixed labels", () => {
  assert.equal(bucketOf(0), 0);
  assert.equal(bucketOf(16), 0);
  assert.equal(bucketOf(17), 1);
  assert.equal(bucketOf(5000), 9);
  assert.equal(bucketOf(5001), 10);
  assert.equal(inputOf("pointerup"), "pointer");
  assert.equal(inputOf("click"), "pointer");
  assert.equal(inputOf("keydown"), "key");
  assert.equal(inputOf("input"), "other");
  assert.equal(perfViewOf("agent"), "session");
  assert.equal(perfViewOf("projects"), "project");
  assert.equal(perfViewOf("a private page"), "other");

  const a = new PerfAggregate();
  a.record("interaction", "session", 40, "key");
  a.record("interaction", "session", 600, "key");
  a.record("interaction", "session", 20, "pointer");
  a.record("long_task", "home", 250);
  a.record("open", "diff", Number.NaN);
  a.record("open", "diff", -1);
  const batch = a.take("main", ["event"]);
  assert.ok(batch);
  const key = batch.histograms.find(h => h.input === "key");
  assert.deepEqual(key?.counts, [0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
  assert.equal(key?.sum_ms, 640);
  assert.equal(key?.max_ms, 600);
  assert.equal(batch.histograms.length, 3);
  assert.deepEqual(batch.slow, [{ kind: "interaction", view: "session", ms: 600 }, { kind: "long_task", view: "home", ms: 250 }]);
  assert.equal(a.take("main", []), null);
});

test("flushes about every minute while shown and once on hide, never while hidden", () => {
  const { clock, sent, r } = recorder();
  r.record("interaction", "home", 30, "pointer");
  clock.advance(PERF_FLUSH_MS - 1);
  assert.equal(sent.length, 0);
  clock.advance(1);
  assert.equal(sent.length, 1);
  assert.deepEqual(sent[0].shown_ms, [{ view: "home", ms: PERF_FLUSH_MS }]);

  clock.advance(10_000);
  r.visibility(false);
  assert.equal(sent.length, 2);
  assert.deepEqual(sent[1].shown_ms, [{ view: "home", ms: 10_000 }]);
  assert.equal(clock.timers.size, 0);

  // Hidden: samples wait, no timer runs, and a page going with nothing new sends nothing.
  clock.advance(5 * PERF_FLUSH_MS);
  r.flush();
  assert.equal(sent.length, 2);
  r.record("long_task", "home", 400);
  assert.equal(clock.timers.size, 0);

  r.visibility(true);
  assert.equal(clock.timers.size, 1);
  r.flush();
  assert.equal(sent.length, 3);
  assert.equal(sent[2].hidden_ms, 5 * PERF_FLUSH_MS);
  assert.equal(sent[2].histograms[0].kind, "long_task");
});

test("an idle shown window sends nothing on the timer, and its shown time goes with the next batch", () => {
  const { clock, sent, r } = recorder();
  clock.advance(3 * PERF_FLUSH_MS);
  assert.equal(sent.length, 0);
  assert.equal(clock.timers.size, 1);

  r.record("open", "home", 40);
  clock.advance(PERF_FLUSH_MS);
  assert.equal(sent.length, 1);
  assert.deepEqual(sent[0].shown_ms, [{ view: "home", ms: 4 * PERF_FLUSH_MS }]);

  clock.advance(PERF_FLUSH_MS / 2);
  r.visibility(false);
  assert.equal(sent.length, 2);
  assert.deepEqual(sent[1].histograms, []);
  assert.deepEqual(sent[1].shown_ms, [{ view: "home", ms: PERF_FLUSH_MS / 2 }]);
});

test("charges time and samples to the view or overlay showing", () => {
  const { clock, sent, r } = recorder();
  clock.advance(1000);
  r.setView("session");
  clock.advance(2000);
  r.layer("picker", true);
  clock.advance(500);
  r.layer("picker", false);
  clock.advance(100);
  assert.equal(r.viewAt(1500), "session");
  assert.equal(r.viewAt(500), "home");
  assert.equal(r.view, "session");
  r.flush();
  assert.deepEqual(Object.fromEntries(sent[0].shown_ms.map(s => [s.view, s.ms])), { home: 1000, session: 2100, picker: 500 });
});

test("sends a slow sample soon, at most once a minute", () => {
  const { clock, sent, r } = recorder();
  r.record("interaction", "session", 800, "key");
  clock.advance(PERF_SLOW_FLUSH_MS);
  assert.equal(sent.length, 1);
  r.record("interaction", "session", 900, "key");
  clock.advance(PERF_SLOW_FLUSH_MS);
  assert.equal(sent.length, 1);
  clock.advance(PERF_FLUSH_MS);
  assert.equal(sent.length, 2);
  assert.equal(sent[1].slow[0].ms, 900);
});
