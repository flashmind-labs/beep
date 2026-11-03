import test from "node:test";
import assert from "node:assert/strict";
import { threadCode, issueTile, threadTile, issueMeta, threadMeta, prLines, prMeta, prChip, issueLines, issueCount, firstRows, issueBranch, issuePrompt, prPrompt, prNumber, SHOWN, makeThreads, type ThreadDeps } from "../src/threads.ts";
import type { KeptThreads } from "../src/threads.ts";
import { PageCache } from "../src/pagecache.ts";

const ago = () => "2 h ago";

test("threads are named by kind and number", () => {
  assert.equal(threadCode({ kind: "pr", number: 41 }), "PR #41");
  assert.equal(threadCode({ kind: "issue", number: 31 }), "#31");
});

test("the Issues tile counts the untracked issues", () => {
  assert.equal(issueTile(undefined).empty, "Loading…");
  assert.equal(issueTile(null).rows.length, 0);
  const tile = issueTile([
    { number: 52, title: "Crash on rotate", labels: ["bug"], comments: 0, url: "https://github.com/acme/widget/issues/52" },
    { number: 48, title: "Onboarding in Spanish", labels: [], comments: 4, url: "https://github.com/acme/widget/issues/48" },
  ]);
  assert.equal(tile.count, "2 untracked");
  assert.deepEqual(tile.rows.map(r => [r.code, r.note]), [["#52", "bug"], ["#48", "4 comments"]]);
  assert.equal(issueTile([]).empty, "No untracked issues");
});

test("the Pull requests tile puts what needs a reply first", () => {
  const fallback = { count: "1 open", rows: [{ text: "from a task" }], empty: "No pull requests from tasks yet" };
  assert.equal(threadTile(undefined, fallback), fallback);
  assert.equal(threadTile(null, fallback), fallback);
  const tile = threadTile([
    { kind: "pr", number: 42, title: "Spanish strings", needs_reply: false },
    { kind: "issue", number: 31, title: "Upload fails", needs_reply: true },
    { kind: "pr", number: 41, title: "Login copy", needs_reply: true },
    { kind: "issue", number: 30, title: "Old question", needs_reply: false },
  ], fallback);
  assert.equal(tile.count, "2 need reply");
  assert.deepEqual(tile.rows.map(r => [r.code, r.chip?.text || ""]), [["#31", "reply"], ["#41", "reply"], ["#42", ""]]);
  assert.equal(threadTile([{ kind: "pr", number: 1, title: "x", needs_reply: false }], fallback).count, "1 open");
});

test("the Pull requests tile lists every open PR, replies first", () => {
  const fallback = { count: "", rows: [], empty: "No pull requests from tasks yet" };
  const pr = (number: number, author: string, created_at: number, extra: { mine?: boolean; draft?: boolean } = {}) =>
    ({ number, title: `PR ${number}`, url: `https://github.com/acme/widget/pull/${number}`, author, created_at, mine: false, draft: false, ...extra });
  const prs = [
    pr(7, "dependabot[bot]", 100),
    pr(9, "sam", 300),
    pr(8, "alice", 200, { mine: true, draft: true }),
    pr(5, "alice", 50, { mine: true }),
  ];
  const threads = [
    { kind: "pr", number: 5, title: "PR 5", needs_reply: true },
    { kind: "issue", number: 31, title: "Upload fails", needs_reply: true },
    { kind: "pr", number: 8, title: "PR 8", needs_reply: false },
  ];
  const tile = threadTile(threads, fallback, prs);
  assert.equal(tile.count, "4 open");
  assert.deepEqual(tile.rows.map(r => [r.code, r.chip?.text || "", r.note]), [
    ["#5", "reply", ""],
    ["#31", "reply", ""],
    ["#9", "", "sam"],
    ["#8", "", "draft"],
    ["#7", "", "dependabot[bot]"],
  ]);
  // Others' PRs alone still show, before the user's threads load.
  const bots = threadTile(undefined, fallback, [pr(7, "dependabot[bot]", 100)]);
  assert.equal(bots.count, "1 open");
  assert.deepEqual(bots.rows.map(r => r.note), ["dependabot[bot]"]);
  assert.equal(threadTile([], fallback, []).empty, "No open pull requests");
  // A server too old to list them keeps the user's own threads.
  assert.equal(threadTile([{ kind: "pr", number: 1, title: "x", needs_reply: false }], fallback, null).count, "1 open");
  assert.equal(threadTile(undefined, fallback, null), fallback);
});

test("an issue's line says who opened it and how busy it is", () => {
  assert.equal(issueMeta({ author: "sam", opened_at: 1, labels: ["bug"], comments: 1 }, ago), "opened by sam · 2 h ago · bug · 1 comment");
  assert.equal(threadMeta({ last_other: "sam: Logs?", updated_at: 1, needs_reply: true }, ago), "sam: Logs? · 2 h ago");
  assert.equal(threadMeta({ last_other: "", updated_at: 0, needs_reply: false }, ago), "your last comment is the newest");
});

const pr = (number: number, o: Partial<{ author: string; mine: boolean; draft: boolean; created_at: number }> = {}) => ({
  number,
  title: `PR ${number}`,
  url: `https://example.com/acme/widget/pull/${number}`,
  author: o.author ?? "kim",
  draft: o.draft ?? false,
  mine: o.mine ?? false,
  created_at: o.created_at ?? number,
  updated_at: 0,
});
const task = (id: string, o: Partial<{ title: string; state: string; pr_url: string | null; issue_number: number | null; issue_url: string | null; branch: string | null }> = {}) => ({
  id,
  title: o.title ?? `Task ${id}`,
  state: o.state ?? "running",
  pr_url: o.pr_url ?? null,
  issue_number: o.issue_number ?? null,
  issue_url: o.issue_url ?? null,
  branch: o.branch ?? null,
});
const review = (branch: string, number: number | null, checks: string, open = 0) => ({
  branch,
  merge: "clean",
  pr: number == null ? null : { number, url: "", checks },
  shards: [{ state: "done" }],
  findings: Array.from({ length: open }, () => ({ verdict: "confirmed", dismissed: false })),
});

test("every open pull request shows its task, review and reply, those waiting first", () => {
  const prs = [pr(40, { created_at: 1 }), pr(41, { mine: true, created_at: 2 }), pr(42, { draft: true, created_at: 3 })];
  const tasks = [task("t1", { title: "Login copy", state: "review", pr_url: "https://example.com/acme/widget/pull/41", branch: "login-copy" })];
  const threads = [{ kind: "pr", number: 41, needs_reply: true, repo: "acme/widget" }];
  const lines = prLines(prs, tasks, threads, [review("login-copy", null, "none"), review("bump", 40, "failing")] as never);
  assert.deepEqual(lines.map(l => l.pr.number), [41, 42, 40]);
  const [mine, draft, other] = lines;
  assert.equal(mine.task?.id, "t1");
  assert.equal(mine.branch, "login-copy");
  assert.equal(mine.review, "ready");
  assert.deepEqual(prChip(mine), { cls: "blocked", text: "needs reply" });
  assert.deepEqual(prMeta(mine, ago), ["you", "2 h ago", "ready", "task: Login copy"]);
  assert.deepEqual(prChip(draft), { cls: "", text: "draft" });
  assert.deepEqual(prMeta(draft, ago), ["kim", "2 h ago"]);
  assert.equal(other.branch, "bump", "the review names the branch");
  assert.equal(other.review, "blocked");
  assert.deepEqual(prMeta(other, ago), ["kim", "2 h ago", "blocked · CI failing"]);
  assert.equal(prNumber("https://example.com/x/pulls/7"), 7);
  assert.equal(prNumber(null), null);
});

test("the issues are the untracked ones, then the tasks' still going", () => {
  const issues = [{ number: 52, title: "Crash", labels: [], author: "sam", opened_at: 1, comments: 0, url: "u52" }];
  const tasks = [
    task("a", { issue_number: 44, issue_url: "u44" }),
    task("b", { issue_number: 37, state: "done" }),
    task("c", { issue_number: 52 }),
    task("d"),
  ];
  const lines = issueLines(issues, tasks);
  assert.deepEqual(lines.map(l => [l.number, l.task?.id ?? null]), [[52, null], [44, "a"]]);
  assert.equal(lines[1].url, "u44");
  assert.equal(issueCount(lines), "2 open · 1 not tracked");
  assert.equal(issueCount(lines.slice(0, 1)), "1 open, none tracked");
  assert.equal(issueCount([]), "");
});

test("a section shows its first rows until Show all", () => {
  const rows = Array.from({ length: SHOWN + 3 }, (_, i) => i);
  assert.deepEqual(firstRows(rows, false), { rows: rows.slice(0, SHOWN), hidden: 3 });
  assert.equal(firstRows(rows, true).rows.length, SHOWN + 3);
  assert.equal(firstRows([1, 2], false).hidden, 0);
});

test("Work on it and Continue fill the start form", () => {
  assert.equal(issueBranch(52, "App crashes when the phone rotates during a call"), "issue-52-app-crashes-when-the-phone");
  assert.equal(issueBranch(7, "Fix: the *login* bug!"), "issue-7-fix-the-login-bug");
  assert.equal(issueBranch(8, "????"), "issue-8");
  assert.equal(issueBranch(9, "Supercalifragilisticexpialidocious-and-more"), "issue-9-supercalifragilisticexpialidoc");
  assert.equal(issuePrompt({ number: 52, title: "Crash", url: "https://example.com/i/52" }),
    "Work on issue #52: Crash (https://example.com/i/52). Read the issue and its comments first, then propose and implement a fix.");
  assert.equal(prPrompt({ number: 41, title: "Login copy", url: "https://example.com/p/41" }),
    "Continue PR #41: Login copy (https://example.com/p/41). Read the review comments and failing checks first.");
});

test("the Issues tile says why the host could not be read", () => {
  const why = "git.example.lan is on your own network, which Beep's server cannot reach.";
  assert.equal(issueTile([], why).empty, why);
  assert.equal(issueTile([{ number: 1, title: "x", url: "u", labels: [], comments: 0 }], why).rows.length, 1);
});

test("a second load while the lists first load asks the server once", async () => {
  const asked: string[] = [];
  const invoke = (async (cmd: string) => { asked.push(cmd); return []; }) as unknown as ThreadDeps["invoke"];
  const thr = makeThreads({ invoke, h: null!, button: null!, ago, redraw() {}, openUrl() {}, openChat() {}, changed() {} });
  await Promise.all([thr.load("p1"), thr.load("p1")]);
  assert.equal(asked.filter(c => c === "center_project_issues").length, 1);
  await thr.load("p1");
  assert.equal(asked.filter(c => c === "center_project_issues").length, 2, "a later load reads again");
});

test("kept lists show at once, a fresh cache is not read again, and a read is kept", async () => {
  const asked: string[] = [];
  const invoke = (async (cmd: string) => { asked.push(cmd); return cmd === "center_project_issues" ? [{ number: 9 }] : []; }) as unknown as ThreadDeps["invoke"];
  const cache = new PageCache("acct");
  cache.put("threads", "p1", { issues: [{ number: 1 }], threads: [], prs: [], chats: [] });
  const kept = cache.section<KeptThreads>("threads");
  const thr = makeThreads({ invoke, h: null!, button: null!, ago, redraw() {}, openUrl() {}, openChat() {}, changed() {}, kept });
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [1]);
  thr.ensure("p1");
  assert.equal(asked.length, 0, "a fresh cache is not read again");
  // A project never read is.
  thr.ensure("p2");
  assert.ok(asked.includes("center_project_issues"));
  await thr.load("p1");
  assert.deepEqual(kept.get("p1")?.issues?.map(i => i.number), [9]);
});

test("a failed read keeps the last good lists in the cache", async () => {
  const invoke = (async (cmd: string) => { if (cmd === "center_project_issues") throw new Error("host down"); return []; }) as unknown as ThreadDeps["invoke"];
  const cache = new PageCache("acct");
  cache.put("threads", "p1", { issues: [{ number: 1 }], threads: [], prs: [], chats: [] });
  const kept = cache.section<KeptThreads>("threads");
  const thr = makeThreads({ invoke, h: null!, button: null!, ago, redraw() {}, openUrl() {}, openChat() {}, changed() {}, kept });
  await thr.load("p1");
  assert.deepEqual(kept.get("p1")?.issues?.map(i => i.number), [1]);
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [1]);
});
