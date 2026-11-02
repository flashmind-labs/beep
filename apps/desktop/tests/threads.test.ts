import test from "node:test";
import assert from "node:assert/strict";
import { threadCode, issueTile, threadTile, issueMeta, threadMeta, workChoices } from "../src/threads.ts";

const ago = () => "2 h ago";

test("threads are named by kind and number", () => {
  assert.equal(threadCode({ kind: "pr", number: 41 }), "PR #41");
  assert.equal(threadCode({ kind: "issue", number: 31 }), "#31");
});

test("the Issues tile counts the untracked issues", () => {
  assert.equal(issueTile(undefined).empty, "Loading…");
  assert.equal(issueTile(null).rows.length, 0);
  const tile = issueTile([
    { number: 52, title: "Crash on rotate", labels: ["bug"], comments: 0, url: "https://github.com/acme/widget/issues/52" },
    { number: 48, title: "Onboarding in Spanish", labels: [], comments: 4, url: "https://github.com/acme/widget/issues/48" },
  ]);
  assert.equal(tile.count, "2 untracked");
  assert.deepEqual(tile.rows.map(r => [r.code, r.note]), [["#52", "bug"], ["#48", "4 comments"]]);
  assert.equal(issueTile([]).empty, "No untracked issues");
});

test("the Pull requests tile puts what needs a reply first", () => {
  const fallback = { count: "1 open", rows: [{ text: "from a task" }], empty: "No pull requests from tasks yet" };
  assert.equal(threadTile(undefined, fallback), fallback);
  assert.equal(threadTile(null, fallback), fallback);
  const tile = threadTile([
    { kind: "pr", number: 42, title: "Spanish strings", needs_reply: false },
    { kind: "issue", number: 31, title: "Upload fails", needs_reply: true },
    { kind: "pr", number: 41, title: "Login copy", needs_reply: true },
    { kind: "issue", number: 30, title: "Old question", needs_reply: false },
  ], fallback);
  assert.equal(tile.count, "2 need reply");
  assert.deepEqual(tile.rows.map(r => [r.code, r.chip?.text || ""]), [["#31", "reply"], ["#41", "reply"], ["#42", ""]]);
  assert.equal(threadTile([{ kind: "pr", number: 1, title: "x", needs_reply: false }], fallback).count, "1 open");
});

test("an issue's line says who opened it and how busy it is", () => {
  assert.equal(issueMeta({ author: "sam", opened_at: 1, labels: ["bug"], comments: 1 }, ago), "opened by sam · 2 h ago · bug · 1 comment");
  assert.equal(threadMeta({ last_other: "sam: Logs?", updated_at: 1, needs_reply: true }, ago), "sam: Logs? · 2 h ago");
  assert.equal(threadMeta({ last_other: "", updated_at: 0, needs_reply: false }, ago), "your last comment is the newest");
});

test("Work on it offers the machines' agents, first one picked", () => {
  const hosts = [
    { machine_id: "a", machine_name: "build-box", agents: ["claude", "codex"] },
    { machine_id: "b", machine_name: "mac-box", agents: ["codex", "pi"] },
    { machine_id: "c", machine_name: "gpu-box", agents: ["flsh"] },
  ];
  const where = [{ id: "a", kind: "here" }, { id: "b", kind: "here" }, { id: "c", kind: "missing" }];
  const c = workChoices(hosts, where);
  assert.deepEqual(c.agents, ["claude", "codex", "pi", "flsh"]);
  assert.deepEqual(c.machines.map(m => m.name), ["build-box", "mac-box"]);
  assert.equal(c.agent, "claude");
  assert.equal(c.machine, "a");
  const none = workChoices([], []);
  assert.equal(none.agent, null);
  assert.equal(none.machine, null);
});
