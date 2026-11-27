import test from "node:test";
import assert from "node:assert/strict";
import { beepCall, beepLine, commandOf, diffBlocks, keptSteps, liveThought, rowParts, runLabel, span, splitTurns, stepLabel, stepOpen, summary, progressSummary, toolKind, toolRuns, turnSections, withoutDiffs } from "../src/parts/session/agentturns.ts";

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

test("the files a turn made after its answer leave the answer in view", () => {
  const o = (text: string) => ({ kind: "other", text });
  assert.deepEqual(splitTurns([u("clean up"), t("git pull [completed]"), a("Pulled main."), o("Made 3 files")]), [
    { key: 0, user: 0, steps: [1], answer: [2, 3] },
  ]);
  assert.deepEqual(splitTurns([u("go"), t("ls [completed]"), o("Made a.md")]), [
    { key: 0, user: 0, steps: [1, 2], answer: [] },
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

test("an opened step has its whole command, without the verb", () => {
  const title = "exec: cd /srv/project;\n  git ls-files | grep widget";
  const entry = `${title} [failed]\nexit 1`;
  assert.equal(stepLabel(entry, false, true), "Ran cd /srv/project;\n  git ls-files | grep widget");
  assert.equal(stepLabel(entry), "Ran cd /srv/project; …");
  assert.equal(rowParts(entry).target, "cd /srv/project;");
  const opened = rowParts(entry, false, true);
  assert.equal(opened.verb, "Ran");
  assert.equal(opened.target, "cd /srv/project;\n  git ls-files | grep widget");
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
  assert.equal(beepCall("mcp__beep__remote_run"), null);
  assert.equal(beepLine("Beep: Run on build-box: make"), "Ran on build-box: make");
  assert.equal(beepLine("Beep: Send 2 files to build-box", true), "Sending 2 files to build-box");
  assert.equal(stepLabel("Beep: Run on build-box: make [completed]"), "Ran on build-box: make");
  assert.equal(toolKind("Beep: Run on build-box: make [completed]"), "beep");
});

test("a device call reads as the device and its machine", () => {
  assert.deepEqual(beepCall("Beep: Boot iPhone 17 on mac-box"),
    { done: "Booted iPhone 17 on", doing: "Booting iPhone 17 on", machine: "mac-box", detail: "", files: "" });
  assert.deepEqual(beepCall("Beep: Tap Pixel 9 on build-box: 120, 400"),
    { done: "Tapped Pixel 9 on", doing: "Tapping Pixel 9 on", machine: "build-box", detail: "120, 400", files: "" });
  assert.equal(beepCall("Beep: Press home on Pixel 9 on build-box")?.done, "Pressed home on Pixel 9 on");
  assert.equal(beepCall("Beep: Type into Pixel 9 on build-box: turn on wifi")?.detail, "turn on wifi");
  assert.equal(beepCall("Beep: List devices")?.done, "Listed devices");
  assert.equal(beepLine("Beep: Shut down Pixel 9 on build-box"), "Shut down Pixel 9 on build-box");
  assert.equal(beepLine("Beep: Capture Pixel 9 on build-box", true), "Capturing Pixel 9 on build-box");
});

test("agent starts expose the full prompt with or without a target machine", () => {
  const prompt = "Fix the login flow.\n\nRun `cargo test` and preserve <session>.\n" + "Last instruction. ".repeat(200);

  for (const [head, machine] of [["Start codex agent", ""], ["Start claude agent on build-box", "build-box"], ["Start an agent", ""]]) {
    const title = `Beep: ${head}: ${prompt}`;
    const call = beepCall(title)!;
    assert.equal(call.machine, machine);
    assert.equal(call.detail, prompt.trimEnd());
    assert.ok(!call.done.includes("Fix the login flow"));
    assert.equal(beepCall(title.replace(`: ${prompt}`, ""))?.detail, "");
  }
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

test("a row the reader chose stays as chosen when it is no longer the newest step", () => {
  // Failed and newest it starts open; the reader closes it, and a new step must not reopen it.
  assert.equal(stepOpen(true, true, undefined), true);
  assert.equal(stepOpen(true, true, false), false);
  assert.equal(stepOpen(true, false, false), false);
  // Opened by the reader it stays open after a new step arrives, failed or not.
  assert.equal(stepOpen(true, false, true), true);
  assert.equal(stepOpen(false, false, true), true);
  assert.equal(stepOpen(false, true, undefined), false);
  assert.equal(stepOpen(true, false, undefined), false);
});

test("a row the reader opened is not folded into a group or the turn's heading", () => {
  const log = [u("go"), t("Read a.rs [completed]"), t("Read b.rs [completed]"), t("Read c.rs [completed]"), a("done")];
  const turn = splitTurns(log)[0];
  assert.deepEqual(toolRuns(log, [1, 2, 3]), [[1, 2, 3]]);
  assert.deepEqual(toolRuns(log, [1, 2, 3], 0, new Set([2])), [[1], [2], [3]]);
  assert.deepEqual(keptSteps(log, turn), []);
  assert.deepEqual(keptSteps(log, turn, 0, new Set([1, 3])), [1, 3]);
});


test("agent output separates progress groups containing thoughts and tools", () => {
  const log = [u("go"), { kind: "thought", text: "check" }, t("ls [completed]"), t("pwd [completed]"),
    a("Found it."), t("cargo test [completed]"), a("All passed."), a("Done.")];
  assert.deepEqual(turnSections(log, splitTurns(log)[0]), [
    { progress: true, ats: [1, 2, 3] },
    { progress: false, ats: [4] }, { progress: true, ats: [5] }, { progress: false, ats: [6, 7] },
  ]);
  const partial = log.slice(2, 6);
  assert.deepEqual(turnSections(partial, splitTurns(partial, 42)[0], 42), [
    { progress: true, ats: [42, 43] }, { progress: false, ats: [44] }, { progress: true, ats: [45] },
  ]);
});


test("a response stays outside the fold when more tools arrive", () => {
  const log = [u("go"), t("ls [completed]"), a("Checking the result.")];
  const before = turnSections(log, splitTurns(log)[0]);
  log.push(t("cargo test [in_progress]"));
  const after = turnSections(log, splitTurns(log)[0]);
  assert.deepEqual(after.slice(0, 2), before);
  assert.deepEqual(after[2], { progress: true, ats: [3] });
});

test("alternating thoughts and tool calls accumulate in one progress group", () => {
  const log = [u("go"), { kind: "thought", text: "one" }, t("ls [completed]"),
    { kind: "thought", text: "two" }, t("Read file [completed]"), t("cargo test [in_progress]")];
  const turn = splitTurns(log)[0];
  assert.deepEqual(turnSections(log, turn).map(section => section.ats), [[1, 2, 3, 4, 5]]);
});


test("progress groups keep their key as thoughts and tools arrive, including partial logs", () => {
  const log = [th("First", 8000), t("ls [completed]"), th("Next", 6000), t("pwd [in_progress]")];
  assert.deepEqual(turnSections(log, splitTurns(log, 40)[0], 40), [{ progress: true, ats: [40, 41, 42, 43] }]);
  log.push(a("Found it."), th("Checking", 1000), t("cargo test [completed]"), { kind: "other", text: "Compacted" }, th("Continuing"));
  assert.deepEqual(turnSections(log, splitTurns(log, 40)[0], 40), [
    { progress: true, ats: [40, 41, 42, 43] }, { progress: false, ats: [44] },
    { progress: true, ats: [45, 46] }, { progress: false, ats: [47] }, { progress: true, ats: [48] },
  ]);
});

test("progress summary combines known thought time and tool results", () => {
  const log = [th("First", 8000), t("ls [completed]"), th("Next", 6000), t("cargo test [failed]\nexit 1")];
  assert.equal(progressSummary(log, splitTurns(log)[0]), "Thought for 14s · Ran 1 command (1 failed), searched once");
  const thoughts = [th("Reading", 12000), th("Checking")];
  assert.equal(progressSummary(thoughts, splitTurns(thoughts)[0]), "Thought for 12s");
  assert.equal(progressSummary([th("Unknown")], splitTurns([th("Unknown")])[0]), "Thought");
  const tools = [t("cargo test [completed]")];
  assert.equal(progressSummary(tools, splitTurns(tools, 20)[0], 20), "Ran 1 command");
});

test("a failed command shows its first exit code or a failure label", () => {
  const detail = (out: string) => rowParts(`cargo test [failed]\n${out}`).detail;
  assert.equal(detail("exit code 1\nrun exit 0 to quit"), "exit 1");
  assert.equal(detail("no code here"), "failed");
});
