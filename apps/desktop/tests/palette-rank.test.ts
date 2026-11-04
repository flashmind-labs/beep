import test from "node:test";
import assert from "node:assert/strict";
import { rank, type Entry, type HitKind, type PaletteIndex } from "../src/center/palette-rank.ts";

function entry(kind: HitKind, title: string, id = title, detail = ""): Entry {
  return { kind, id, title, detail, keys: [title.toLowerCase()] };
}

function index(lists: Partial<PaletteIndex> = {}): PaletteIndex {
  return { actions: [], settings: [], code: [], projects: [], sessions: [], chats: [], memories: [], routines: [], machines: [], ...lists };
}

const titles = (hits: Entry[]) => hits.map(h => h.title);

test("a prefix beats a word start, which beats a substring", () => {
  const routines = ["Prebrief", "Morning brief", "Brief review"].map(t => entry("routine", t));
  assert.deepEqual(titles(rank("bri", index({ routines }))), ["Brief review", "Morning brief", "Prebrief"]);
});

test("a later word start counts when an earlier match is inside a word", () => {
  const routines = [entry("routine", "Debrief or brief"), entry("routine", "Debrief")];
  assert.deepEqual(titles(rank("brief", index({ routines }))), ["Debrief or brief", "Debrief"]);
});

test("the best of an entry's keys is its tier, ties in list order", () => {
  const session = (title: string, ...keys: string[]) => ({ ...entry("agent_session", title), keys });
  const sessions = [session("Fix widget", "fix widget"), session("Sync", "sync", "widget"), session("Login", "widget login")];
  assert.deepEqual(titles(rank("widget", index({ sessions }))), ["Sync", "Login", "Fix widget"]);
});

test("the query is trimmed and case does not matter", () => {
  const machines = [entry("machine", "Build-Box")];
  assert.deepEqual(titles(rank("  BUILD ", index({ machines }))), ["Build-Box"]);
});

test("each group shows eight", () => {
  const memories = Array.from({ length: 20 }, (_, i) => entry("memory", `likes tea ${i}`));
  assert.equal(rank("tea", index({ memories })).length, 8);
});

test("nothing typed lists every action, then the newest chats", () => {
  const actions = [entry("action", "Home"), entry("action", "Settings")];
  const chats = Array.from({ length: 7 }, (_, i) => entry("chat", `chat ${i}`));
  const hits = rank("", index({ actions, chats }));
  assert.deepEqual(titles(hits), ["Home", "Settings", "chat 0", "chat 1", "chat 2", "chat 3", "chat 4"]);
});

test("a title match beats a chat found only by its text", () => {
  const chats = [entry("chat", "Groceries", "a"), entry("chat", "Trip plan", "b")];
  const text = [entry("chat", "Groceries", "a", "...trip...")];
  const hits = rank("trip", index({ chats }), text);
  assert.deepEqual(hits.map(h => h.id), ["b", "a"]);
  assert.equal(hits[1].detail, "...trip...");
});

test("a chat found by title and text is one hit with the first snippet", () => {
  const chats = [entry("chat", "Trip plan", "a")];
  const text = [entry("chat", "Trip plan", "a", "first"), entry("chat", "Trip plan", "a", "second")];
  const hits = rank("trip", index({ chats }), text);
  assert.equal(hits.length, 1);
  assert.equal(hits[0].detail, "first");
  assert.equal(chats[0].detail, "", "the index keeps its own entry");
});

test("the top hit leads when several groups match, and lists once", () => {
  const hits = rank("acme", index({
    projects: [entry("project", "acme/widget")],
    code: [entry("code", "acme/widget PR")],
    chats: [entry("chat", "Acme ideas")],
  }));
  assert.equal(hits[0].kind, "project");
  assert.equal(hits[0].top, true);
  assert.equal(hits.filter(h => h.top).length, 1);
  assert.equal(hits.filter(h => h.kind === "project").length, 1);
});

test("no top hit from one group, or without a prefix match", () => {
  assert.ok(rank("morning", index({ routines: [entry("routine", "Morning brief")] })).every(h => !h.top));
  const noPrefix = index({ routines: [entry("routine", "A morning")], machines: [entry("machine", "the morning box")] });
  assert.ok(rank("morning", noPrefix).every(h => !h.top));
});

/// Enough for a heavy account: about 3,600 entries.
function bigIndex(): PaletteIndex {
  const many = (n: number, make: (i: number) => Entry) => Array.from({ length: n }, (_, i) => make(i));
  return index({
    actions: many(9, i => entry("action", `Action ${i}`)),
    settings: many(30, i => entry("setting", `Setting number ${i}`)),
    code: many(200, i => ({ ...entry("code", `acme/project-${i % 100} · PR #${i} Fix issue ${i} in the login flow`), keys: [`fix issue ${i} in the login flow`, `acme/project-${i % 100}`, `#${i}`] })),
    projects: many(200, i => entry("project", `acme/project-${i}`)),
    sessions: many(300, i => ({ ...entry("agent_session", `Agent work ${i}`), keys: [`agent work ${i}`, `folder-${i % 60}`, "claude code", "claude", `box-${i % 10}`] })),
    chats: many(1800, i => entry("chat", `Chat number ${i} about the login flow`, String(i))),
    memories: many(1000, i => entry("memory", `the user likes thing number ${i} a lot, and said so in a long sentence`)),
    routines: many(50, i => entry("routine", `Routine ${i}`)),
    machines: many(10, i => entry("machine", `box-${i}`)),
  });
}

test("a keystroke over a big index stays under a millisecond", () => {
  const big = bigIndex();
  const typed = ["l", "lo", "log", "logi", "login", "project", "x", "nothing here", "#42", "box-3"];
  // Warm, as it is after the first few keystrokes of a session.
  for (let run = 0; run < 200; run++) rank(typed[run % typed.length], big);
  const times: number[] = [];
  for (let run = 0; run < 1000; run++) {
    const at = performance.now();
    rank(typed[run % typed.length], big);
    times.push(performance.now() - at);
  }
  times.sort((a, b) => a - b);
  const [median, p95, worst] = [times[500], times[950], times[999]];
  console.log(`palette rank over 3600 entries: median ${median.toFixed(3)} ms, p95 ${p95.toFixed(3)} ms, max ${worst.toFixed(3)} ms`);
  // The median, as a busy host can stall any single run.
  assert.ok(median < 1, `median ${median} ms`);
});
