import test from "node:test";
import assert from "node:assert/strict";
import { endedPage, hasNews, LINKED, lostNote, noteAfterRead, readFailed, sendError, viewError } from "../src/stores/agents/reconnect-state.ts";
import { joinRuns } from "../src/stores/agents/older-state.ts";
import type { AgentPage } from "../src/stores/agents/agents-state.ts";

const page = (status: string, lines: number) => ({
  session: { status, question: null, machine_name: "build-box" },
  entries: Array.from({ length: lines }, (_, i) => ({ kind: "agent", text: `e${i}` })),
  earlier: 0,
}) as unknown as AgentPage;

test("a rejected read is told apart by its kind; a plain string is a failure", () => {
  assert.deepEqual(viewError({ kind: "lost" }), { kind: "lost" });
  assert.deepEqual(viewError({ kind: "gone" }), { kind: "gone" });
  assert.deepEqual(viewError({ kind: "failed", message: "no such agent" }), { kind: "failed", message: "no such agent" });
  assert.deepEqual(viewError("sending stopped by peer: error 0"), { kind: "failed", message: "sending stopped by peer: error 0" });
});

test("a lost link says so and counts the reads lost in a row", () => {
  let link = LINKED;

  for (let i = 1; i <= 3; i++) {
    const failed = readFailed(link, { kind: "lost" }, "build-box");
    assert.equal(failed.note, "Lost the connection to build-box. Reconnecting...");
    link = failed.link;
    assert.equal(link.lost, i);
  }
});

test("reattached: the reconnect line goes and the session's real state replaces the running turn", () => {
  const held = page("working", 3);
  const lost = readFailed(LINKED, { kind: "lost" }, "build-box");
  const fresh = page("idle", 3);
  const note = noteAfterRead({ note: lost.note, failed: lost.note, link: lost.link, news: hasNews(held, fresh) });
  assert.equal(note, "");
  assert.equal(joinRuns(held, fresh).session.status, "idle");
});

test("a session its machine came back without ends in plain words, and its turn stops", () => {
  const lost = readFailed(LINKED, { kind: "lost" }, "build-box");
  const gone = readFailed(lost.link, { kind: "gone" }, "build-box");
  assert.equal(gone.note, "This session ended when build-box restarted.");
  assert.ok(gone.link.ended && !gone.link.lost);
  const ended = endedPage(page("working", 3));
  assert.equal(ended.session.status, "ended");
  // Read again later, it stays ended rather than turning into "gone".
  assert.equal(readFailed(gone.link, { kind: "gone" }, "build-box").note, gone.note);
});

test("a missing session with no lost link before it stays the usual note", () => {
  assert.deepEqual(readFailed(LINKED, { kind: "gone" }, "build-box"), { link: LINKED, note: "gone" });
});

test("an error, then a new event: no error is shown", () => {
  // A send that failed while the node restarted left its own error.
  const error = "sending stopped by peer: error 0";
  const held = page("working", 3);
  const quiet = noteAfterRead({ note: error, failed: "", link: LINKED, news: hasNews(held, page("working", 3)) });
  assert.equal(quiet, error);
  assert.equal(noteAfterRead({ note: error, failed: "", link: LINKED, news: hasNews(held, page("working", 4)) }), "");
  assert.equal(noteAfterRead({ note: error, failed: "", link: LINKED, news: hasNews(held, page("idle", 3)) }), "");
  // The reconnect line too, once the session reads again.
  const note = lostNote("build-box");
  assert.equal(noteAfterRead({ note, failed: note, link: { lost: 2, ended: false }, news: false }), "");
});

test("a failed send says whether anything went", () => {
  assert.deepEqual(sendError({ kind: "lost", message: "Lost the connection to build-box." }), { kind: "lost", message: "Lost the connection to build-box." });
  assert.equal(sendError({ kind: "unsure", message: "x" }).kind, "unsure");
  assert.equal(sendError({ kind: "waiting", message: "x" }).kind, "waiting");
  assert.deepEqual(sendError({ kind: "failed", message: "bad request: The message is empty." }), { kind: "failed", message: "The message is empty." });
  // The page stopped waiting: the outbox has the message, so it is not put back in the field.
  assert.deepEqual(sendError("no answer in 40 s. Try again."), { kind: "timeout", message: "no answer in 40 s. Try again." });
});
