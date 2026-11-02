import test from "node:test";
import assert from "node:assert/strict";
import { openLabel, runningChats, sideThreads, threadLabel, type ThreadChatEntry } from "../src/sidechats.ts";

const thread = (chat_id: string, project: string, number: number): ThreadChatEntry => ({
  chat_id, kind: "pr", repo: project, number, title: `PR #${number} Fix`, project_id: `id-${project}`, project,
});

test("a chat runs when this window runs it or the server says so", () => {
  const running = runningChats([null, "here"], ["phone"]);
  assert.deepEqual([...running].sort(), ["here", "phone"]);
  assert.equal(runningChats([], null).size, 0);
});

test("threads are labelled by kind and number", () => {
  assert.equal(threadLabel("pr", 41), "PR #41");
  assert.equal(threadLabel("issue", 7), "Issue #7");
});

test("a GitHub link opens on GitHub, any other in the browser", () => {
  assert.equal(openLabel("https://github.com/acme/widget/pull/41"), "Open on GitHub");
  assert.equal(openLabel("https://git.example.org/acme/widget/pulls/41"), "Open in browser");
  assert.equal(openLabel("not a url"), "Open in browser");
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

test("a thread chat missing from the chat list comes last", () => {
  const chats = [thread("old", "acme/widget", 1), thread("new", "acme/widget", 2)];
  const nested = sideThreads(chats, ["new"], null, new Set(), 3);
  assert.deepEqual(nested.get("acme/widget")?.map(t => t.chat_id), ["new", "old"]);
});
