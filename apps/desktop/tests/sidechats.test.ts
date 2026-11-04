import test from "node:test";
import assert from "node:assert/strict";
import { sideThreads, type ThreadChatEntry } from "../src/center/sidechats.ts";

const thread = (chat_id: string, project: string, number: number): ThreadChatEntry => ({
  chat_id, kind: "pr", repo: project, number, title: `PR #${number} Fix`, project_id: `id-${project}`, project,
});

test("each project lists its newest thread chats under it", () => {
  const chats = [thread("a", "acme/widget", 1), thread("b", "acme/widget", 2), thread("c", "acme/widget", 3), thread("d", "acme/other", 4)];
  const nested = sideThreads(chats, ["c", "x", "a", "b", "d"], null, new Set(), 2);
  assert.deepEqual(nested.get("acme/widget")?.map(t => t.chat_id), ["c", "a"]);
  assert.deepEqual(nested.get("acme/other")?.map(t => t.chat_id), ["d"]);
});

test("the open and running thread chats always show", () => {
  const chats = [thread("a", "acme/widget", 1), thread("b", "acme/widget", 2), thread("c", "acme/widget", 3)];
  const nested = sideThreads(chats, ["a", "b", "c"], "c", new Set(["b"]), 1);
  assert.deepEqual(nested.get("acme/widget")?.map(t => t.chat_id), ["b", "c"]);
});
