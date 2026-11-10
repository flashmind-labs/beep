import test from "node:test";
import assert from "node:assert/strict";
import { TranscriptCache, transcriptKey, type TranscriptUpdate } from "../src/agents/transcript-cache-state.ts";
import { AgentFeed } from "../src/agents/agentfeed-state.ts";
import type { Run } from "../src/agents/older-state.ts";

function update(kind: "snapshot" | "delta", earlier: number, texts: string[], start = 0, resumed = false): TranscriptUpdate<Run> {
  return {
    kind, page: { earlier, entries: texts.map(text => ({ kind: "agent", text })) },
    cursor: { start, next: earlier + texts.length, fingerprint: texts.join(":") }, resumed,
  };
}

test("reconnect applies a snapshot after N and keeps the validated cached prefix", () => {
  const cache = new TranscriptCache<Run>();
  cache.receive("one", update("snapshot", 0, ["first", "second"]));
  const cursor = cache.get("one")!.cursor;
  assert.equal(cursor.next, 2);
  assert.deepEqual(cache.receive("one", update("snapshot", 2, ["third"], 0, true))?.entries.map(e => e.text), ["first", "second", "third"]);
  assert.deepEqual(cache.receive("one", update("snapshot", 3, [], 0, true))?.entries.map(e => e.text), ["first", "second", "third"]);
});

test("same-count delta replacements and empty truncations keep canonical positions", () => {
  const cache = new TranscriptCache<Run>();
  cache.receive("one", update("snapshot", 3, ["first", "thi"], 3));
  assert.deepEqual(cache.receive("one", update("delta", 4, ["thinking"], 3))?.entries.map(e => e.text), ["first", "thinking"]);
  const truncated = cache.receive("one", update("delta", 4, [], 3));
  assert.equal(truncated?.earlier, 3);
  assert.deepEqual(truncated?.entries.map(e => e.text), ["first"]);
});

test("reset snapshots discard stale prefixes and missing resumed prefixes require a fresh read", () => {
  const cache = new TranscriptCache<Run>();
  cache.receive("one", update("snapshot", 0, ["old", "history"]));
  assert.deepEqual(cache.receive("one", update("snapshot", 1, ["new"], 1))?.entries.map(e => e.text), ["new"]);
  assert.equal(cache.receive("missing", update("snapshot", 2, ["suffix"], 0, true)), null);
  assert.equal(cache.receive("one", update("delta", 9, ["gap"], 1)), null);
  assert.equal(cache.get("one"), undefined);
});

test("moving retained windows drop only the prefix outside the new cursor", () => {
  const cache = new TranscriptCache<Run>();
  cache.receive("one", update("snapshot", 0, ["one", "two", "three"]));
  const page = cache.receive("one", update("delta", 3, ["four"], 1));
  assert.equal(page?.earlier, 1);
  assert.deepEqual(page?.entries.map(e => e.text), ["two", "three", "four"]);
});

test("cache eviction bounds inactive sessions and keys include machine and agent", () => {
  const cache = new TranscriptCache<Run>(2, 20);
  cache.receive("one", update("snapshot", 0, ["first"]));
  cache.receive("two", update("snapshot", 0, ["second"]));
  cache.get("one");
  cache.receive("three", update("snapshot", 0, ["third"]));
  assert.equal(cache.get("two"), undefined);
  cache.receive("large", update("snapshot", 0, ["a".repeat(30)]));
  assert.equal(cache.get("one"), undefined);
  assert.ok(cache.get("large"));
  const key = { machine_id: "machine-a", agent: "agent-a", session: "same" };
  assert.notEqual(transcriptKey(key), transcriptKey({ ...key, machine_id: "machine-b" }));
  assert.notEqual(transcriptKey(key), transcriptKey({ ...key, agent: "agent-b" }));
});

test("a dropped reader cannot alter a cached session once it is followed again", () => {
  const cache = new TranscriptCache<Run>();
  const readers: ((update: TranscriptUpdate<Run>) => void)[] = [];
  const feed = new AgentFeed<string, TranscriptUpdate<Run>>({
    id: () => String(readers.length), now: () => 0, name: key => key, later: () => () => {}, show: () => {},
    open: (_key, _id, receive) => { readers.push(receive); return new Promise(() => {}); },
    close: () => {}, unfollowed: () => {}, disconnected: () => {}, receive: (key, value) => { cache.receive(key, value); },
  });
  feed.follow("one");
  readers[0](update("snapshot", 0, ["first"]));
  feed.drop("one");
  readers[0](update("delta", 1, ["late"]));
  feed.follow("one");
  readers[1](update("snapshot", 1, ["current"], 0, true));
  assert.deepEqual(cache.get("one")?.page.entries.map(e => e.text), ["first", "current"]);
});


test("cache budget includes structured tool payloads", () => {
  const cache = new TranscriptCache<Run>(8, 90);
  const first = update("snapshot", 0, ["a"]);
  Object.assign(first.page.entries[0], { tool: { result: "x".repeat(80) } });
  cache.receive("one", first);
  cache.receive("two", update("snapshot", 0, ["b"]));
  assert.equal(cache.get("one"), undefined);
  assert.ok(cache.get("two"));
});
