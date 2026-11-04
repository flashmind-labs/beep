import test from "node:test";
import assert from "node:assert/strict";
import { AnsweredIds, OrbAsks } from "../src/chat/approvals.ts";

const ask = (query_id: string, session = "chat-a", work = false) => ({ session, approval: { query_id }, work });

test("an approval answered here stays gone when a list fetched before the answer lands late", () => {
  const answered = new AnsweredIds();
  // The chat is redrawn: the fetch leaves while the approval still waits.
  const stale = ["q1"];
  answered.mark("q1", "chat-a");
  // The fetch comes back after the answer, still listing it.
  assert.deepEqual(answered.keep("chat-a", stale, q => q), []);
  // Read before the resumed run saved its result: still listed, still gone.
  assert.deepEqual(answered.keep("chat-a", ["q1"], q => q), []);
  // The server dropped it, so the id is forgotten.
  assert.deepEqual(answered.keep("chat-a", [], q => q), []);
  assert.equal(answered.has("q1"), false);
});

test("another chat's list does not forget an answer", () => {
  const answered = new AnsweredIds();
  answered.mark("q1", "chat-a");
  answered.keep("chat-b", ["q9"], q => q);
  assert.deepEqual(answered.keep("chat-a", ["q1", "q2"], q => q), ["q2"]);
});

test("a failed answer lets the card show again", () => {
  const answered = new AnsweredIds();
  answered.mark("q1", "chat-a");
  answered.forget("q1");
  assert.deepEqual(answered.keep("chat-a", ["q1"], q => q), ["q1"]);
});

test("the orb shows background work and other chats' approvals, one at a time", () => {
  const asks = new OrbAsks<ReturnType<typeof ask>>();
  assert.equal(asks.add(ask("turn")), true);
  assert.equal(asks.add(ask("work", "chat-a", true)), true);
  assert.equal(asks.add(ask("other", "chat-b")), true);
  // The stream and the notice of one approval show it once.
  assert.equal(asks.add(ask("turn")), false);
  assert.equal(asks.shown?.approval.query_id, "turn");
  assert.equal(asks.count, 3);
  // Answered in the bar: the next one is on the card.
  assert.equal(asks.remove("turn"), true);
  assert.equal(asks.shown?.approval.query_id, "work");
});

test("an approval answered in the orb never comes back on a repeated event", () => {
  const asks = new OrbAsks<ReturnType<typeof ask>>();
  asks.add(ask("q1"));
  assert.equal(asks.answer()?.approval.query_id, "q1");
  assert.equal(asks.shown, null);
  // The bar hid and the orb was shown again: Rust repeats what it had.
  assert.equal(asks.add(ask("q1")), false);
  assert.equal(asks.shown, null);
});

test("a failed answer in the orb puts the card back first", () => {
  const asks = new OrbAsks<ReturnType<typeof ask>>();
  asks.add(ask("q1"));
  asks.add(ask("q2"));
  const answered = asks.answer()!;
  asks.failed(answered);
  assert.equal(asks.shown?.approval.query_id, "q1");
  assert.equal(asks.count, 2);
});
