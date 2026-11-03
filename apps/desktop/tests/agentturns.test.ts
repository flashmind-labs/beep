import test from "node:test";
import assert from "node:assert/strict";
import { beepCall, beepLine, clock, commandDetail, commandOf, duration, folded, heading, keptSteps, keptThoughts, liveThought, onlyNotes, span, splitTurns, stepLabel, stepOpen, summary, thoughtLabel, thoughtTail, toolKind } from "../src/agentturns.ts";

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

test("a turn that ends on a tool has no answer", () => {
  assert.deepEqual(splitTurns([u("go"), a("Running it."), t("make [in_progress]")])[0].answer, []);
});

test("only a finished turn with steps folds, until it is opened", () => {
  const turn = { key: 0, user: 0, steps: [1], answer: [2] };
  assert.equal(folded(turn, false, false), true);
  assert.equal(folded(turn, false, true), false);
  assert.equal(folded(turn, true, false), false);
  assert.equal(folded({ ...turn, steps: [] }, false, false), false);
});

test("tools are counted by kind", () => {
  assert.equal(toolKind("Bash: cargo test (failed)"), "command");
  assert.equal(toolKind("cargo test -p widget [completed]"), "command");
  assert.equal(toolKind("Edit `src/main.rs` [completed]"), "edit");
  assert.equal(toolKind("Write: /srv/project/notes.md"), "edit");
  assert.equal(toolKind("Read: /srv/project/a.rs"), "read");
  assert.equal(toolKind("Grep: fn main"), "search");
  assert.equal(toolKind("WebFetch: https://example.com"), "web");
  assert.equal(toolKind("Web search rust async [completed]"), "web");
  assert.equal(toolKind("TodoWrite: plan"), "other");
});

test("the folded line counts each kind, with plurals", () => {
  const log = [u("go"), t("cargo test [failed]"), t("Edit `src/login.rs` [completed]"), t("Edit `tests/login.rs` [completed]"), a("Done.")];
  assert.equal(summary(log, splitTurns(log)[0]), "Ran 1 command (1 failed), edited 2 files");
  const more = [u("go"), t("Bash: ls"), t("Bash: make"), t("Read: a.rs"), t("Read: a.rs"), t("Grep: x"), t("WebFetch: https://example.com"), t("Task: x"), t("Task: y")];
  assert.equal(summary(more, splitTurns(more)[0]), "Ran 2 commands, read 1 file, performed 4 other actions");
  const twice = [u("go"), t("Grep: x"), t("Glob: *.rs"), t("WebSearch: y"), t("WebSearch: z")];
  assert.equal(summary(twice, splitTurns(twice)[0]), "Searched 2 times, made 2 web requests");
  const one = [u("go"), t("Bash: ls"), t("Edit: a.rs"), t("Grep: x")];
  assert.equal(summary(one, splitTurns(one)[0]), "Ran 1 command, edited 1 file, performed 1 other action");
  const talk = [u("go"), a("Thinking."), { kind: "question", text: "Run it?" }, a("Done.")];
  assert.equal(summary(talk, splitTurns(talk)[0]), "2 steps");
});

test("durations read as seconds, minutes and hours", () => {
  assert.equal(duration(0), "0s");
  assert.equal(duration(48_400), "48s");
  assert.equal(duration(72_000), "1m 12s");
  assert.equal(duration(60_000), "1m 0s");
  assert.equal(duration(2 * 3600_000 + 5 * 60_000 + 30_000), "2h 5m");
  assert.equal(duration(-5), "0s");
});

test("the heading says how the turn stands", () => {
  assert.equal(heading("done", 72_000), "Worked for 1m 12s");
  assert.equal(heading("working", 48_000), "Working for 48s");
  assert.equal(heading("waiting", 48_000), "Waiting for you · 48s");
  assert.equal(heading("done", null), "Worked");
  assert.equal(heading("waiting", null), "Waiting for you");
});

test("a time of day is HH:MM", () => {
  assert.equal(clock(new Date(2026, 9, 7, 9, 5).getTime()), "09:05");
  assert.equal(clock(new Date(2026, 9, 7, 17, 14).getTime()), "17:14");
});

test("a turn's span comes from the session or from what the window saw", () => {
  const base = { first: false, last: false, running: false, sessionStart: 1000, lastActivity: 9000 };
  assert.deepEqual(span({ ...base, first: true, last: true }), { start: 1000, end: 9000 });
  assert.deepEqual(span({ ...base, first: true, running: true, last: true }), { start: 1000, end: undefined });
  assert.deepEqual(span({ ...base, seenStart: 5000, seenEnd: 7000 }), { start: 5000, end: 7000 });
  assert.deepEqual(span(base), { start: undefined, end: undefined });
  assert.deepEqual(span({ ...base, last: true, seenStart: 9500 }), { start: 9500, end: undefined });
});

test("a step reads as one short line", () => {
  assert.equal(stepLabel("cargo test -p widget [completed]"), "Ran cargo test -p widget");
  assert.equal(stepLabel("cargo test [in_progress]", true), "Running cargo test");
  assert.equal(stepLabel("Read src/main.rs [completed]"), "Read src/main.rs");
  assert.equal(stepLabel("Bash: make"), "Ran make");
  assert.equal(stepLabel("Edit: a.rs"), "Edited a.rs");
  assert.equal(stepLabel("Grep: fn main"), "Searched fn main");
  assert.equal(stepLabel("TodoWrite: plan"), "TodoWrite: plan");
});

test("notes and subagents stay out of the fold; a tool's pictures fold, counted", () => {
  const log = [u("go"), t("ls [completed]"), t("cargo test [failed]\nexit code: 101"), { kind: "other", text: "Switched to deep from the next turn" },
    t("Explore [completed]\n[steps] 1\n[step completed] Read a.rs\nFound it"), t("Screenshot [completed]\n[image: image-0123456789abcdef]"), a("Done.")];
  assert.deepEqual(keptSteps(log, splitTurns(log)[0]), [3, 4]);
  assert.equal(summary(log, splitTurns(log)[0]), "Ran 3 commands (1 failed), searched once, viewed 1 image");
});

test("notes before the first message are not a turn of work", () => {
  const log = [{ kind: "other", text: "Compacted" }, u("go"), a("ok")];
  const turns = splitTurns(log);
  assert.equal(onlyNotes(log, turns[0]), true);
  assert.equal(onlyNotes(log, turns[1]), false);
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
  // Before its arguments come, no machine is named.
  assert.equal(beepCall("Beep: Run on a machine")?.machine, "");
  assert.equal(beepCall("Run cargo test"), null);
  assert.equal(beepCall("mcp__beep__run_on"), null);
  assert.equal(beepLine("Beep: Run on build-box: make"), "Ran on build-box: make");
  assert.equal(beepLine("Beep: Send 2 files to build-box", true), "Sending 2 files to build-box");
  assert.equal(stepLabel("Beep: Run on build-box: make [completed]"), "Ran on build-box: make");
  assert.equal(toolKind("Beep: Run on build-box: make [completed]"), "beep");
});

test("the summary counts Beep calls on their own", () => {
  const log = [u("go"), t("Beep: List machines [completed]"), t("Beep: Run on build-box: make [completed]"), a("done")];
  assert.equal(summary(log, splitTurns(log)[0]), "Used Beep 2 times");
});

test("only the newest step of a running turn starts open when it failed", () => {
  assert.equal(stepOpen(true, true, false), true);
  assert.equal(stepOpen(true, true, true), false);
  assert.equal(stepOpen(true, false, false), false);
  assert.equal(stepOpen(true, false, true), true);
  assert.equal(stepOpen(false, true, false), false);
});

const th = (text: string, ms: number | null = null) => ({ kind: "thought", text, ms });

test("a thought reads how long it took, or just Thought when that is not known", () => {
  assert.equal(thoughtLabel(12_400), "Thought for 12s");
  assert.equal(thoughtLabel(80_000), "Thought for 1m 20s");
  assert.equal(thoughtLabel(300), "Thought for 1s", "never 0s");
  assert.equal(thoughtLabel(null), "Thought");
  assert.equal(thoughtLabel(undefined), "Thought");
});

test("the preview beside Thinking is the thought's newest line, plain", () => {
  assert.equal(thoughtTail("**Reading**\n\nThe test expects `Wrong password`.\n"), "The test expects Wrong password.");
  assert.equal(thoughtTail("## Plan\n- check the *fixtures*"), "check the fixtures");
  assert.equal(thoughtTail("  \n"), "");
});

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
