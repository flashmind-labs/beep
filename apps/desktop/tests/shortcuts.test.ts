import test from "node:test";
import assert from "node:assert/strict";
import { filterShortcuts, SHORTCUTS } from "../src/ui/shortcuts.ts";

const rows = (q: string) => filterShortcuts(SHORTCUTS, q).flatMap(s => s.rows.map(r => r.does));

test("the filter matches words and keys", () => {
  assert.deepEqual(rows("word"), ["Word back", "Word forward", "Delete word", "Delete next word"]);
  assert.ok(filterShortcuts(SHORTCUTS, "⌘k").some(s => s.title === "Window" && s.rows.some(r => r.does === "Search")));
  assert.deepEqual(filterShortcuts(SHORTCUTS, ""), [...SHORTCUTS]);
  assert.deepEqual(filterShortcuts(SHORTCUTS, "zzzz"), []);
});

test("a section title keeps all its rows", () => {
  const rows = [{ keys: "a", does: "Accept" }, { keys: "x", does: "Dismiss" }];
  const sections = [{ title: "Review", rows }, { title: "Navigation", rows: [{ keys: "r", does: "Review" }] }];
  assert.deepEqual(filterShortcuts(sections, "review"), sections);
});

test("zoom is under General", () => {
  const general = SHORTCUTS.find(s => s.title === "General")!;
  assert.deepEqual(general.rows.map(r => [r.keys, r.does]), [["⌘= ⌘-", "Zoom in, out"], ["⌘0", "Actual size"]]);
  assert.deepEqual(rows("zoom"), ["Zoom in, out", "Zoom the pane"]);
  assert.deepEqual(rows("dictate"), ["Dictate while held"]);
});
