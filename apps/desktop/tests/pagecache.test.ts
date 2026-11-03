import test from "node:test";
import assert from "node:assert/strict";
import { PAGE_CACHE_FORMAT, PageCache, REFRESH_MS, backgroundTick, evict } from "../src/pagecache.ts";

/// A clock the test moves by hand.
function clock(start = 1_000_000) {
  const c = { now: start, tick: () => c.now };
  return c;
}

test("a section is due until read, then again after the refresh time", () => {
  const c = clock();
  const cache = new PageCache("acct", c.tick);
  assert.equal(cache.due("threads", "p1"), true);
  cache.put("threads", "p1", { prs: [] });
  assert.equal(cache.due("threads", "p1"), false);
  c.now += REFRESH_MS - 1;
  assert.equal(cache.due("threads", "p1"), false);
  c.now += 1;
  assert.equal(cache.due("threads", "p1"), true);
  // Another project and another section are their own.
  assert.equal(cache.due("threads", "p2"), true);
  assert.equal(cache.due("branches", "p1"), true);
});

test("restoring keeps what was read since, when newer", () => {
  const c = clock();
  const cache = new PageCache(null, c.tick);
  cache.put("view", "p", "read now");
  const file = { format: PAGE_CACHE_FORMAT, account: "a", slots: {
    "view\np": { data: "saved", at: c.now - 1000, used: c.now - 1000 },
    "view\nq": { data: "saved q", at: c.now - 1000, used: c.now - 1000 },
  } };
  cache.restore(file, "a");
  assert.equal(cache.account, "a");
  assert.equal(cache.get("view", "p"), "read now");
  assert.equal(cache.get("view", "q"), "saved q");
});

test("a saved cache comes back for its account only", () => {
  const c = clock();
  const cache = new PageCache("acct-a", c.tick);
  cache.put("view", "org/repo", { tasks: [1] });
  cache.put("where", "org/repo", [{ id: "m" }]);
  const file = JSON.parse(JSON.stringify(cache.toFile()));

  const again = PageCache.from(file, "acct-a", c.tick);
  assert.deepEqual(again.get("view", "org/repo"), { tasks: [1] });
  assert.deepEqual(again.get("where", "org/repo"), [{ id: "m" }]);
  assert.equal(again.at("view", "org/repo"), c.now);

  assert.equal(PageCache.from(file, "acct-b", c.tick).size, 0);
  assert.equal(PageCache.from(file, null, c.tick).size, 0);
});

test("a file of another format, a broken one or unknown sections start empty", () => {
  const c = clock();
  const slot = { data: 1, at: c.now, used: c.now };
  assert.equal(PageCache.from({ format: PAGE_CACHE_FORMAT + 1, account: "a", slots: { "view\np": slot } }, "a", c.tick).size, 0);
  assert.equal(PageCache.from("nonsense", "a", c.tick).size, 0);
  assert.equal(PageCache.from(null, "a", c.tick).size, 0);
  const mixed = PageCache.from({ format: PAGE_CACHE_FORMAT, account: "a", slots: { "view\np": slot, "later\np": slot, "where\nq": { data: 1 } } }, "a", c.tick);
  assert.equal(mixed.size, 1);
  assert.equal(mixed.get("view", "p"), 1);
});

test("eviction keeps the most recently used within the slot and size caps", () => {
  const slot = (used: number, data: unknown = "x") => ({ data, at: used, used });
  const kept = evict([["view\na", slot(1)], ["view\nb", slot(3)], ["view\nc", slot(2)]], 2);
  assert.deepEqual(kept.map(([k]) => k), ["view\nb", "view\nc"]);

  // One too big for the size cap goes; the smaller ones stay.
  const big = "y".repeat(500);
  const sized = evict([["view\na", slot(1)], ["view\nbig", slot(5, big)], ["view\nc", slot(2)]], 10, 200);
  assert.deepEqual(sized.map(([k]) => k), ["view\nc", "view\na"]);
});

test("saving drops what does not fit from memory too, and using a slot keeps it", () => {
  const c = clock();
  const cache = new PageCache("a", c.tick);
  for (const id of ["p1", "p2", "p3"]) { cache.put("sessions", id, []); c.now += 1; }
  // p1 was used last, so p2 is the oldest now.
  cache.get("sessions", "p1");
  const file = cache.toFile(2)!;
  assert.deepEqual(Object.keys(file.slots).sort(), ["sessions\np1", "sessions\np3"]);
  assert.equal(cache.size, 2);
  assert.equal(cache.get("sessions", "p2"), undefined);
});

test("sign-out and another account clear it", () => {
  // Learning the account of data read before keeps it.
  const early = new PageCache(null);
  early.put("view", "p", 1);
  early.setAccount("a");
  assert.equal(early.size, 1);

  const cache = new PageCache("a");
  cache.put("view", "p", 1);
  cache.takeChanged();
  cache.setAccount("a");
  assert.equal(cache.size, 1);
  cache.setAccount("b");
  assert.equal(cache.size, 0);
  assert.equal(cache.account, "b");
  assert.equal(cache.takeChanged(), true);
  cache.put("view", "p", 1);
  cache.setAccount(null);
  assert.equal(cache.size, 0);
  assert.equal(cache.toFile(), null);
});

test("background ticks run online, skip offline, and skip one late after sleep", () => {
  const every = 4 * 60_000;
  assert.equal(backgroundTick(0, every, every, true), "run");
  assert.equal(backgroundTick(0, every, every, false), "offline");
  assert.equal(backgroundTick(0, 2 * every + 1, every, true), "woke");
});
