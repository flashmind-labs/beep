import test from "node:test";
import assert from "node:assert/strict";
import { prLines, prMeta, prChip, issueLines, issueBranch, issuePrompt, prPrompt, prNumber, makeThreads, type ThreadDeps } from "../src/project/threads.ts";
import type { KeptThreads } from "../src/project/threads.ts";
import { PageCache } from "../src/project/pagecache.ts";

const ago = () => "2 h ago";

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

test("a second load while the lists first load asks the server once", async () => {
  const asked: string[] = [];
  // A server too old to keep the lists answers null, and they are read the older way.
  const invoke = (async (cmd: string) => { asked.push(cmd); return cmd === "center_project_lists" ? null : []; }) as unknown as ThreadDeps["invoke"];
  const thr = makeThreads({ invoke, h: null!, button: null!, ago, redraw() {}, openUrl() {}, openChat() {}, changed() {} });
  await Promise.all([thr.load("p1"), thr.load("p1")]);
  assert.equal(asked.filter(c => c === "center_project_issues").length, 1);
  await thr.load("p1");
  assert.equal(asked.filter(c => c === "center_project_issues").length, 2, "a later load reads again");
});

test("the lists come from the server's copy, with when it was read and why a list failed", async () => {
  const asked: { cmd: string; args: unknown }[] = [];
  const kept = {
    prs: [{ number: 41, title: "Login copy", url: "u", author: "kim", draft: false, mine: false, created_at: 1, updated_at: 1 }],
    prs_error: null,
    issues: [],
    issues_error: "git.example.lan is on your own network, which Beep's server cannot reach.",
    synced_at: 1_800_000_000_000,
  };
  const invoke = (async (cmd: string, args: unknown) => { asked.push({ cmd, args }); return cmd === "center_project_lists" ? kept : []; }) as unknown as ThreadDeps["invoke"];
  const thr = makeThreads({ invoke, h: null!, button: null!, ago, redraw() {}, openUrl() {}, openChat() {}, changed() {} });
  await thr.load("p1", true);
  assert.deepEqual(asked.find(a => a.cmd === "center_project_lists")?.args, { projectId: "p1", refresh: true });
  assert.ok(!asked.some(a => a.cmd === "center_project_issues" || a.cmd === "center_project_pull_requests"), "the host is not read again");
  assert.equal(thr.prs("p1")?.length, 1);
  assert.equal(thr.issuesError("p1"), kept.issues_error);
  assert.equal(thr.syncedAt("p1"), kept.synced_at);
});

test("a failed read keeps the last good lists in the cache", async () => {
  const invoke = (async (cmd: string) => { if (cmd === "center_project_issues") throw new Error("host down"); return cmd === "center_project_lists" ? null : []; }) as unknown as ThreadDeps["invoke"];
  const cache = new PageCache("acct");
  cache.put("threads", "p1", { issues: [{ number: 1 }], threads: [], prs: [], chats: [] });
  const kept = cache.section<KeptThreads>("threads");
  const thr = makeThreads({ invoke, h: null!, button: null!, ago, redraw() {}, openUrl() {}, openChat() {}, changed() {}, kept });
  await thr.load("p1");
  assert.deepEqual(kept.get("p1")?.issues?.map(i => i.number), [1]);
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [1]);
});
