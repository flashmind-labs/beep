import test from "node:test";
import assert from "node:assert/strict";
import { type Entry, type Open, type Research, closeTab, doing, openTab, progress, shownTab } from "../src/research/research-state.ts";

const agents = [
  { id: "lead", name: "Lead", role: "", model: "", round: 0 },
  { id: "scout-1", name: "Scout 1", role: "", model: "", round: 1 },
  { id: "critic", name: "Critic", role: "", model: "", round: 0 },
  { id: "writer", name: "Writer", role: "", model: "", round: 0 },
];
const research = (status: Research["status"], draft = ""): Research => ({
  topic: "t", status, agents, sources: [], draft, draft_html: draft, started_at: 0, finished_at: null, round: 1,
});
const step: Entry = { seq: 1, agent: "scout-1", at: 0, kind: "step", label: "Read a report", success: true };
const open = (r: Research | null): Open => ({
  session: "s", research: r, entries: [step], after: 1, turns: [], tabs: [], tab: null, pick: null, menu: false, sending: null, error: "", exporting: false,
});

test("agents show what they do in each phase", () => {
  const researching = open(research("researching"));
  assert.deepEqual(doing(researching, "scout-1"), ["Read a report", "run"]);
  assert.deepEqual(doing(researching, "critic"), ["Starts after the scouts", ""]);
  const writing = open(research("writing"));
  assert.deepEqual(doing(writing, "scout-1"), ["Done: 0 notes, 0 sources", "ok"]);
  assert.deepEqual(doing(open(research("planning")), "lead"), ["Waiting for your go", "wait"]);
});

test("the progress line counts the scouts at work and the minutes", () => {
  const o = open(research("researching"));
  assert.equal(progress(o, 6 * 60000), "Researching: 1 of 1 scout working, 6 min");
  assert.equal(progress(open(research("writing")), 12 * 60000), "The writer is drafting, 12 min");
});

test("the pane shows the paper once written, else the agents, until a tab is picked", () => {
  assert.equal(shownTab(open(research("researching"))), "agents");
  assert.equal(shownTab(open(research("done", "# Paper"))), "draft");
  const o = open(research("done", "# Paper"));
  openTab(o, "scout-1");
  assert.equal(shownTab(o), "scout-1");
  closeTab(o, "scout-1");
  assert.deepEqual([o.tabs, shownTab(o)], [[], "agents"]);
});
