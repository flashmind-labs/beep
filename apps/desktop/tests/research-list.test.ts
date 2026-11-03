import test from "node:test";
import assert from "node:assert/strict";
import { newestFirst, phaseStep, progressLine, researchState, researchSummary, STATE_LABEL, withoutResearch, type ResearchRow, type ResearchStatus } from "../src/research-list.ts";

const MIN = 60_000;
const at = Date.UTC(2026, 10, 3, 9, 0);
const row = (session: string, status: ResearchStatus, started_at: number, more: Partial<ResearchRow> = {}): ResearchRow => ({
  session, topic: `Topic ${session}`, status, round: 1, sources: 0, started_at, finished_at: null, ...more,
});

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

test("the Research list is newest first, whatever order the server sent", () => {
  const list = [row("old", "done", at - 3 * 24 * 60 * MIN), row("new", "researching", at), row("mid", "planning", at - 60 * MIN)];
  assert.deepEqual(newestFirst(list).map(r => r.session), ["new", "mid", "old"]);
  // The server's list is not changed.
  assert.equal(list[0].session, "old");
});

test("each status reads as running, waiting, done, failed or stopped", () => {
  const states = (["planning", "researching", "reviewing", "writing", "done", "failed", "stopped", "interrupted"] as const).map(researchState);
  assert.deepEqual(states, ["waiting", "running", "running", "running", "done", "failed", "stopped", "stopped"]);
  assert.equal(STATE_LABEL.waiting, "Waiting for your go");
  assert.equal(STATE_LABEL.running, "Running");
});

test("a running research shows its round and step", () => {
  assert.equal(phaseStep("researching"), 1);
  assert.equal(phaseStep("writing"), 3);
  assert.equal(phaseStep("done"), 0);
  assert.equal(progressLine(row("a", "reviewing", at)), "Reviewing");
  assert.equal(progressLine(row("a", "researching", at, { round: 2 })), "Round 2 · Researching");
  assert.equal(progressLine(row("a", "planning", at)), "Plan ready");
});

test("a finished research says how long it took, a stopped one where it stopped", () => {
  assert.equal(progressLine(row("a", "done", at, { finished_at: at + 14 * MIN })), "took 14 min");
  assert.equal(progressLine(row("a", "done", at, { round: 2, finished_at: at + 130 * MIN })), "2 rounds · took 2 h");
  assert.equal(progressLine(row("a", "failed", at)), "Failed in round 1");
  assert.equal(progressLine(row("a", "interrupted", at, { round: 2 })), "Cut off in round 2");
});

test("the page's line counts what runs and waits", () => {
  assert.equal(researchSummary([]), "Nothing researched yet");
  assert.equal(researchSummary([row("a", "done", at)]), "1 research");
  assert.equal(researchSummary([row("a", "writing", at), row("b", "planning", at), row("c", "done", at)]), "1 running, 1 waiting · 3 researches");
});
