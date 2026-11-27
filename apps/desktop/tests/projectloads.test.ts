import test from "node:test";
import assert from "node:assert/strict";
import { PageCache, SWITCH_MS } from "../src/ui/pagecache.ts";
import { makeThreads, type KeptThreads, type ThreadDeps } from "../src/parts/project/threads.ts";

/// A read the test answers when it likes.
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>(r => { resolve = r; });
  return { promise, resolve };
}

const issue = (number: number) => ({ number, title: `issue ${number}`, labels: [], author: "kim", opened_at: 1, comments: 0, url: `https://example.com/issues/${number}` });

test("kept issues draw before any read answers, and one closed upstream goes once the read in the background answers", async () => {
  let now = 1_000_000;
  const cache = new PageCache("acct", () => now);
  const kept = cache.section<KeptThreads>("threads");
  kept.put("p1", { issues: [issue(1), issue(2)], prs: [] });
  const lists = deferred<unknown>();
  const asked: string[] = [];
  const invoke = ((cmd: string) => { asked.push(cmd); return cmd === "center_project_lists" ? lists.promise : Promise.resolve([]); }) as unknown as ThreadDeps["invoke"];
  let redraws = 0;
  const thr = makeThreads({ invoke, redraw() { redraws++; }, openUrl() {}, kept });
  // Opened again soon: what is kept shows and nothing is asked.
  now += SWITCH_MS - 1;
  assert.equal(thr.ensure("p1", SWITCH_MS), undefined);
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [1, 2]);
  assert.equal(asked.length, 0);
  // Later: still drawn from the cache at once, read behind it.
  now += 1;
  const read = thr.ensure("p1", SWITCH_MS);
  assert.ok(read);
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [1, 2], "the kept list shows while the read is on its way");
  lists.resolve({ prs: [], prs_error: null, issues: [issue(2), issue(3)], issues_error: null });
  await read;
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [2, 3], "#1 closed, #3 opened");
  assert.deepEqual(cache.get<KeptThreads>("threads", "p1")?.issues?.map(i => i.number), [2, 3], "kept for next time");
  assert.ok(redraws > 0);
});


test("pushed lists are taken as they come, and a read that was out meanwhile does not replace them", async () => {
  const late = deferred<unknown>();
  const invoke = (() => late.promise) as unknown as ThreadDeps["invoke"];
  const thr = makeThreads({ invoke, redraw() {}, openUrl() {} });
  const read = thr.load("p1", true);
  thr.take("p1", { value: { prs: [], prs_error: null, issues: [issue(5)], issues_error: null } });
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [5]);
  late.resolve({ prs: [], prs_error: null, issues: [issue(1)], issues_error: null });
  await read;
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [5], "the older answer is dropped");
  thr.take("p1", { error: "host down" });
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [5], "a failed push keeps the last good list");
});
