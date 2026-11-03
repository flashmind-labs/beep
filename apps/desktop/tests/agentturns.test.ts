import test from "node:test";
import assert from "node:assert/strict";
import { clock, duration, folded, heading, keptSteps, onlyNotes, span, splitTurns, stepLabel, summary, toolKind } from "../src/agentturns.ts";

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
  assert.equal(summary(log, splitTurns(log)[0]), "Ran 1 command, edited 2 files");
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

test("notes, failures and subagents stay out of the fold; a tool's pictures fold, counted", () => {
  const log = [u("go"), t("ls [completed]"), t("cargo test [failed]\nexit code: 101"), { kind: "other", text: "Switched to deep from the next turn" },
    t("Explore [completed]\n[steps] 1\n[step completed] Read a.rs\nFound it"), t("Screenshot [completed]\n[image: image-0123456789abcdef]"), a("Done.")];
  assert.deepEqual(keptSteps(log, splitTurns(log)[0]), [2, 3, 4]);
  assert.equal(summary(log, splitTurns(log)[0]), "Ran 3 commands, searched once, viewed 1 image");
});

test("notes before the first message are not a turn of work", () => {
  const log = [{ kind: "other", text: "Compacted" }, u("go"), a("ok")];
  const turns = splitTurns(log);
  assert.equal(onlyNotes(log, turns[0]), true);
  assert.equal(onlyNotes(log, turns[1]), false);
});
