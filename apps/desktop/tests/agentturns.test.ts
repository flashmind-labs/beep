import test from "node:test";
import assert from "node:assert/strict";
import { beepCall, beepLine, commandDetail, commandOf, diffBlocks, keptSteps, keptThoughts, liveThought, runLabel, span, splitTurns, stepLabel, summary, toolKind, toolRuns, withoutDiffs } from "../src/agents/agentturns.ts";

const u = (text: string) => ({ kind: "user", text });
const a = (text: string) => ({ kind: "agent", text });
const t = (text: string) => ({ kind: "tool", text });

test("each user message opens a turn; the agent text it ends with is its answer", () => {
  const log = [u("Fix the login test"), a("Looking at the output."), t("cargo test login [failed]"), a("Changing the message."),
    t("Edit `src/login.rs` [completed]"), a("Done."), a("2 files changed."), u("Thanks"), a("Any time.")];
  assert.deepEqual(splitTurns(log), [
    { key: 0, user: 0, steps: [1, 2, 3, 4], answer: [5, 6] },
    { key: 7, user: 7, steps: [], answer: [8] },
  ]);
});

test("a log that starts mid-turn keys that turn by its first entry, in session places", () => {
  assert.deepEqual(splitTurns([t("ls [completed]"), a("ok"), u("next")], 40), [
    { key: 40, user: null, steps: [40], answer: [41] },
    { key: 42, user: 42, steps: [], answer: [] },
  ]);
  assert.deepEqual(splitTurns([]), []);
});

test("a turn's span comes from the session or from what the window saw", () => {
  const base = { first: false, last: false, running: false, sessionStart: 1000, lastActivity: 9000 };
  assert.deepEqual(span({ ...base, first: true, last: true }), { start: 1000, end: 9000 });
  assert.deepEqual(span({ ...base, first: true, running: true, last: true }), { start: 1000, end: undefined });
  assert.deepEqual(span({ ...base, seenStart: 5000, seenEnd: 7000 }), { start: 5000, end: 7000 });
  assert.deepEqual(span(base), { start: undefined, end: undefined });
  assert.deepEqual(span({ ...base, last: true, seenStart: 9500 }), { start: 9500, end: undefined });
});

test("notes and subagents stay out of the fold; a tool's pictures fold, counted", () => {
  const log = [u("go"), t("ls [completed]"), t("cargo test [failed]\nexit code: 101"), { kind: "other", text: "Switched to deep from the next turn" },
    t("Explore [completed]\n[steps] 1\n[step completed] Read a.rs\nFound it"), t("Screenshot [completed]\n[image: image-0123456789abcdef]"), a("Done.")];
  assert.deepEqual(keptSteps(log, splitTurns(log)[0]), [3, 4]);
  assert.equal(summary(log, splitTurns(log)[0]), "Ran 3 commands (1 failed), searched once, viewed 1 image");
});

test("failed steps fold with the rest; the summary counts them with their kind", () => {
  const log = [u("go"), t("git ls-files | grep foo [failed]\nexit code: 1"), t("cargo test [completed]"),
    t("exec: grep -rn bar src [failed]\nexit 1"), t("Edit `a.rs` [completed]"), t("Grep: fn main (failed)"),
    t("TodoWrite: plan [completed]"), t("WebFetch: docs [failed]"), a("Done.")];
  const turn = splitTurns(log)[0];
  assert.deepEqual(keptSteps(log, turn), []);
  assert.equal(summary(log, turn), "Ran 3 commands (2 failed), edited 1 file, performed 3 other actions (2 failed)");
  const ok = [u("go"), t("make [completed]"), a("ok")];
  assert.equal(summary(ok, splitTurns(ok)[0]), "Ran 1 command");
});

test("an opened step does not print its command twice", () => {
  const title = "exec: cd /srv/project;\n  git ls-files | grep widget";
  const whole = stepLabel(`${title} [failed]\nexit 1`, false, true);
  assert.equal(whole, "Ran cd /srv/project;\n  git ls-files | grep widget");
  assert.equal(commandDetail(title, whole), "");
  assert.equal(commandDetail("exec:  make   test", "Ran make test"), "");
  assert.equal(commandDetail("Bash: make", "Ran make"), "");
  assert.equal(commandDetail("TodoWrite: plan", "TodoWrite: plan"), "");
  // Closed, the label is the first line; the rest of the command is new.
  const closed = stepLabel(`${title} [failed]`);
  assert.equal(closed, "Ran cd /srv/project; …");
  assert.equal(commandDetail(title, closed), "cd /srv/project;\n  git ls-files | grep widget");
  assert.equal(commandOf("exec: make"), "make");
  assert.equal(commandOf("make"), "make");
});

test("a Beep call reads as its machine, command, files and verb", () => {
  assert.deepEqual(beepCall("Beep: Run on build-box: cargo test"),
    { done: "Ran on", doing: "Running on", machine: "build-box", detail: "cargo test", files: "" });
  assert.deepEqual(beepCall("Beep: Start codex on mac-mini: Build the app\nthen test it"),
    { done: "Started codex on", doing: "Starting codex on", machine: "mac-mini", detail: "Build the app\nthen test it", files: "" });
  assert.deepEqual(beepCall("Beep: Send 3 files to build-box"),
    { done: "Sent files to", doing: "Sending files to", machine: "build-box", detail: "", files: "3 files" });
  assert.deepEqual(beepCall("Beep: Sync widget to build-box"),
    { done: "Synced", doing: "Syncing", machine: "build-box", detail: "widget", files: "" });
  assert.equal(beepCall("Beep: Sync widget")?.machine, "");
  assert.equal(beepCall("Beep: Check job job-7f2c")?.detail, "job-7f2c");
  assert.equal(beepCall("Beep: Cancel job job-7f2c")?.done, "Cancelled job");
  assert.equal(beepCall("Beep: Ask: Push it?")?.detail, "Push it?");
  assert.equal(beepCall("Beep: List machines")?.done, "Listed machines");
  assert.equal(beepCall("Beep: Read log of build-box")?.machine, "build-box");
  // Before its arguments come, no machine is named.
  assert.equal(beepCall("Beep: Run on a machine")?.machine, "");
  assert.equal(beepCall("Run cargo test"), null);
  assert.equal(beepCall("mcp__beep__run_on"), null);
  assert.equal(beepLine("Beep: Run on build-box: make"), "Ran on build-box: make");
  assert.equal(beepLine("Beep: Send 2 files to build-box", true), "Sending 2 files to build-box");
  assert.equal(stepLabel("Beep: Run on build-box: make [completed]"), "Ran on build-box: make");
  assert.equal(toolKind("Beep: Run on build-box: make [completed]"), "beep");
});

const th = (text: string, ms: number | null = null) => ({ kind: "thought", text, ms });

test("only the running turn's last entry, a thought without a length, is still thinking", () => {
  const log = [u("go"), th("Reading", 4000), t("Read `a.rs` [completed]"), th("Now the tests")];
  const [turn] = splitTurns(log);
  assert.equal(liveThought(log, turn, true), 3);
  assert.equal(liveThought(log, turn, false), null, "a finished turn thinks no more");
  const ended = [...log.slice(0, 3), th("Now the tests", 2000)];
  assert.equal(liveThought(ended, splitTurns(ended)[0], true), null);
  const answered = [...log, a("Done.")];
  assert.equal(liveThought(answered, splitTurns(answered)[0], true), null);
});

test("thoughts fold with the tools, and stay in view when no tool is left to fold", () => {
  const withTools = [u("go"), th("Reading", 1000), t("Read `a.rs` [completed]"), th("Done", 1000), a("Fixed.")];
  const turn = splitTurns(withTools)[0];
  assert.deepEqual([...keptThoughts(withTools, turn, new Set(keptSteps(withTools, turn)))], []);
  // The running turn keeps its newest tool in view: nothing left to fold.
  assert.deepEqual([...keptThoughts(withTools, turn, new Set([2]))].sort(), [1, 2, 3]);
  const onlyThoughts = [u("hi"), th("Greeting", 1000), a("Hello.")];
  const plain = splitTurns(onlyThoughts)[0];
  assert.deepEqual([...keptThoughts(onlyThoughts, plain, new Set(keptSteps(onlyThoughts, plain)))], [1]);
});

test("an edit's diff is read back from its output, and the rest of the output kept apart", () => {
  const output = "[diff] src/limits.rs +2 -1\n fn a() {\n-    3\n+    5\n… 3 more lines\nsaved";
  assert.deepEqual(diffBlocks(output), [{ path: "src/limits.rs", added: 2, removed: 1, more: 3,
    lines: [{ mark: " ", text: "fn a() {" }, { mark: "-", text: "    3" }, { mark: "+", text: "    5" }] }]);
  assert.equal(withoutDiffs(output), "saved");
  assert.deepEqual(diffBlocks("plain output"), []);
});

test("finished calls of one kind in a row fold into one run; failures, running calls and other entries stand alone", () => {
  const log = [u("go"), t("Read a.rs [completed]"), t("Read b.rs [completed]"), t("Read c.rs [completed]"),
    t("cargo test [completed]"), t("Read d.rs [failed]"), t("Read e.rs [completed]"), th("hmm", 1000), t("Read f.rs [completed]"), t("Read g.rs [in_progress]")];
  assert.deepEqual(toolRuns(log, [1, 2, 3, 4, 5, 6, 7, 8, 9]), [[1, 2, 3], [4], [5], [6], [7], [8], [9]]);
  assert.equal(runLabel("read", 6), "Read 6 files");
  assert.equal(runLabel("command", 1), "Ran 1 command");
  assert.equal(runLabel("search", 3), "Searched 3 times");
});
