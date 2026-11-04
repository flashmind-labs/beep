import test from "node:test";
import assert from "node:assert/strict";
import type { KeptBranches, ReviewDeps, ReviewView } from "../src/review/review.ts";
import { PageCache } from "../src/project/pagecache.ts";
import { makeReviews, reviewState, mergeBlock, staleText } from "../src/review/review.ts";

const ago = () => "12 min";

function review(over: Partial<ReviewView> = {}): ReviewView {
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
  } as ReviewView;
}

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

test("a review whose branch moved is stale and cannot merge", () => {
  const r = { branch: "login-copy", head: "3f2c1a0aaaa", findings: [], shards: [], pr: { number: 41, url: "", checks: "passing" }, merge: "clean", conflicts: [] };
  assert.equal(staleText(r, null), "");
  assert.equal(staleText(r, { head: "3f2c1a0aaaa", stale: false }), "");
  assert.equal(staleText(r, { head: "9e1b2c4dddd", stale: true }), "login-copy moved to 9e1b2c4 since this review.");
  assert.equal(staleText(r, { head: null, stale: true }), "login-copy is gone since this review.");
  assert.equal(mergeBlock(r, { head: "9e1b2c4dddd", stale: true }), "Merge is off: the branch moved since this review");
  assert.equal(mergeBlock(r, { head: "3f2c1a0aaaa", stale: false }), "Merge always asks first. It never runs on its own.");
});

test("a second load while branches first load asks the server once", async () => {
  const asked: string[] = [];
  const invoke = (async (cmd: string) => {
    asked.push(cmd);
    return cmd === "project_branches" ? { default_branch: "main", branches: [] } : [];
  }) as unknown as ReviewDeps["invoke"];
  const revs = makeReviews({ invoke, h: null!, button: null!, ago, redraw() {}, openUrl() {}, openTile() {} });
  await Promise.all([revs.load("acme"), revs.load("acme")]);
  assert.equal(asked.filter(c => c === "project_branches").length, 1);
  await revs.load("acme");
  assert.equal(asked.filter(c => c === "project_branches").length, 2, "a later load reads again");
});

test("kept branches show at once and a read replaces them in the cache", async () => {
  const asked: string[] = [];
  const invoke = (async (cmd: string) => {
    asked.push(cmd);
    return cmd === "project_branches" ? { default_branch: "main", branches: [{ name: "new" }] } : [];
  }) as unknown as ReviewDeps["invoke"];
  const cache = new PageCache("acct");
  cache.put("branches", "acme", { branches: { default_branch: "main", branches: [{ name: "old" }] }, reviews: [] });
  const kept = cache.section<KeptBranches>("branches");
  const revs = makeReviews({ invoke, h: null!, button: null!, ago, redraw() {}, openUrl() {}, openTile() {}, kept });
  assert.deepEqual(revs.branches("acme")?.branches.map(b => b.name), ["old"]);
  revs.ensure("acme");
  assert.equal(asked.length, 0, "a fresh cache is not read again");
  await revs.load("acme");
  assert.deepEqual(kept.get("acme")?.branches?.branches.map(b => b.name), ["new"]);
});
