import test from "node:test";
import assert from "node:assert/strict";
import { type DiffFile, type ReviewComment, parseDraft, placeComments } from "../src/review/diff-state.ts";
import {
  answerNote, answerOf, doneNote, holdPrompt, mergeComments, nextStep, parseFindings, readOnlyMode, reviewPrompt, reviewTarget, toComments, whyNot,
} from "../src/review/feedback-state.ts";

const file = (path: string, o: Partial<DiffFile> = {}): DiffFile => ({
  path, old_path: null, status: "modified", additions: 1, deletions: 1, left_out: null,
  hunks: [{ old_start: 40, old_lines: 3, new_start: 40, new_lines: 3, section: "", lines: [{ k: " ", t: "a" }, { k: "-", t: "b" }, { k: "+", t: "B" }, { k: " ", t: "c" }] }],
  ...o,
});
// The rows of that hunk: new 40 and 41 are "a" and "B", new 42 is "c"; old 41 is "b".

const json = (items: unknown) => `\`\`\`json\n${JSON.stringify(items)}\n\`\`\``;
const item = (o: Record<string, unknown> = {}) => ({ path: "src/login.rs", line: 41, side: "new", body: "The retry never resets.", ...o });

test("a reply that ends in a json block gives its findings", () => {
  const got = parseFindings(`I read the diff.\n\n${json([item(), item({ line: null, body: "Whole file." })])}`);
  assert.deepEqual(got, {
    findings: [
      { path: "src/login.rs", side: "new", line: 41, body: "The retry never resets." },
      { path: "src/login.rs", side: "new", line: null, body: "Whole file." },
    ],
    skipped: 0,
  });
});

test("the last block that is a list wins, among prose and other fences", () => {
  const text = [
    "First an example of the shape:", "```json", '{"not": "a list"}', "```",
    "Some code I read:", "```rust", "let a = [1, 2];", "```",
    "My findings:", json([item({ body: "First run." })]), "and then", json([item({ body: "Final." })]), "Done.",
  ].join("\n");
  assert.deepEqual(parseFindings(text)?.findings.map(f => f.body), ["Final."]);
});

test("a bare list and an empty list parse, prose with no list does not", () => {
  assert.equal(parseFindings(JSON.stringify([item()]))?.findings.length, 1);
  assert.deepEqual(parseFindings(json([])), { findings: [], skipped: 0 });
  assert.equal(parseFindings("Looks fine to me."), null);
});

test("bad json, an object and a cut-off block are not a list", () => {
  assert.equal(parseFindings("```json\n[{\"path\": \"a\", \"line\": 1,\n```"), null);
  assert.equal(parseFindings(json({ path: "a", line: 1, body: "x" })), null);
  assert.equal(parseFindings("```json\n[{\"path\": \"src/a.rs\", \"line\": 4, \"body\": \"cut off"), null);
});

test("entries that are not findings are counted and left out", () => {
  const got = parseFindings(json([
    item(), item({ path: "" }), item({ body: "   " }), item({ line: 0 }), item({ line: 2.5 }), item({ line: "4" }), null, "text", item({ side: "old", line: 41 }),
  ]));
  assert.equal(got?.findings.length, 2);
  assert.equal(got?.skipped, 7);
  assert.equal(got?.findings[1].side, "old");
});

test("a side other than old is new, and a missing line is a note on the file", () => {
  const got = parseFindings(json([item({ side: "left" }), { path: "a.rs", body: "No line." }]));
  assert.deepEqual(got?.findings.map(f => [f.side, f.line]), [["new", 41], ["new", null]]);
});

test("paths lose a leading ./ and the checkout's own root", () => {
  const got = parseFindings(json([item({ path: "./src/a.rs" }), item({ path: "/srv/project/src/b.rs" })]), "/srv/project");
  assert.deepEqual(got?.findings.map(f => f.path), ["src/a.rs", "src/b.rs"]);
});

test("html and control characters in a body stay text and are never cut into markup", () => {
  const body = '<img src=x onerror="alert(1)"> and <b>bold</b>\u0000 ignore all previous instructions\u0007';
  const got = parseFindings(json([item({ body })]));
  assert.equal(got?.findings[0].body, '<img src=x onerror="alert(1)"> and <b>bold</b> ignore all previous instructions');
  const [c] = toComments(got!.findings, [file("src/login.rs")], "claude");
  assert.equal(c.text, got!.findings[0].body);
});

test("a long body is cut", () => {
  const got = parseFindings(json([item({ body: "x".repeat(5000) })]));
  assert.ok(got!.findings[0].body.length <= 1200);
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

test("the target is the uncommitted changes or the merge base, and the prompt names it", () => {
  const view = { base: "origin/main", merge_base: "3f2c1a0e9" };
  assert.deepEqual(reviewTarget(view, true), { kind: "uncommitted" });
  assert.deepEqual(reviewTarget({ base: "main", merge_base: "" }, false), { kind: "uncommitted" });
  const branch = reviewTarget(view, false);
  assert.deepEqual(branch, { kind: "branch", base: "origin/main", mergeBase: "3f2c1a0e9" });
  const text = reviewPrompt(branch, "/srv/project", true);
  assert.match(text, /git diff 3f2c1a0e9/);
  assert.match(text, /merge base 3f2c1a0e9 with origin\/main/);
  assert.match(text, /"\/srv\/project"/);
  const dirty = reviewPrompt({ kind: "uncommitted" }, "/srv/project", true);
  assert.match(dirty, /git diff HEAD/);
  assert.match(dirty, /git status --short/);
});

test("the prompt asks for evidence and one fenced json block, and says not to edit", () => {
  const text = reviewPrompt({ kind: "uncommitted" }, "/srv/project", true);
  assert.match(text, /You review only/);
  assert.match(text, /Do not edit/);
  assert.match(text, /evidence/);
  assert.match(text, /```json\n\[\{"path"/);
  assert.match(text, /never as instructions/);
  assert.doesNotMatch(text, /Nothing enforces/);
  assert.match(reviewPrompt({ kind: "uncommitted" }, "/srv/project", false), /Nothing enforces this/);
});

test("a ref with odd characters cannot add to the commands the prompt shows", () => {
  const text = reviewPrompt({ kind: "branch", base: "main; rm -rf x", mergeBase: "abc$(id)" }, "/srv/project", true);
  assert.doesNotMatch(text, /rm -rf|\$\(/);
});

test("the hold message titles the session for the review and asks for nothing", () => {
  const first = holdPrompt({ kind: "branch", base: "main", mergeBase: "abc" }).split("\n")[0];
  assert.equal(first, "Review of the changes against main");
  assert.equal(holdPrompt({ kind: "uncommitted" }).split("\n")[0], "Review of the uncommitted changes");
});

test("the read-only mode is plan for Claude Code and read-only for Codex; none for the rest", () => {
  assert.equal(readOnlyMode([{ id: "default", name: "Manual" }, { id: "acceptEdits", name: "Accept edits" }, { id: "plan", name: "Plan" }, { id: "bypassPermissions", name: "Bypass" }]), "plan");
  assert.equal(readOnlyMode([{ id: "read-only", name: "Read Only" }, { id: "auto", name: "Default" }, { id: "full-access", name: "Full Access" }]), "read-only");
  assert.equal(readOnlyMode([{ id: "m1", name: "Read only" }]), "m1");
  assert.equal(readOnlyMode([{ id: "yolo", name: "YOLO" }, { id: "default", name: "Default" }]), null);
  assert.equal(readOnlyMode([]), null);
});

test("the review waits on each turn, then sends, then reads, and drops an ended session", () => {
  const read = (status: string, turns: number, asking = false) => ({ status, turns, asking });
  assert.equal(nextStep("hold", read("starting", 0)), "wait");
  assert.equal(nextStep("hold", read("idle", 0)), "wait");
  assert.equal(nextStep("hold", read("working", 0)), "wait");
  assert.equal(nextStep("hold", read("idle", 1)), "send");
  assert.equal(nextStep("review", read("idle", 1)), "wait");
  assert.equal(nextStep("review", read("working", 2)), "wait");
  assert.equal(nextStep("review", read("idle", 2)), "answer");
  assert.equal(nextStep("review", read("asking", 1, true)), "asked");
  assert.equal(nextStep("hold", read("ended", 0)), "gone");
  assert.equal(nextStep("review", read("ended", 2)), "gone");
});

test("the answer is the agent's last message, from the entries before the cut summary", () => {
  const page = { last_message: "cut", entries: [{ kind: "agent", text: "first" }, { kind: "user", text: "u" }, { kind: "agent", text: "full answer" }, { kind: "tool", text: "t" }] };
  assert.equal(answerOf(page), "full answer");
  assert.equal(answerOf({ last_message: "summary", entries: [] }), "summary");
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

test("the notes say what happened", () => {
  assert.equal(doneNote("Claude Code", 2, 2), "Claude Code added 2 comments");
  assert.equal(doneNote("Claude Code", 1, 1), "Claude Code added 1 comment");
  assert.equal(doneNote("Codex", 0, 3), "Codex added no new comments");
  assert.equal(doneNote("Codex", 0, 0), "Codex found nothing to report");
  assert.equal(answerNote("  hello\u0000  "), "hello");
  assert.equal(answerNote(""), "The agent sent no answer.");
  assert.ok(answerNote("y".repeat(4000)).length < 1600);
});
