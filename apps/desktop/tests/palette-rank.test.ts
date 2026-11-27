import test from "node:test";
import assert from "node:assert/strict";
import { rank, type Entry, type HitKind, type PaletteIndex } from "../src/shell/center/palette-rank.ts";

function entry(kind: HitKind, title: string, id = title, detail = ""): Entry {
  return { kind, id, title, detail, keys: [title.toLowerCase()] };
}

function index(lists: Partial<PaletteIndex> = {}): PaletteIndex {
  return { actions: [], settings: [], code: [], projects: [], sessions: [], machines: [], ...lists };
}

const titles = (hits: Entry[]) => hits.map(h => h.title);

test("a prefix beats a word start, which beats a substring", () => {
  const settings = ["Prebrief", "Morning brief", "Brief review"].map(t => entry("setting", t));
  assert.deepEqual(titles(rank("bri", index({ settings }))), ["Brief review", "Morning brief", "Prebrief"]);
});

test("a later word start counts when an earlier match is inside a word", () => {
  const settings = [entry("setting", "Debrief or brief"), entry("setting", "Debrief")];
  assert.deepEqual(titles(rank("brief", index({ settings }))), ["Debrief or brief", "Debrief"]);
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
  const projects = Array.from({ length: 20 }, (_, i) => entry("project", `acme/tea-${i}`));
  assert.equal(rank("tea", index({ projects })).length, 8);
});

test("repeated matches in a project title rank before siblings and survive the group limit", () => {
  const siblings = Array.from({ length: 10 }, (_, i) => entry("project", `sample/tool-${i}`));
  const repeated = entry("project", "Sample/Sample");
  const hits = rank(" sample ", index({ projects: [...siblings, repeated], machines: [entry("machine", "sample")] }));
  assert.equal(hits[0].title, repeated.title);
  assert.equal(hits[0].top, true);
  assert.deepEqual(titles(hits.slice(1, 8)), titles(siblings.slice(0, 7)));
});

test("a stronger prefix match wins Top hit across groups, even with duplicate project keys", () => {
  const project = { ...entry("project", "sample/tool"), keys: ["sample/tool", "sample/tool"] };
  const hits = rank("sample", index({ projects: [project], machines: [entry("machine", "sample-sample")] }));
  assert.equal(hits[0].kind, "machine");
  assert.equal(hits[0].top, true);
});

test("nothing typed lists every action", () => {
  const actions = [entry("action", "Home"), entry("action", "Settings")];
  const projects = [entry("project", "acme/widget")];
  assert.deepEqual(titles(rank("", index({ actions, projects }))), ["Home", "Settings"]);
});

test("the top hit leads when several groups match, and lists once", () => {
  const hits = rank("acme", index({
    projects: [entry("project", "acme/widget")],
    code: [entry("code", "acme/widget PR")],
    machines: [entry("machine", "acme box")],
  }));
  assert.equal(hits[0].kind, "project");
  assert.equal(hits[0].top, true);
  assert.equal(hits.filter(h => h.top).length, 1);
  assert.equal(hits.filter(h => h.kind === "project").length, 1);
});

test("projects, machines and coding agents list before the rest", () => {
  const hits = rank("acme", index({
    actions: [entry("action", "New acme session")],
    settings: [entry("setting", "Acme look")],
    code: [entry("code", "acme/widget PR")],
    projects: [entry("project", "acme/widget")],
    sessions: [entry("agent_session", "Fix acme login")],
    machines: [entry("machine", "acme box")],
  }));
  assert.deepEqual(hits.map(h => h.kind), ["project", "machine", "agent_session", "code", "setting", "action"]);
});

test("no top hit from one group, or without a prefix match", () => {
  assert.ok(rank("morning", index({ settings: [entry("setting", "Morning brief")] })).every(h => !h.top));
  const noPrefix = index({ settings: [entry("setting", "A morning")], machines: [entry("machine", "the morning box")] });
  assert.ok(rank("morning", noPrefix).every(h => !h.top));
});

/// Enough for a heavy account: about 2,700 entries.
function bigIndex(): PaletteIndex {
  const many = (n: number, make: (i: number) => Entry) => Array.from({ length: n }, (_, i) => make(i));
  return index({
    actions: many(9, i => entry("action", `Action ${i}`)),
    settings: many(30, i => entry("setting", `Setting number ${i}`)),
    code: many(200, i => ({ ...entry("code", `acme/project-${i % 100} · PR #${i} Fix issue ${i} in the login flow`), keys: [`fix issue ${i} in the login flow`, `acme/project-${i % 100}`, `#${i}`] })),
    projects: many(200, i => entry("project", `acme/project-${i}`)),
    sessions: many(2250, i => ({ ...entry("agent_session", `Agent work ${i} on the login flow`), keys: [`agent work ${i} on the login flow`, `folder-${i % 60}`, "claude code", "claude", `box-${i % 10}`] })),
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
  console.log(`palette rank over 2700 entries: median ${median.toFixed(3)} ms, p95 ${p95.toFixed(3)} ms, max ${worst.toFixed(3)} ms`);
  // The median, as a busy host can stall any single run.
  assert.ok(median < 1, `median ${median} ms`);
});

test("prompt matches join coding sessions, with an excerpt and the session to open", () => {
  const session = { machine_id: "build-box", id: "a" };
  const listed = { ...entry("agent_session", "Connection fixes", "build-box a", "Codex · widget"), session };
  const prompt = { ...listed, snippet: "Add a retry button" };
  const hits = rank("retry", index({ sessions: [listed] }), [prompt]);
  assert.equal(hits.length, 1);
  assert.equal(hits[0].kind, "agent_session");
  assert.equal(hits[0].session, session);
  assert.equal(hits[0].detail, prompt.detail);
  assert.equal(hits[0].snippet, prompt.snippet);
});

test("title and prompt hits deduplicate without merging other kinds with the same id", () => {
  const listed = entry("agent_session", "Retry fixes", "same", "Codex · widget");
  const prompt = { ...listed, snippet: "Add a retry button" };
  const project = entry("project", "Retry tools", "same");
  const hits = rank("retry", index({ sessions: [listed], projects: [project] }), [prompt]);
  assert.equal(hits.filter(hit => hit.kind === "agent_session").length, 1);
  assert.equal(hits.find(hit => hit.kind === "agent_session")?.detail, prompt.detail);
  assert.equal(hits.find(hit => hit.kind === "agent_session")?.snippet, prompt.snippet);
  assert.equal(hits.filter(hit => hit.kind === "project").length, 1);
  assert.equal(listed.detail, "Codex · widget");
});
