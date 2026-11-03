import test from "node:test";
import assert from "node:assert/strict";
import { type Attached, batches, DELIVERED_SHOWN_MS, dropDelivered, markDelivered, type Waiting } from "../src/bar/agentqueue.ts";

const img = (name: string, size = 10): Attached => ({ name, mime: "image/png", size, data: "", url: "" });
const item = (text: string, images: Attached[] = []): Waiting => ({ how: "queue", typed: text, text, paths: [], images, state: null, after: 0 });
const steer = (text: string, after: number): Waiting => ({ ...item(text), how: "steer", state: "sent", after });

test("images past what one message takes start the next message", () => {
  const out = batches([item("one", [img("a"), img("b")]), item("two", [img("c")]), item("three", [img("d", 95)])], 2, 100);
  assert.deepEqual(out.map(b => b.text), ["one", "two", "three"]);
});

test("a steer counts as delivered only from reads after the node took it", () => {
  const q = [steer("old", 1), steer("new", 5)];
  // Read 3 left before "new" was taken: only "old" is judged.
  assert.equal(markDelivered(q, 0, 3, 1000), true);
  assert.deepEqual(q.map(x => x.state), ["delivered", "sent"]);
  // Read 6 says one still waits: "new" is held.
  assert.equal(markDelivered(q, 1, 6, 1000), false);
  assert.equal(markDelivered(q, 0, 7, 1000), true);
  assert.deepEqual(q.map(x => x.state), ["delivered", "delivered"]);
  dropDelivered(q, 1000 + DELIVERED_SHOWN_MS);
  assert.deepEqual(q, []);
});
