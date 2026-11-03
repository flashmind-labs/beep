import test from "node:test";
import assert from "node:assert/strict";
import { withoutResearch } from "../src/research-list.ts";

test("research chats leave the Chats list, other chats and groups stay", () => {
  const chats = [
    { kind: "chat", id: "lunch", title: "Lunch with Sam" },
    { kind: "chat", id: "r1", title: "Solid-state batteries" },
    { kind: "group", id: "trip", title: "Tokyo trip" },
    { kind: "chat", id: "r2", title: "Heat pumps" },
  ];
  const kept = withoutResearch(chats, new Set(["r1", "r2", "gone"]));
  assert.deepEqual(kept.map(c => c.id), ["lunch", "trip"]);
  assert.deepEqual(withoutResearch(chats, new Set()).map(c => c.id), ["lunch", "r1", "trip", "r2"]);
});
