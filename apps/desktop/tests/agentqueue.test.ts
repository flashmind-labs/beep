import test from "node:test";
import assert from "node:assert/strict";
import { type Attached, attachOf, dropDelivered, markDelivered, type OutboxRow, outboxNote, sentFromOutbox, stopPending, syncSteers, type Waiting } from "../src/agents/agentqueue.ts";

const img = (name: string, size = 10): Attached => ({ name, mime: "image/png", size, data: "", url: "", original: null });
const item = (text: string, images: Attached[] = []): Waiting => ({ typed: text, text, paths: [], images, state: null, after: 0 });
const steer = (text: string, after: number): Waiting => ({ ...item(text), state: "sent", after });

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

test("reopening restores every queued message once and retires it after delivery", () => {
  const queue: Waiting[] = [];
  const run = { earlier: 10, entries: [{ kind: "user", text: "again" }], queued: 2, queued_messages: ["again", "again"] };
  syncSteers(queue, run, 3);
  assert.deepEqual(queue.map(q => [q.text, q.state]), [["again", "sent"], ["again", "sent"]]);
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

test("a steer another app sent shows on a later read and leaves when the node drops it", () => {
  const queue: Waiting[] = [];
  syncSteers(queue, { earlier: 0, entries: [], queued: 0, queued_messages: [] }, 1);
  syncSteers(queue, { earlier: 0, entries: [], queued: 1, queued_messages: ["from the phone"] }, 2);
  assert.deepEqual(queue.map(q => [q.text, q.state]), [["from the phone", "sent"]]);
  syncSteers(queue, { earlier: 0, entries: [], queued: 1, queued_messages: ["from the phone"] }, 3);
  assert.equal(queue.length, 1);
  syncSteers(queue, { earlier: 0, entries: [], queued: 0, queued_messages: [] }, 4);
  dropDelivered(queue);
  assert.deepEqual(queue, []);
});

const out: OutboxRow = { id: 0, how: "steer", text: "hello", images: [], state: "sending", reason: null, keys: [] };
const run = { earlier: 0, entries: [] };

test("an outbox event after the page applied the turn adds no row", () => {
  const queue: Waiting[] = [];
  const page = { ...run, queued: 0, queued_messages: [] };
  assert.equal(sentFromOutbox(out, queue, page, 4), null);
});

test("a page that lists the message before the outbox event draws it once", () => {
  const queue: Waiting[] = [];
  const page = { ...run, queued: 1, queued_messages: ["hello"] };
  syncSteers(queue, page, 4);
  assert.equal(sentFromOutbox(out, queue, page, 4), null);
  assert.deepEqual(queue.map(q => q.text), ["hello"]);
});

test("a busy session shows the message once the page lists it and drops it when taken", () => {
  const queue: Waiting[] = [];
  assert.equal(sentFromOutbox(out, queue, { ...run, queued: 0, queued_messages: [] }, 4), null);
  syncSteers(queue, { ...run, queued: 1, queued_messages: ["hello"] }, 5);
  assert.deepEqual(queue.map(q => [q.text, q.state]), [["hello", "sent"]]);
  syncSteers(queue, { ...run, queued: 0, queued_messages: [] }, 6);
  dropDelivered(queue);
  assert.deepEqual(queue, []);
});

test("an older node's page without message text still gets a local row for a held message", () => {
  const queue: Waiting[] = [];
  assert.equal(sentFromOutbox(out, queue, { ...run, queued: 0 }, 4), null);
  const row = sentFromOutbox(out, queue, { ...run, queued: 1 }, 4);
  assert.deepEqual(row && [row.text, row.state, row.after], ["hello", "sent", 4]);
  if (row) queue.push(row);
  assert.equal(sentFromOutbox(out, queue, { ...run, queued: 1 }, 4), null);
});

test("files go to the outbox with the paths the message mentions beside them", () => {
  const file = { name: "a.txt", mime: "text/plain", size: 1, data: "YQ==" };
  assert.equal(attachOf({ paths: ["src/lib.rs"], files: [] }), null);
  assert.deepEqual(attachOf({ paths: ["src/lib.rs", "attachment:1:a.txt"], files: [{ path: "attachment:1:a.txt", file }], root: "/srv/project" }), {
    root: "/srv/project", files: [{ name: "a.txt", mime: "text/plain", data: "YQ==" }], mentions: ["src/lib.rs"],
  });
});

test("an outbox row that uploads says so", () => {
  assert.equal(outboxNote({ id: 1, how: "prompt", text: "t", images: [], state: "uploading", reason: null, keys: [] }), "uploading");
});
