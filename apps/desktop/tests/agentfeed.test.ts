import test from "node:test";
import assert from "node:assert/strict";
import { AgentFeed, BACKGROUND_FOLLOWS, IDLE_FOLLOW_MS } from "../src/stores/agents/agentfeed-state.ts";

function setup() {
  let now = 0;
  const readers: { key: string; id: string; receive(update: string): void; end(): void; fail(error: unknown): void }[] = [];
  const received: string[] = [], closed: string[] = [], unfollowed: string[] = [], lost: unknown[] = [], failed: string[] = [], shown: (string | null)[] = [];
  const timers = new Set<{ at: number; run(): void }>();
  const feed = new AgentFeed<string, string>({
    id: () => String(readers.length), now: () => now, name: key => key,
    later: (ms, run) => { const timer = { at: now + ms, run }; timers.add(timer); return () => { timers.delete(timer); }; },
    show: id => { shown.push(id); },
    open: (key, id, receive) => new Promise<void>((end, fail) => { readers.push({ key, id, receive, end, fail }); }),
    close: id => { closed.push(id); }, unfollowed: key => { unfollowed.push(key); },
    receive: (key, update) => { received.push(`${key}: ${update}`); },
    disconnected: (_key, error) => { lost.push(error); },
    failed: key => { failed.push(key); },
  });
  const advance = (ms: number) => {
    now += ms;
    for (const timer of [...timers]) if (timer.at <= now) { timers.delete(timer); timer.run(); }
  };
  return { feed, readers, received, closed, unfollowed, lost, failed, shown, timers, advance };
}

test("a quiet view keeps one live reader past the former five-minute cutoff", () => {
  const { feed, readers, received, advance } = setup();
  feed.follow("first");
  readers[0].receive("initial");
  advance(10 * 60_000);
  feed.follow("first");
  assert.equal(readers.length, 1);
  assert.equal(feed.live("first"), true);
  readers[0].receive("new turn");
  assert.deepEqual(received, ["first: initial", "first: new turn"]);
});

test("a running session left for another keeps streaming into its model with no new reader", () => {
  const { feed, readers, received, closed, shown } = setup();
  feed.follow("first");
  readers[0].receive("initial");
  feed.activity("first", "running");
  feed.follow("second");
  readers[0].receive("off screen");
  assert.deepEqual(closed, []);
  assert.deepEqual(feed.followed(), ["first", "second"]);
  feed.follow("first");
  assert.equal(readers.length, 2);
  assert.equal(feed.live("first"), true);
  assert.deepEqual(received, ["first: initial", "first: off screen"]);
  assert.deepEqual(shown, ["0", "1", "0"]);
});

test("an idle session off screen is unfollowed five minutes after it went idle", () => {
  const { feed, readers, closed, timers, advance } = setup();
  feed.follow("first");
  feed.activity("first", "running");
  feed.follow("second");
  advance(60_000);
  feed.activity("first", "idle");
  assert.equal(timers.size, 1);
  advance(IDLE_FOLLOW_MS - 1);
  assert.deepEqual(closed, []);
  feed.activity("first", "running");
  assert.equal(timers.size, 0);
  feed.activity("first", "idle");
  advance(IDLE_FOLLOW_MS);
  assert.deepEqual(closed, ["0"]);
  assert.deepEqual(feed.followed(), ["second"]);
  readers[0].receive("late");
  assert.equal(feed.live("first"), false);
});

test("leaving a session idle long enough unfollows it at once", () => {
  const { feed, closed, advance } = setup();
  feed.follow("first");
  advance(IDLE_FOLLOW_MS);
  feed.leave();
  assert.deepEqual(closed, ["0"]);
  assert.deepEqual(feed.followed(), []);
});

test("past the cap the least recently viewed goes, and one that asks goes last", () => {
  const { feed, closed } = setup();
  const names = Array.from({ length: BACKGROUND_FOLLOWS + 2 }, (_, i) => `s${i}`);

  for (const name of names) {
    feed.follow(name);
    feed.activity(name, name === "s0" ? "asking" : "running");
  }

  assert.deepEqual(closed, ["1"]);
  feed.leave();
  assert.deepEqual(closed, ["1", "2"]);
  assert.ok(feed.followed().includes("s0"));
  assert.equal(feed.followed().length, BACKGROUND_FOLLOWS);
});

test("a background reader that drops is forgotten, and the shown one reconnects after backoff", async () => {
  const { feed, readers, advance, lost } = setup();
  feed.follow("first");
  feed.activity("first", "running");
  feed.follow("second");
  readers[0].end();
  await Promise.resolve();
  assert.deepEqual(feed.followed(), ["second"]);
  assert.deepEqual(lost, []);
  readers[1].receive("initial");
  const version = feed.version("second");
  readers[1].end();
  await Promise.resolve();
  assert.equal(feed.live("second"), false);
  assert.equal(lost.length, 1);
  feed.follow("second");
  assert.equal(readers.length, 2);
  advance(5000);
  feed.follow("second");
  assert.equal(readers.length, 3);
  assert.ok(feed.version("second") > version);
  readers[2].receive("fresh snapshot");
  assert.equal(feed.live("second"), true);
});

test("resync reopens every followed session once and keeps the one shown", () => {
  const { feed, readers, closed, shown } = setup();
  feed.follow("first");
  feed.activity("first", "running");
  feed.follow("second");
  feed.resync();
  assert.deepEqual(closed, ["0", "1"]);
  assert.deepEqual(readers.slice(2).map(r => r.key), ["first", "second"]);
  assert.equal(shown.at(-1), "3");
  readers[0].receive("stale");
  readers[2].receive("resumed");
  assert.equal(feed.live("first"), true);
});

test("a follow that failed to open is tried again every five seconds", async () => {
  const { feed, readers, advance, lost } = setup();
  feed.follow("first");
  readers[0].fail("unknown protocol");
  await Promise.resolve();
  assert.equal(feed.live("first"), false);
  assert.deepEqual(lost, []);
  advance(4999);
  feed.follow("first");
  assert.equal(readers.length, 1);
  advance(1);
  feed.follow("first");
  assert.equal(readers.length, 2);
});

test("an opening follow is waited on until it fails, then the shown session reads another way", async () => {
  const { feed, readers, advance, failed } = setup();
  feed.follow("first");
  assert.equal(feed.waiting("first"), true);
  advance(10_000);
  assert.equal(feed.waiting("first"), true);
  feed.follow("second");
  assert.equal(feed.waiting("second"), true);
  readers[1].fail("unsupported");
  await Promise.resolve();
  assert.equal(feed.waiting("second"), false);
  assert.deepEqual(failed, ["second"]);
});

test("a reader ending for idleness, the cap or a gap says its session is unfollowed", () => {
  const { feed, unfollowed, advance } = setup();
  feed.follow("idle");
  feed.follow("shown");
  advance(IDLE_FOLLOW_MS);
  assert.deepEqual(unfollowed, ["idle"]);

  for (let i = 0; i <= BACKGROUND_FOLLOWS; i++) {
    feed.follow(`running ${i}`);
    feed.activity(`running ${i}`, "running");
  }

  // Left idle past the limit, then dropped by the cap.
  feed.leave();
  assert.deepEqual(unfollowed, ["idle", "shown", "running 0"]);
  feed.drop("running 1");
  assert.deepEqual(unfollowed, ["idle", "shown", "running 0", "running 1"]);
});

test("a held session stays followed beside the shown one and never takes the mark", () => {
  const { feed, readers, closed, shown, advance } = setup();
  feed.follow("main");
  const release = feed.hold("side", () => {});
  readers[1].receive("pushed");
  feed.activity("side", "running");
  feed.activity("side", "idle");
  advance(IDLE_FOLLOW_MS * 2);
  assert.deepEqual(closed, []);
  assert.deepEqual(shown, ["0"]);
  assert.equal(feed.live("side"), true);
  release();
  advance(IDLE_FOLLOW_MS);
  assert.deepEqual(closed, ["1"]);
});

test("a held session opens again after its link drops, and reports one that never opened", async () => {
  const { feed, readers, failed, advance } = setup();
  const reports: string[] = [];
  feed.hold("side", () => { reports.push("failed"); });
  readers[0].receive("first");
  readers[0].fail("dropped");
  await Promise.resolve();
  assert.equal(readers.length, 1);
  assert.deepEqual(reports, []);
  advance(5000);
  assert.equal(readers.length, 2);
  readers[1].end();
  await Promise.resolve();
  assert.deepEqual(reports, ["failed"]);
  assert.deepEqual(failed, []);
});

test("a warmed asking session stays followed among eight hidden ones that were viewed", () => {
  const { feed, readers, closed } = setup();

  for (let i = 0; i < BACKGROUND_FOLLOWS; i++) {
    feed.follow(`seen-${i}`);
    readers[i].receive("page");
    feed.activity(`seen-${i}`, "running");
  }

  feed.leave();
  feed.warm("asked", "asking");
  assert.equal(feed.followed().includes("asked"), true);
  assert.equal(closed.includes(String(BACKGROUND_FOLLOWS)), false);
});
