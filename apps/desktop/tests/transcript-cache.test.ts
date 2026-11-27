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
    close: () => {}, unfollowed: () => {}, disconnected: () => {}, failed: () => {}, receive: (key, value) => { cache.receive(key, value); },
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

test("the budget follows deltas that replace, append and drop entries", () => {
  const cache = new TranscriptCache<Run>(8, 25);
  cache.receive("one", update("snapshot", 0, ["a".repeat(10), "b".repeat(10)]));
  // The last entry is replaced by a shorter one: 15 characters, under the budget with another session.
  cache.receive("one", update("delta", 1, ["c".repeat(5)]));
  cache.receive("two", update("snapshot", 0, ["d".repeat(10)]));
  assert.ok(cache.get("one"));
  // The window moves past its first entry: 5 + 10 for "one".
  cache.receive("one", update("delta", 2, ["e".repeat(10)], 1));
  assert.ok(cache.get("two"));
  // Growing "one" past what both may hold lets the other go.
  cache.receive("one", update("delta", 3, ["f".repeat(10)], 1));
  assert.equal(cache.get("two"), undefined);
  assert.deepEqual(cache.get("one")?.page.entries.map(e => e.text[0]), ["c", "e", "f"]);
});

test("modes and tasks come with an update, survive a gap, and go with the session", () => {
  const cache = new TranscriptCache<Run>();
  const task = { id: "t", name: "Build", description: "", state: "running", can_stop: true, output_file: null, summary: null, entry: null };
  cache.receive("one", { ...update("snapshot", 0, ["a"]), modes: null, tasks: [task] });
  assert.deepEqual(cache.tasks("one"), [task]);
  assert.equal(cache.receive("one", update("delta", 9, ["gap"], 1)), null);
  assert.deepEqual(cache.tasks("one"), [task]);
  cache.setTasks("one", []);
  assert.deepEqual(cache.tasks("one"), []);
  cache.setTasks("one", [task]);
  cache.delete("one");
  assert.deepEqual(cache.tasks("one"), []);
});

test("amending a held page replaces it and tells the listeners", () => {
  const cache = new TranscriptCache<Run & { note: string }>();
  cache.receive("one", { ...update("snapshot", 0, ["first"]), page: { earlier: 0, entries: [{ kind: "agent", text: "first" }], note: "a" } });
  cache.receive("two", { ...update("snapshot", 0, ["other"]), page: { earlier: 0, entries: [{ kind: "agent", text: "other" }], note: "b" } });
  const before = cache.peek("one");
  let heard = 0;
  cache.subscribe(() => { heard++; });

  cache.amend("one", page => ({ ...page, note: "c" }));
  assert.equal(heard, 1);
  assert.notEqual(cache.peek("one"), before, "a new entry, so a store snapshot sees the change");
  assert.equal(cache.peek("one")?.page.note, "c");
  assert.equal(cache.peek("one")?.cursor, before?.cursor);

  cache.amend("missing", page => page);
  assert.equal(heard, 1);
});

test("the pinned page is never evicted, and a seed obeys the limit", () => {
  const cache = new TranscriptCache<Run>(2);
  cache.receive("open", update("snapshot", 0, ["a"]));
  cache.pin("open");
  cache.receive("one", update("snapshot", 0, ["b"]));
  cache.receive("two", update("snapshot", 0, ["c"]));
  assert.ok(cache.peek("open"));
  assert.equal(cache.peek("one"), undefined);
  assert.ok(cache.peek("two"));

  const kept = (text: string) => ({ page: { earlier: 0, entries: [{ kind: "agent", text }] }, cursor: { start: 0, next: 1, fingerprint: text } });
  cache.seed("three", kept("d"));
  cache.seed("four", kept("e"));
  assert.ok(cache.peek("open"));
  assert.equal(cache.peek("two"), undefined);
  assert.equal(cache.peek("three"), undefined);
  assert.ok(cache.peek("four"));
});
