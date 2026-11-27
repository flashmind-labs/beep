import test from "node:test";
import assert from "node:assert/strict";
import {
  type DiffFile, type DiffView, allFolded, baseTitle, changedLabel, compactFolders, diffRows, diffSpans, filesInTreeOrder, filterHits, lineMatches,
  splitHeight, splitRows, withLoaded,
} from "../src/stores/review/diff-state.ts";
import { colourFor } from "../src/ui/syntax.ts";

const file = (path: string, o: Partial<DiffFile> = {}): DiffFile => ({
  path, old_path: null, status: "modified", additions: 1, deletions: 1, hunks: [], left_out: null, ...o,
});

test("diff order follows tree folders when added files arrive after modified files", () => {
  const paths = [
    "src/link/events.rs",
    "src/link/events/snapshots.rs",
    "src/link/mod.rs",
    "tests/link.rs",
    "src/link/channels.rs",
    "src/link/commands.rs",
    "src/link/events/subscriptions.rs",
    "src/link/subscriptions.rs",
    "README.md",
  ];
  const files = paths.map(path => file(path));
  const ordered = filesInTreeOrder(files);
  assert.deepEqual(ordered.map(f => f.path), [
    "src/link/events.rs",
    "src/link/events/snapshots.rs",
    "src/link/events/subscriptions.rs",
    "src/link/mod.rs",
    "src/link/channels.rs",
    "src/link/commands.rs",
    "src/link/subscriptions.rs",
    "tests/link.rs",
    "README.md",
  ]);
  assert.deepEqual(files.map(f => f.path), paths);
  assert.equal(ordered[0], files[0]);
  assert.deepEqual(filesInTreeOrder(ordered), ordered);
  assert.deepEqual(filesInTreeOrder([]), []);
});

test("rows number both sides and count the lines between hunks", () => {
  const rows = diffRows({
    hunks: [
      { old_start: 1, old_lines: 3, new_start: 1, new_lines: 3, section: "", lines: [{ k: " ", t: "a" }, { k: "-", t: "b" }, { k: "+", t: "B" }, { k: " ", t: "c" }] },
      { old_start: 20, old_lines: 1, new_start: 20, new_lines: 2, section: "fn x() {", lines: [{ k: " ", t: "y" }, { k: "+", t: "z", e: true }] },
    ],
  });
  const text = rows.map(r => r.kind === "line" ? `${r.old ?? "."} ${r.new ?? "."} ${r.k}${r.text}${r.eol ? "$" : ""}` : r.gap ? `gap ${r.gap} ${r.text}` : r.text);
  assert.deepEqual(text, [
    "@@ -1,3 +1,3 @@", "1 1  a", "2 . -b", ". 2 +B", "3 3  c",
    "gap 16 @@ -20 +20,2 @@ fn x() {", "20 20  y", ". 21 +z$",
  ]);
});

test("a new file counts from its first line and a deleted one from its old", () => {
  const added = diffRows({ hunks: [{ old_start: 0, old_lines: 0, new_start: 1, new_lines: 2, section: "", lines: [{ k: "+", t: "a" }, { k: "+", t: "b" }] }] });
  assert.deepEqual(added.filter(r => r.kind === "line").map(r => r.kind === "line" && r.new), [1, 2]);
  const gone = diffRows({ hunks: [{ old_start: 1, old_lines: 1, new_start: 0, new_lines: 0, section: "", lines: [{ k: "-", t: "a" }] }] });
  assert.deepEqual(gone.filter(r => r.kind === "line").map(r => r.kind === "line" && r.old), [1]);
});

test("a file loaded in full takes its place", () => {
  const view: DiffView = { root: "/srv/project", base: "origin/main", merge_base: "abc", working_tree: true, files: [file("a.rs"), file("big.rs", { left_out: "too_large" })], additions: 2, deletions: 2, more: 0 };
  const full = file("big.rs", { hunks: [{ old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, section: "", lines: [] }] });
  const next = withLoaded(view, [full]);
  assert.deepEqual(next.files.map(f => f.left_out), [null, null]);
  assert.equal(next.files[1], full);
});

const hunk = (lines: string, old_start = 1, new_start = 1) => ({
  old_start, old_lines: 1, new_start, new_lines: 1, section: "",
  lines: [...lines].map(c => ({ k: c as "+" | "-" | " ", t: c === "+" ? "new" : c === "-" ? "old" : "same" })),
});
const pairs = (lines: string) => splitRows(diffRows({ hunks: [hunk(lines)] })).filter(r => r.kind === "pair")
  .map(r => (r.kind === "pair" ? [r.left?.num ?? null, r.right?.num ?? null] : []));

test("side by side pairs a replaced run row by row and pads the shorter side", () => {
  assert.deepEqual(pairs("--+"), [[1, 1], [2, null]]);
  assert.deepEqual(pairs("-+++"), [[1, 1], [null, 2], [null, 3]]);
});

test("side by side puts a pure add on the right and a pure delete on the left", () => {
  assert.deepEqual(pairs("++"), [[null, 1], [null, 2]]);
  assert.deepEqual(pairs("--"), [[1, null], [2, null]]);
});

test("side by side shows context on both sides and starts a new run after it", () => {
  assert.deepEqual(pairs("-+ -+"), [[1, 1], [2, 2], [3, 3]]);
  const rows = splitRows(diffRows({ hunks: [hunk(" -")] }));
  const ctx = rows[1];
  assert.equal(ctx.kind === "pair" && ctx.left?.k, " ");
  assert.equal(ctx.kind === "pair" && ctx.right?.text, "same");
});

test("splitHeight counts wrapped lines", () => {
  const cell = (n: number) => ({ k: " " as const, num: 1, text: "x".repeat(n), eol: false });
  const rows = [{ kind: "hunk" as const, text: "@@", index: 0, gap: 0, expanded: false }, { kind: "pair" as const, left: cell(10), right: cell(25) }];
  assert.equal(splitHeight(rows, 10), 19 + 19 * 3);
});

test("the header counts files and keeps the base in its tooltip", () => {
  assert.equal(changedLabel({ files: [file("a"), file("b")], more: 1 }), "3 files");
  assert.equal(changedLabel({ files: [file("a")], more: 0 }), "1 file");
  assert.equal(changedLabel({ files: [], more: 0 }), "No changes");
  const v = { base: "origin/main", merge_base: "3f2c1a0e9b", working_tree: true };
  assert.equal(baseTitle(v, false), "Against origin/main at 3f2c1a0, uncommitted included");
  assert.equal(baseTitle({ ...v, merge_base: "", working_tree: false }, false), "Against origin/main");
  assert.equal(baseTitle(v, true), "Against HEAD, uncommitted changes only");
});

test("the fold toggle offers expand only when every shown file is folded", () => {
  const files = [file("a"), file("gone", { status: "deleted" })];
  assert.equal(allFolded(files, new Map()), false);
  assert.equal(allFolded(files, new Map([["a", true]])), true);
  assert.equal(allFolded([files[1]], new Map()), true);
  assert.equal(allFolded(files, new Map([["a", true], ["gone", false]])), false);
  assert.equal(allFolded([], new Map()), false);
});

test("expanded context keeps line numbers and review placement", async () => {
  const { contextFile, placeComments } = await import("../src/stores/review/diff-state.ts");
  const full = file("sample.ts", { hunks: [{ old_start: 1, new_start: 1, old_lines: 30, new_lines: 30, section: "", lines: Array.from({ length: 30 }, (_, i) => ({ k: " " as const, t: String(i + 1) })) }] });
  const original = file("sample.ts", { hunks: [{ ...full.hunks[0], old_start: 14, new_start: 14, old_lines: 3, new_lines: 3, lines: full.hunks[0].lines.slice(13, 16) }] });
  const expanded = contextFile(original, full, [{ above: 10, below: 0 }]);
  assert.equal(expanded.hunks[0].new_start, 4);
  assert.equal(expanded.hunks[0].new_lines, 13);
  const comment = { path: "sample.ts", side: "new" as const, line: 4, text: "Check this" };
  assert.equal(placeComments([expanded], [comment]).files.get("sample.ts")?.lines.size, 1);
  assert.equal(contextFile(original, full, [{ above: 0, below: 10 }]).hunks[0].new_start, 14);
});

test("a gap between two hunks expands from either side alone and collapses", async () => {
  const { contextFile, diffRows, editExtents } = await import("../src/stores/review/diff-state.ts");
  const lines = Array.from({ length: 100 }, (_, i) => ({ k: " " as const, t: String(i + 1) }));
  const hunk = (start: number, count: number) => ({ old_start: start, new_start: start, old_lines: count, new_lines: count, section: "", lines: lines.slice(start - 1, start - 1 + count) });
  const full = file("sample.ts", { hunks: [hunk(1, 100)] });
  const original = file("sample.ts", { hunks: [hunk(10, 7), hunk(60, 7)] });
  const none = [{ above: 0, below: 0 }, { above: 0, below: 0 }];
  const shape = (f: typeof original) => f.hunks.map(h => [h.new_start, h.new_lines]);
  const seam = (f: typeof original, extents: typeof none) => diffRows(f, extents).flatMap(row => row.kind === "hunk" && row.index ? [[row.gap, row.expanded]] : []);
  assert.deepEqual(seam(original, none), [[43, false]]);

  const down = editExtents(none, [{ hunk: 0, side: "below", show: true }]);
  assert.deepEqual(shape(contextFile(original, full, down)), [[10, 17], [60, 7]]);
  assert.deepEqual(seam(contextFile(original, full, down), down), [[33, true]]);

  // Two hunks that come to touch stay two, so the seam can still collapse.
  const joined = [{ above: 0, below: 30 }, { above: 20, below: 0 }];
  assert.deepEqual(shape(contextFile(original, full, joined)), [[10, 30], [40, 27]]);
  assert.deepEqual(seam(contextFile(original, full, joined), joined), [[0, true]]);

  const back = editExtents(joined, [{ hunk: 0, side: "below", show: false }, { hunk: 1, side: "above", show: false }]);
  assert.deepEqual(back, none);
  assert.deepEqual(shape(contextFile(original, full, back)), shape(original));
});

test("folder chains compact up to branches and direct files", () => {
  assert.deepEqual([...compactFolders([
    "apps/client/src/fleet/events.rs",
    "apps/client/src/tests/events.rs",
    "crates/core/src/lib.rs",
    "crates/core/src/events/transport.rs",
    "README.md",
  ])], [
    ["apps/client/src/", { dir: "", name: "apps/client/src" }],
    ["apps/client/src/fleet/", { dir: "apps/client/src/", name: "fleet" }],
    ["apps/client/src/tests/", { dir: "apps/client/src/", name: "tests" }],
    ["crates/core/src/", { dir: "", name: "crates/core/src" }],
    ["crates/core/src/events/", { dir: "crates/core/src/", name: "events" }],
  ]);
  assert.deepEqual([...compactFolders(["src/auth/login.ts"])],
    [["src/auth/", { dir: "", name: "src/auth" }]]);
  assert.equal(compactFolders(["README.md"]).size, 0);
  assert.equal(compactFolders([]).size, 0);
});

const found = file("src/metrics.rs", { hunks: [{ old_start: 10, old_lines: 2, new_start: 10, new_lines: 2, section: "", lines: [
  { k: " ", t: "struct Metrics {" },
  { k: "-", t: "  old: Metrics, metrics: u8," },
  { k: "+", t: "  new: Metrics," },
] }] });

test("the filter finds every place in a line, and the tree lists each line once", () => {
  const hits = filterHits([found], "METRICS");
  assert.deepEqual(hits.map(h => [h.side, h.line, h.at]), [["new", 10, 7], ["old", 11, 7], ["old", 11, 16], ["new", 11, 7]]);
  assert.ok(hits.every(h => h.path === "src/metrics.rs"));
  assert.deepEqual(lineMatches(found, "metrics", 10).map(m => [m.side, m.line, m.at]), [["new", 10, 7], ["old", 11, 7], ["new", 11, 7]]);
});

test("each side of a hunk is coloured as one text, keyed by its own line numbers", async () => {
  const colour = await colourFor("src/metrics.rs");
  assert.ok(colour);
  const spans = diffSpans(found, colour);
  const text = (key: string, line: string) => (spans.get(key) ?? []).map(s => `${s.cls}:${line.slice(s.from, s.to)}`);
  assert.deepEqual(text("old:10", "struct Metrics {"), ["k:struct", "ty:Metrics"]);
  assert.deepEqual(text("new:10", "struct Metrics {"), ["k:struct", "ty:Metrics"]);
  assert.ok(text("old:11", "  old: Metrics, metrics: u8,").includes("ty:u8"));
  assert.ok(text("new:11", "  new: Metrics,").includes("ty:Metrics"));
});
