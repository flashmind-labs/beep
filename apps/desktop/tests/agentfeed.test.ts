import test from "node:test";
import assert from "node:assert/strict";
import { AgentFeed } from "../src/agents/agentfeed-state.ts";

function setup() {
  let now = 0;
  const readers: { key: string; receive(update: string): void; end(): void; fail(error: unknown): void }[] = [];
  const received: string[] = [], closed: string[] = [], lost: unknown[] = [];
  const feed = new AgentFeed<string, string>({
    id: () => String(readers.length), now: () => now,
    open: (key, _id, receive) => new Promise<void>((end, fail) => { readers.push({ key, receive, end, fail }); }),
    close: id => { closed.push(id); },
    receive: (key, update) => { received.push(`${key}: ${update}`); },
    disconnected: (_key, error) => { lost.push(error); },
  });
  return { feed, readers, received, closed, lost, advance(ms: number) { now += ms; } };
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

test("switching sessions detaches the old reader and ignores its late messages and failure", async () => {
  const { feed, readers, received, closed, lost } = setup();
  feed.follow("first");
  readers[0].receive("initial");
  const oldVersion = feed.version("first");
  feed.follow("second");
  readers[0].receive("late");
  readers[0].fail("old failure");
  readers[1].receive("current");
  await Promise.resolve();
  assert.deepEqual(closed, ["0"]);
  assert.deepEqual(lost, []);
  assert.deepEqual(received, ["first: initial", "second: current"]);
  assert.ok(feed.version("second") > oldVersion);
  feed.stop();
  readers[1].receive("detached");
  assert.equal(feed.live("second"), false);
  assert.deepEqual(closed, ["0", "1"]);
  assert.equal(received.length, 2);
});

test("a dropped live reader reconnects after backoff and advances the read version", async () => {
  const { feed, readers, advance, lost } = setup();
  feed.follow("first");
  readers[0].receive("initial");
  const version = feed.version("first");
  readers[0].end();
  await Promise.resolve();
  assert.equal(feed.live("first"), false);
  assert.equal(lost.length, 1);
  feed.follow("first");
  assert.equal(readers.length, 1);
  advance(5000);
  feed.follow("first");
  assert.equal(readers.length, 2);
  assert.ok(feed.version("first") > version);
  readers[1].receive("fresh snapshot");
  assert.equal(feed.live("first"), true);
});

test("nodes without streaming keep the fallback available without a tight retry loop", async () => {
  const { feed, readers, advance, lost } = setup();
  feed.follow("first");
  readers[0].fail("unknown protocol");
  await Promise.resolve();
  assert.equal(feed.live("first"), false);
  assert.deepEqual(lost, []);
  advance(10_000);
  feed.follow("first");
  assert.equal(readers.length, 1);
  advance(50_000);
  feed.follow("first");
  assert.equal(readers.length, 2);
});

test("the opening snapshot gets a bounded wait before fallback reads are needed", async () => {
  const { feed, readers, advance } = setup();
  feed.follow("first");
  assert.equal(feed.waiting("first"), true);
  advance(3000);
  assert.equal(feed.waiting("first"), false);
  feed.follow("second");
  assert.equal(feed.waiting("second"), true);
  readers[1].fail("unsupported");
  await Promise.resolve();
  assert.equal(feed.waiting("second"), false);
});
