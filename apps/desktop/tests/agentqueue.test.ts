import test from "node:test";
import assert from "node:assert/strict";
import { type Attached, dropDelivered, markDelivered, pendingSteers, stopPending, type Waiting } from "../src/agents/agentqueue.ts";

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
