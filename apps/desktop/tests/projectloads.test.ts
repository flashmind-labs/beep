import test from "node:test";
import assert from "node:assert/strict";
import { keyedReads } from "../src/project/projectloads.ts";
import { PageCache, SWITCH_MS } from "../src/ui/pagecache.ts";
import { makeThreads, type KeptThreads, type ThreadDeps } from "../src/project/threads.ts";

/// A read the test answers when it likes.
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>(r => { resolve = r; });
  return { promise, resolve };
}

test("a slow answer for the project left behind is kept for it and does not redraw the one shown", async () => {
  const pending = new Map<string, ReturnType<typeof deferred<string>>>();
  const kept = new Map<string, string>();
  let shown = "alpha";
  const drawn: string[] = [];
  const reads = keyedReads<string>({
    read: key => { const d = deferred<string>(); pending.set(key, d); return d.promise; },
    got: (key, v) => kept.set(key, v),
    failed: () => assert.fail("no read fails here"),
    shown: () => shown,
    redraw: () => drawn.push(shown),
  });
  const alpha = reads.load("alpha");
  shown = "beta";
  const beta = reads.load("beta");
  pending.get("beta")!.resolve("beta view");
  await beta;
  assert.deepEqual(drawn, ["beta"]);
  pending.get("alpha")!.resolve("alpha view");
  await alpha;
  assert.equal(kept.get("alpha"), "alpha view");
  assert.equal(kept.get("beta"), "beta view");
  assert.deepEqual(drawn, ["beta"], "alpha's answer drew nothing over beta");
});

test("a project's read runs once at a time, and one asked for after a change runs once more", async () => {
  let reads = 0;
  const pending: ReturnType<typeof deferred<number>>[] = [];
  const r = keyedReads<number>({
    read: () => { reads++; const d = deferred<number>(); pending.push(d); return d.promise; },
    got() {}, failed() {}, shown: () => null, redraw() {},
  });
  const a = r.load("p");
  assert.equal(r.load("p"), a, "the same read");
  assert.ok(r.loading("p"));
  const after = r.load("p", true);
  assert.equal(r.load("p", true), after, "one more read, not one per ask");
  pending[0].resolve(1);
  await a;
  await Promise.resolve();
  assert.equal(reads, 2);
  pending[1].resolve(2);
  await after;
  assert.ok(!r.loading("p"));
});

const issue = (number: number) => ({ number, title: `issue ${number}`, labels: [], author: "kim", opened_at: 1, comments: 0, url: `https://example.com/issues/${number}` });

test("kept issues draw before any read answers, and one closed upstream goes once the read in the background answers", async () => {
  let now = 1_000_000;
  const cache = new PageCache("acct", () => now);
  const kept = cache.section<KeptThreads>("threads");
  kept.put("p1", { issues: [issue(1), issue(2)], threads: [], prs: [], chats: [] });
  const lists = deferred<unknown>();
  const asked: string[] = [];
  const invoke = ((cmd: string) => { asked.push(cmd); return cmd === "center_project_lists" ? lists.promise : Promise.resolve([]); }) as unknown as ThreadDeps["invoke"];
  let redraws = 0;
  const thr = makeThreads({ invoke, h: null!, button: null!, ago: () => "", redraw() { redraws++; }, openUrl() {}, openChat() {}, changed() {}, kept });
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
  assert.ok(thr.loading("p1"));
  lists.resolve({ prs: [], prs_error: null, issues: [issue(2), issue(3)], issues_error: null, synced_at: now });
  await read;
  assert.deepEqual(thr.issues("p1")?.map(i => i.number), [2, 3], "#1 closed, #3 opened");
  assert.deepEqual(cache.get<KeptThreads>("threads", "p1")?.issues?.map(i => i.number), [2, 3], "kept for next time");
  assert.ok(redraws > 0);
});

