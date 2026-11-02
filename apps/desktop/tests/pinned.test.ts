import test from "node:test";
import assert from "node:assert/strict";
import { splitPinned } from "../src/pinned.ts";

const rows = [
  { id: "a", title: "A", kind: "chat", pinned: false },
  { id: "b", title: "B", kind: "chat", pinned: true },
  { id: "g", title: "G", kind: "group", pinned: true },
  { id: "c", title: "C", kind: "chat", pinned: false },
];

test("a pinned chat is under Pinned and not under Chats", () => {
  const { pinned, recent } = splitPinned(rows, 12);
  assert.deepEqual(pinned.map(c => c.id), ["b", "g"]);
  assert.deepEqual(recent.map(c => c.id), ["a", "c"]);
});

test("the Chats limit counts only unpinned chats", () => {
  const { recent } = splitPinned(rows, 1);
  assert.deepEqual(recent.map(c => c.id), ["a"]);
});

test("nothing pinned leaves Pinned empty", () => {
  assert.deepEqual(splitPinned(rows.filter(c => !c.pinned), 12).pinned, []);
});
