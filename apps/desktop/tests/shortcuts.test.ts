import test from "node:test";
import assert from "node:assert/strict";
import { filterShortcuts, SHORTCUTS, visibleShortcuts } from "../src/ui/shortcuts.ts";

const rows = (q: string) => filterShortcuts(SHORTCUTS, q).flatMap(s => s.rows.map(r => r.does));

test("the filter matches words and keys", () => {
  assert.deepEqual(rows("word"), ["Word back", "Word forward", "Delete word", "Delete next word"]);
  assert.ok(filterShortcuts(SHORTCUTS, "⌘k").some(s => s.title === "Window" && s.rows.some(r => r.does === "Search")));
  assert.deepEqual(filterShortcuts(SHORTCUTS, ""), [...SHORTCUTS]);
  assert.deepEqual(filterShortcuts(SHORTCUTS, "zzzz"), []);
});

test("a section title keeps all its rows", () => {
  const review = filterShortcuts(SHORTCUTS, "review");
  assert.equal(review[0].rows.length, SHORTCUTS.find(s => s.title === "Review")!.rows.length);
});

test("rows for features that are off are left out", () => {
  const off = visibleShortcuts({ server: false, live: false });
  const all = off.flatMap(s => s.rows.map(r => r.does));
  assert.ok(!all.includes("Memories") && !all.includes("Talk while held"));
  assert.ok(!off.some(s => s.title === "Lists"));
  assert.ok(visibleShortcuts({ server: true, live: true }).some(s => s.title === "Lists"));
});
