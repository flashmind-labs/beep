import test from "node:test";
import assert from "node:assert/strict";
import { formatAgo, reason, type AgoStyle } from "../src/ui/format.ts";

process.env.TZ = "UTC";

// Thursday, 15 January 2026, 12:00 UTC.
const NOW = Date.UTC(2026, 0, 15, 12, 0, 0);
const SEC = 1000, MIN = 60 * SEC, HOUR = 60 * MIN, DAY = 24 * HOUR;
const date = (ms: number) => new Date(NOW - ms).toLocaleDateString([], { day: "numeric", month: "short" });

// Each row: the style, how long ago, and the exact words. Places show these
// strings, so they must not drift.
const TABLE: [AgoStyle, number, string][] = [
  ["short", 30 * SEC, "now"],
  ["short", 12 * MIN, "12 min"],
  ["short", 3 * HOUR, "3 h"],
  ["short", 2 * DAY, "2 d"],
  ["since", 30 * SEC, "just now"],
  ["since", 12 * MIN, "12 min ago"],
  ["since", 3 * HOUR, "3 h ago"],
  ["since", 2 * DAY, "2 d ago"],
  ["long", 30 * SEC, "just now"],
  ["long", 12 * MIN, "12 min ago"],
  ["long", 3 * HOUR, "3 h ago"],
  ["long", DAY, "yesterday"],
  ["long", 3 * DAY, "3 days ago"],
  ["long", 10 * DAY, "last week"],
  ["long", 21 * DAY, "3 weeks ago"],
  ["rough", 30 * SEC, "under 2 min"],
  ["rough", 3 * MIN, "3 min"],
  ["rough", 2 * HOUR, "2 h"],
  ["rough", 4 * DAY, "4 days"],
  ["chat", 20 * SEC, "now"],
  ["chat", 40 * SEC, "1 min ago"],
  ["chat", 12 * MIN, "12 min ago"],
  ["chat", 3 * HOUR, "3 h ago"],
  ["chat", DAY, "Yesterday"],
  ["chat", 10 * DAY, date(10 * DAY)],
  ["day", 3 * HOUR, "09:00"],
  ["day", DAY, "Yesterday"],
  ["day", 3 * DAY, "Mon"],
  ["day", 10 * DAY, date(10 * DAY)],
];

test("formatAgo says each style's exact words", () => {
  for (const [style, ago, words] of TABLE) assert.equal(formatAgo(NOW - ago, style, NOW), words, `${style} ${ago} ms ago`);
});

test("a time in the future reads as now", () => {
  assert.equal(formatAgo(NOW + MIN, "short", NOW), "now");
  assert.equal(formatAgo(NOW + MIN, "long", NOW), "just now");
});

test("reason drops the bridge prefix of a string, an Error or an object", () => {
  assert.equal(reason("bad request: The message is empty."), "The message is empty.");
  assert.equal(reason(new Error("not found: no such file")), "no such file");
  assert.equal(reason({ message: "Conflict: busy" }), "busy");
  assert.equal(reason("plain"), "plain");
});
