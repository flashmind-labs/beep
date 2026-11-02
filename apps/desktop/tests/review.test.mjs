import test from "node:test";
import assert from "node:assert/strict";
import { openFindings, reviewState, mergeBlock, staleText, reviewFacts, findingPlace, shardLine, shardChip, findingsSub, readySummary, branchTile } from "../dist/review.js";

const ago = () => "12 min";

function review(over = {}) {
  return {
    id: "r1", project: "acme/widget", branch: "login-copy", base: "main", head: "3f2c1a0aaaa",
    head_at: 1, started_at: 1, machine_name: "build-box", changed_lines: 412, files: 9,
    merge: "clean", conflicts: [], pr: { number: 41, url: "https://example.com/pull/41", checks: "passing" },
    shards: [
      { area: "apps/web", files: 6, lines: 268, agent: "claude", state: "done", note: "", found: 2 },
      { area: "crates/core", files: 3, lines: 144, agent: "codex", state: "done", note: "", found: 1 },
    ],
    findings: [
      { id: 0, severity: "high", title: "Says which field", file: "apps/web/reset.ts", line: 18, why: "Leaks", verdict: "verified", dropped_why: "", dismissed: false },
      { id: 1, severity: "low", title: "Old string", file: "apps/web/Login.test.tsx", line: null, why: "Passes anyway", verdict: "unchecked", dropped_why: "", dismissed: false },
      { id: 100, severity: "low", title: "Unused import", file: "crates/core/limit.rs", line: 3, why: "", verdict: "dropped", dropped_why: "a macro uses it", dismissed: false },
    ],
    ...over,
  };
}

test("open findings leave out dismissed and dropped ones", () => {
  assert.deepEqual(openFindings(review()).map(f => f.id), [0, 1]);
  const r = review();
  r.findings[0].dismissed = true;
  assert.deepEqual(openFindings(r).map(f => f.id), [1]);
  assert.deepEqual(openFindings(null), []);
});

test("a review's state follows the mock's four states", () => {
  assert.equal(reviewState(review()), "changes");
  const running = review();
  running.shards[1].state = "verifying";
  assert.equal(reviewState(running), "running");
  assert.equal(reviewState(review({ merge: "conflicts", conflicts: ["a", "b"] })), "blocked");
  assert.equal(reviewState(review({ pr: { number: 41, url: "", checks: "failing" } })), "blocked");
  const ready = review();
  ready.findings = ready.findings.filter(f => f.verdict === "dropped");
  assert.equal(reviewState(ready), "ready");
});

test("merge says why it is off, and asks first when it is on", () => {
  assert.equal(mergeBlock(review()), "Merge is off while 2 findings are open. Fix them or dismiss them.");
  assert.equal(mergeBlock(review({ merge: "conflicts", conflicts: ["a", "b"], pr: { number: 1, url: "", checks: "failing" } })),
    "Merge is off while CI fails and 2 files conflict");
  const ready = review({ findings: [] });
  assert.equal(mergeBlock(ready), "Merge always asks first. It never runs on its own.");
  assert.equal(mergeBlock({ ...ready, pr: null }), "Merge needs a pull request for the branch");
  const running = review();
  running.shards[0].state = "reading";
  assert.equal(mergeBlock(running), "Actions open when the review is done");
});

test("the facts row names lines, shards, CI, merge and head", () => {
  const facts = reviewFacts(review(), ago);
  assert.deepEqual(facts.map(f => (f.pre || "") + (f.bold || "") + (f.code || "") + f.text), [
    "412 changed lines", "2 shards", "CI passing", "Merges cleanly", "head 3f2c1a0 · 12 min ago",
  ]);
  const blocked = reviewFacts(review({ merge: "conflicts", conflicts: ["a"], pr: null }), ago);
  assert.equal(blocked[2].text, "No pull request");
  assert.equal(blocked[3].text, "Conflicts in 1 file");
  assert.ok(blocked[3].bad);
  assert.equal(reviewFacts(review(), () => "now")[4].text, " · just now");
});

test("shards and findings read plainly", () => {
  assert.equal(findingPlace({ file: "a.rs", line: 4 }), "a.rs:4");
  assert.equal(findingPlace({ file: "a.rs", line: null }), "a.rs");
  assert.equal(shardLine(review().shards[0], "build-box"), "268 lines · 6 files · Claude Code on build-box");
  assert.deepEqual(shardChip({ state: "reading", note: "Read Login.tsx" }), { cls: "running", text: "Read Login.tsx", spin: true });
  assert.equal(shardChip({ state: "done", found: 0 }).text, "done · nothing found");
  assert.equal(shardChip({ state: "done", found: 1 }).text, "done · 1 finding");
  assert.equal(shardChip({ state: "failed" }).cls, "bad");
  assert.equal(findingsSub(review()), "2 open, ranked");
  assert.equal(findingsSub(review({ findings: [] })), "none open");
  assert.equal(readySummary(review({ findings: [review().findings[2]] })),
    "2 reviewers read all 412 lines. The verifier dropped 1 finding that was not real: a macro uses it");
});

test("the Branches tile puts branches with work first", () => {
  assert.equal(branchTile(undefined, [], ago).empty, "Loading…");
  assert.equal(branchTile(null, [], ago).empty, "The server lists no branches");
  const data = {
    default_branch: "main",
    branches: [
      { name: "main", ahead: 0, behind: 0, worktree: "/srv/project", pushed: true, committed_at: 1, subject: "Release" },
      { name: "login-copy", ahead: 3, behind: 1, worktree: null, pushed: true, committed_at: 1, subject: "Copy" },
      { name: "spike", ahead: 2, behind: 0, worktree: "/srv/project/.worktrees/spike", pushed: false, committed_at: 1, subject: "Try" },
      { name: "old", ahead: 0, behind: 9, worktree: null, pushed: true, committed_at: 1, subject: "Done" },
    ],
  };
  const tile = branchTile(data, [review()], ago);
  assert.equal(tile.count, "2 with work");
  assert.deepEqual(tile.rows.map(r => r.code), ["login-copy", "spike", "main", "old"]);
  assert.deepEqual(tile.rows[0].chip, { cls: "wait", text: "needs changes" });
  assert.equal(tile.rows[1].note, "+2 · worktree · not pushed");
  assert.equal(tile.rows[2].note, "default");
  assert.equal(tile.rows[3].note, "merged · 12 min");
});

test("a review whose branch moved is stale and cannot merge", () => {
  const r = { branch: "login-copy", head: "3f2c1a0aaaa", findings: [], shards: [], pr: { number: 41, url: "", checks: "passing" }, merge: "clean", conflicts: [] };
  assert.equal(staleText(r, null), "");
  assert.equal(staleText(r, { head: "3f2c1a0aaaa", stale: false }), "");
  assert.equal(staleText(r, { head: "9e1b2c4dddd", stale: true }), "login-copy moved to 9e1b2c4 since this review.");
  assert.equal(staleText(r, { head: null, stale: true }), "login-copy is gone since this review.");
  assert.equal(mergeBlock(r, { head: "9e1b2c4dddd", stale: true }), "Merge is off: the branch moved since this review");
  assert.equal(mergeBlock(r, { head: "3f2c1a0aaaa", stale: false }), "Merge always asks first. It never runs on its own.");
});
