import test from "node:test";
import assert from "node:assert/strict";
import { type Attached, dropDelivered, markDelivered, pendingSteers, stopPending, syncSteers, type Waiting } from "../src/agents/agentqueue.ts";

const img = (name: string, size = 10): Attached => ({ name, mime: "image/png", size, data: "", url: "" });
const item = (text: string, images: Attached[] = []): Waiting => ({ how: "steer", typed: text, text, paths: [], images, state: null, after: 0 });
const steer = (text: string, after: number): Waiting => ({ ...item(text), how: "steer", state: "sent", after });

test("a steer counts as delivered only from reads after the node took it", () => {
  const q = [steer("old", 1), steer("new", 5)];
  // Read 3 left before "new" was taken: only "old" is judged.
  assert.equal(markDelivered(q, 0, 3), true);
  assert.deepEqual(q.map(x => x.state), ["delivered", "sent"]);
  // Read 6 says one still waits: "new" is held.
  assert.equal(markDelivered(q, 1, 6), false);
  assert.equal(markDelivered(q, 0, 7), true);
  assert.deepEqual(q.map(x => x.state), ["delivered", "delivered"]);
  dropDelivered(q);
  assert.deepEqual(q, []);
});

test("delivered steers disappear immediately while pending messages stay", () => {
  const delivered = steer("delivered", 1);
  const sending: Waiting = { ...steer("sending", 1), state: "sending" };
  const sent = steer("sent", 5), queued = item("queued");
  const q = [delivered, sending, sent, queued];
  markDelivered(q, 0, 3);
  dropDelivered(q);
  assert.deepEqual(q, [sending, sent, queued]);
});

test("a stop clears when its turn ends between reads and a steer starts another", () => {
  const stop = { id: "session", turns: 3 };
  assert.equal(stopPending(stop, "session", true, 3), stop);
  assert.equal(stopPending(stop, "session", true, 4), null);
  assert.equal(stopPending(stop, "session", false, 3), null);
  assert.equal(stopPending(stop, "other", true, 3), null);
});

test("a steer appears in the chat while sending and waiting for delivery", () => {
  const q: Waiting = { ...item("check the fixture"), state: "sending", entryFrom: 1 };
  const run = { earlier: 0, entries: [{ kind: "user", text: "start" }] };
  assert.deepEqual(pendingSteers([q], run), [q]);
  q.state = "sent";
  assert.deepEqual(pendingSteers([q], run), [q]);
  q.state = "delivered";
  assert.deepEqual(pendingSteers([q], run), []);
});

test("an echo arriving before send acknowledgement replaces the pending chat row", () => {
  const q: Waiting = { ...item("check the fixture"), state: "sending", entryFrom: 2 };
  const run = { earlier: 2, entries: [{ kind: "user", text: q.text }] };
  assert.deepEqual(pendingSteers([q], run), []);
  assert.equal(q.state, "sending", "rendering does not change delivery state");
});

test("older matching messages and repeated steers do not hide pending messages", () => {
  const first: Waiting = { ...steer("again", 1), entryFrom: 1 };
  const second = { ...first };
  const run = { earlier: 0, entries: [{ kind: "user", text: "again" }] };
  assert.deepEqual(pendingSteers([first, second], run), [first, second]);
  run.entries.push({ kind: "user", text: "again" });
  assert.deepEqual(pendingSteers([first, second], run), [second]);
});

test("a steer with images is replaced by its recorded image entry", () => {
  const q: Waiting = { ...item("check this", [img("screen.png")]), state: "sent", entryFrom: 0 };
  const run = { earlier: 0, entries: [{ kind: "user", text: "check this\n[image: image-0123456789abcdef]" }] };
  assert.deepEqual(pendingSteers([q], run), []);
});

test("reopening restores every queued message once and retires it after delivery", () => {
  const queue: Waiting[] = [];
  const run = { earlier: 10, entries: [{ kind: "user", text: "again" }], queued: 2, queued_messages: ["again", "again"] };
  syncSteers(queue, run, 3);
  assert.deepEqual(queue.map(q => [q.text, q.state, q.entryFrom]), [["again", "sent", 11], ["again", "sent", 11]]);
  assert.equal(pendingSteers(queue, run).length, 2, "a previous identical message does not hide the pending steers");
  syncSteers(queue, run, 4);
  assert.equal(queue.length, 2);
  syncSteers(queue, { ...run, queued: 0, queued_messages: [] }, 5);
  dropDelivered(queue);
  assert.deepEqual(queue, []);
});

test("restoring preserves local attachments and sends newer than the read", () => {
  const attached = { ...steer("look", 1), images: [img("shot.png")], paths: ["fixture.json"] };
  const sending: Waiting = { ...item("still sending"), state: "sending" };
  const newer = steer("newer", 5);
  const queue = [attached, sending, newer];
  syncSteers(queue, { earlier: 0, entries: [], queued: 1, queued_messages: ["look\n[Image: shot.png]"] }, 3);
  assert.deepEqual(queue, [attached, sending, newer]);
  assert.equal(attached.state, "sent");
  assert.equal(sending.state, "sending");
  assert.equal(newer.state, "sent");
});

test("an acknowledged send is not duplicated when restoring before its reply", () => {
  const sending: Waiting = { ...item("check the fixture"), state: "sending" };
  const queue = [sending];
  syncSteers(queue, { earlier: 0, entries: [], queued: 1, queued_messages: [sending.text] }, 3);
  assert.deepEqual(queue, [sending]);
  assert.equal(sending.state, "sending");
});

test("restoring repeated steers retires the oldest occurrence first", () => {
  const first = steer("again", 1), second = steer("again", 2);
  const queue = [first, second];
  syncSteers(queue, { earlier: 0, entries: [], queued: 1, queued_messages: ["again"] }, 3);
  assert.equal(first.state, "delivered");
  assert.equal(second.state, "sent");
});

test("older nodes without queued message text still update delivery by count", () => {
  const queue = [steer("first", 1), steer("second", 1)];
  syncSteers(queue, { earlier: 0, entries: [], queued: 1 }, 3);
  assert.deepEqual(queue.map(q => q.state), ["delivered", "sent"]);
});
