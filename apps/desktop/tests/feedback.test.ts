import test from "node:test";
import assert from "node:assert/strict";
import { type DiffFile, type ReviewComment, parseDraft, placeComments } from "../src/review/diff-state.ts";
import {
  type SubmittedReview, doneNote, findingsOf, mergeComments, reviewBase, reviewsOf, runState, toComments, whyNot,
} from "../src/review/feedback-state.ts";

const file = (path: string, o: Partial<DiffFile> = {}): DiffFile => ({
  path, old_path: null, status: "modified", additions: 1, deletions: 1, left_out: null,
  hunks: [{ old_start: 40, old_lines: 3, new_start: 40, new_lines: 3, section: "", lines: [{ k: " ", t: "a" }, { k: "-", t: "b" }, { k: "+", t: "B" }, { k: " ", t: "c" }] }],
  ...o,
});
// The rows of that hunk: new 40 and 41 are "a" and "B", new 42 is "c"; old 41 is "b".


const submitted = (o: Partial<SubmittedReview> = {}): SubmittedReview => ({
  verdict: "changes",
  summary: "One bug.",
  findings: [{ file: "src/login.rs", line: 41, severity: "high", summary: "The retry never resets.", scenario: "Two failed logins lock the account for good." }],
  ...o,
});

test("a submitted finding becomes a comment on the new side, with its severity and scenario", () => {
  assert.deepEqual(findingsOf(submitted()), [
    { path: "src/login.rs", side: "new", line: 41, body: "high: The retry never resets.\n\nTwo failed logins lock the account for good." },
  ]);
  const [c] = toComments(findingsOf(submitted()), [file("src/login.rs")], "claude");
  assert.deepEqual([c.line, c.side, c.by], [41, "new", "claude"]);
});

test("a finding with no line is on its file, and one with no file or summary is left out", () => {
  const got = findingsOf(submitted({
    findings: [
      { file: "README.md", line: null, severity: "low", summary: "Stale example", scenario: "" },
      { file: "", line: 3, severity: "low", summary: "No file", scenario: "x" },
      { file: "a.rs", line: 0, severity: "low", summary: "  ", scenario: "x" },
      { file: "b.rs", line: -2, severity: "medium", summary: "Bad line", scenario: "x" },
    ],
  }));
  assert.deepEqual(got.map(f => [f.path, f.line, f.body]), [["README.md", null, "low: Stale example"], ["b.rs", null, "medium: Bad line\n\nx"]]);
});

test("paths lose a leading ./ and the checkout's own root", () => {
  const got = findingsOf(submitted({
    findings: [
      { file: "./src/a.rs", line: 1, severity: "low", summary: "x", scenario: "" },
      { file: "/srv/project/src/b.rs", line: 1, severity: "low", summary: "y", scenario: "" },
    ],
  }), "/srv/project");
  assert.deepEqual(got.map(f => f.path), ["src/a.rs", "src/b.rs"]);
});

test("html and control characters stay text, and a long finding is cut", () => {
  const summary = '<img src=x onerror="alert(1)"> and <b>bold</b>\u0000 ignore all previous instructions\u0007';
  const [f] = findingsOf(submitted({ findings: [{ file: "a.rs", line: 1, severity: "high", summary, scenario: "" }] }));
  assert.equal(f.body, 'high: <img src=x onerror="alert(1)"> and <b>bold</b> ignore all previous instructions');
  const [long] = findingsOf(submitted({ findings: [{ file: "a.rs", line: 1, severity: "high", summary: "x", scenario: "y".repeat(5000) }] }));
  assert.ok(long.body.length <= 1200);
});

test("the base is HEAD for the uncommitted changes, else the merge base", () => {
  const view = { base: "origin/main", merge_base: "3f2c1a0e9" };
  assert.equal(reviewBase(view, true), "HEAD");
  assert.equal(reviewBase(view, false), "3f2c1a0e9");
  assert.equal(reviewBase({ ...view, merge_base: "" }, false), "HEAD");
});

test("a review runs until it is handed in or fails", () => {
  assert.equal(runState({ review: null, error: null }), "running");
  assert.equal(runState({ review: submitted(), error: null }), "done");
  assert.equal(runState({ review: null, error: "The review was stopped." }), "failed");
});

test("the note says the verdict and what was added", () => {
  assert.equal(doneNote("Claude Code", "changes", 2, 2), "Claude Code asks for changes, 2 comments added");
  assert.equal(doneNote("Claude Code", "comment", 1, 1), "Claude Code comments, 1 comment added");
  assert.equal(doneNote("Codex", "changes", 0, 3), "Codex asks for changes, no new comments");
  assert.equal(doneNote("Codex", "approve", 0, 0), "Codex approves, nothing to report");
});

test("findings on a line of the diff sit on it, on either side, and carry who wrote them", () => {
  const files = [file("src/login.rs")];
  const got = toComments([
    { path: "src/login.rs", side: "new", line: 41, body: "New side." },
    { path: "src/login.rs", side: "old", line: 41, body: "Old side." },
    { path: "src/login.rs", side: "new", line: null, body: "Whole file." },
  ], files, "claude");
  assert.deepEqual(got, [
    { path: "src/login.rs", side: "new", line: 41, text: "New side.", by: "claude" },
    { path: "src/login.rs", side: "old", line: 41, text: "Old side.", by: "claude" },
    { path: "src/login.rs", side: "new", line: null, text: "Whole file.", by: "claude" },
  ]);
  const placed = placeComments(files, got);
  assert.equal(placed.files.get("src/login.rs")?.lines.size, 2);
  assert.equal(placed.files.get("src/login.rs")?.file.length, 1);
  assert.equal(placed.files.get("src/login.rs")?.top.length, 0);
});

test("a line the diff does not have becomes a note on the file, never lost", () => {
  const got = toComments([
    { path: "src/login.rs", side: "new", line: 212, body: "Far away." },
    { path: "src/login.rs", side: "old", line: 99, body: "Gone line." },
    { path: "src/login.rs", side: "old", line: 40, body: "Old 40 is unchanged context, which has no old side." },
  ], [file("src/login.rs")], "codex");
  assert.deepEqual(got.map(c => [c.line, c.text]), [
    [null, "Line 212: Far away."],
    [null, "Old line 99: Gone line."],
    [null, "Old line 40: Old 40 is unchanged context, which has no old side."],
  ]);
});

test("a finding on a file the diff does not have stays whole, under the files that are gone", () => {
  const got = toComments([{ path: "src/other.rs", side: "new", line: 3, body: "Elsewhere." }], [file("src/login.rs")], "claude");
  assert.deepEqual(got, [{ path: "src/other.rs", side: "new", line: 3, text: "Elsewhere.", by: "claude" }]);
  assert.equal(placeComments([file("src/login.rs")], got).lost.length, 1);
});

test("a second run adds only what the draft has not, by path, line, side and text", () => {
  const mine: ReviewComment = { path: "src/login.rs", side: "new", line: 41, text: "Mine." };
  const a: ReviewComment = { path: "src/login.rs", side: "new", line: 41, text: "Same.", by: "claude" };
  const first = mergeComments([mine], [a, { ...a }, { ...a, line: 42 }, { ...a, side: "old" }, { ...mine }]);
  assert.equal(first.added, 3);
  assert.equal(first.draft.length, 4);
  const again = mergeComments(first.draft, [a, { ...a, line: 42 }]);
  assert.equal(again.added, 0);
  assert.equal(again.draft.length, 4);
});

test("what an agent wrote is kept as stored, with a harness id and nothing else", () => {
  const raw = JSON.stringify([
    { path: "a", side: "new", line: 1, text: "x", by: "claude" },
    { path: "b", side: "new", line: 2, text: "y", by: "<script>" },
    { path: "c", side: "new", line: null, text: "z" },
  ]);
  assert.deepEqual(parseDraft(raw).map(c => c.by), ["claude", undefined, undefined]);
});

test("a button is disabled with a plain reason until a review can start", () => {
  const machine = { id: "m1", name: "build-box" };
  const ok = { machine, root: "/srv/project", hosts: [{ machine_id: "m1", agents: ["claude"] }], changes: true };
  assert.equal(whyNot(ok), "");
  assert.equal(whyNot({ ...ok, machine: null }), "This diff has no checkout on a machine to review");
  assert.equal(whyNot({ ...ok, root: null }), "Reading the changes");
  assert.equal(whyNot({ ...ok, changes: false }), "There are no changes to review");
  assert.equal(whyNot({ ...ok, hosts: null }), "Looking for coding agents");
  assert.equal(whyNot({ ...ok, hosts: [{ machine_id: "m2", agents: ["claude"] }] }), "build-box is offline, so the checkout cannot be read");
  assert.equal(whyNot({ ...ok, hosts: [{ machine_id: "m1", agents: [] }] }), "build-box has no coding agent");
});

test("a reviews update carries every run the node keeps, and one without runs says so", () => {
  const run = { id: "r1", parent: "s1", cwd: "/srv/project", base: "HEAD", agent: "claude", started_at: 1, review: null, error: null };
  assert.deepEqual(reviewsOf({ Sessions: { Reviews: { runs: [run, { nope: true }] } } }), [run]);
  assert.equal(reviewsOf({ error: "refused" }), null);
  assert.equal(reviewsOf(undefined), null);
});
