import test from "node:test";
import assert from "node:assert/strict";
import { PAGE_CACHE_FORMAT, PageCache, REFRESH_MS, evict } from "../src/ui/pagecache.ts";

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
  assert.equal(cache.due("runs", "p1"), true);
});

test("restoring keeps what was read since, when newer", () => {
  const c = clock();
  const cache = new PageCache(null, c.tick);
  cache.put("where", "p", "read now");
  const file = { format: PAGE_CACHE_FORMAT, account: "a", slots: {
    "where\np": { data: "saved", at: c.now - 1000, used: c.now - 1000 },
    "where\nq": { data: "saved q", at: c.now - 1000, used: c.now - 1000 },
  } };
  cache.restore(file, "a");
  assert.equal(cache.account, "a");
  assert.equal(cache.get("where", "p"), "read now");
  assert.equal(cache.get("where", "q"), "saved q");
});

test("a saved cache comes back for its account only", () => {
  const c = clock();
  const cache = new PageCache("acct-a", c.tick);
  cache.put("sessions", "org/repo", { tasks: [1] });
  cache.put("where", "org/repo", [{ id: "m" }]);
  const file = JSON.parse(JSON.stringify(cache.toFile()));

  const again = PageCache.from(file, "acct-a", c.tick);
  assert.deepEqual(again.get("sessions", "org/repo"), { tasks: [1] });
  assert.deepEqual(again.get("where", "org/repo"), [{ id: "m" }]);
  assert.equal(again.at("where", "org/repo"), c.now);

  assert.equal(PageCache.from(file, "acct-b", c.tick).size, 0);
  assert.equal(PageCache.from(file, null, c.tick).size, 0);
});

test("a file of another format, a broken one or unknown sections start empty", () => {
  const c = clock();
  const slot = { data: 1, at: c.now, used: c.now };
  assert.equal(PageCache.from({ format: PAGE_CACHE_FORMAT + 1, account: "a", slots: { "where\np": slot } }, "a", c.tick).size, 0);
  assert.equal(PageCache.from("nonsense", "a", c.tick).size, 0);
  assert.equal(PageCache.from(null, "a", c.tick).size, 0);
  const mixed = PageCache.from({ format: PAGE_CACHE_FORMAT, account: "a", slots: { "where\np": slot, "later\np": slot, "where\nq": { data: 1 } } }, "a", c.tick);
  assert.equal(mixed.size, 1);
  assert.equal(mixed.get("where", "p"), 1);
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

test("data held without a read goes from memory with the rest and is never saved", () => {
  const c = clock();
  const cache = new PageCache("a", c.tick);
  for (const id of ["h1", "h2", "h3"]) { cache.hold("sessions", id, []); c.now += 1; }
  cache.put("sessions", "read", []);
  c.now += 1;

  const file = cache.toFile(2)!;
  assert.deepEqual(Object.keys(file.slots), ["sessions\nread"]);
  assert.equal(cache.size, 2);
  assert.equal(cache.peek("sessions", "h1"), undefined);
  assert.deepEqual(cache.peek("sessions", "h3"), []);
});

test("sign-out and another account clear it", () => {
  // Learning the account of data read before keeps it.
  const early = new PageCache(null);
  early.put("where", "p", 1);
  early.setAccount("a");
  assert.equal(early.size, 1);

  const cache = new PageCache("a");
  cache.put("where", "p", 1);
  cache.takeChanged();
  cache.setAccount("a");
  assert.equal(cache.size, 1);
  cache.setAccount("b");
  assert.equal(cache.size, 0);
  assert.equal(cache.account, "b");
  assert.equal(cache.takeChanged(), true);
  cache.put("where", "p", 1);
  cache.setAccount(null);
  assert.equal(cache.size, 0);
  assert.equal(cache.toFile(), null);
});

test("data held without a read keeps the read time, is not saved alone and survives a restore", () => {
  const c = clock();
  const cache = new PageCache("a", c.tick);
  cache.put("where", "p", ["read"]);
  const at = c.now;
  c.now += 10_000;
  cache.hold("where", "p", ["amended"]);
  assert.deepEqual(cache.get("where", "p"), ["amended"]);
  assert.equal(cache.at("where", "p"), at, "an amendment is not a read");
  cache.takeChanged();

  cache.hold("where", "q", null);
  assert.equal(cache.get("where", "q"), null, "a failure is held, apart from never read");
  assert.equal(cache.at("where", "q"), undefined);
  assert.equal(cache.due("where", "q"), true);
  assert.deepEqual(Object.keys(cache.toFile()!.slots), ["where\np"], "what was never read is not saved");

  // A file read later fills what was held without a read, but not what was read since.
  cache.restore({ format: PAGE_CACHE_FORMAT, account: "a", slots: {
    "where\nq": { data: ["old"], at: c.now - 1000, used: c.now - 1000 },
    "where\np": { data: ["older"], at: at - 1000, used: at - 1000 },
  } }, "a");
  assert.deepEqual(cache.get("where", "q"), ["old"]);
  assert.deepEqual(cache.get("where", "p"), ["amended"]);
});

test("a project's section, or a whole section, can be forgotten, and the ids come back", () => {
  const cache = new PageCache("a");
  for (const id of ["p1", "p2"]) { cache.put("where", id, []); cache.put("sessions", id, []); }
  assert.deepEqual(cache.ids("where").sort(), ["p1", "p2"]);
  cache.drop("where", "p1");
  assert.equal(cache.get("where", "p1"), undefined);
  assert.deepEqual(cache.ids("where"), ["p2"]);
  cache.drop("where");
  assert.deepEqual(cache.ids("where"), []);
  assert.deepEqual(cache.ids("sessions").sort(), ["p1", "p2"], "other sections stay");
  assert.equal(cache.peek("sessions", "p1") !== undefined, true);
});
