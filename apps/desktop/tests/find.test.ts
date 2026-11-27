import { test } from "node:test";
import assert from "node:assert/strict";
import { nextMatch, searchPattern, textMatches } from "../src/stores/chat/find-state.ts";

test("find is literal and case insensitive", () => {
  assert.deepEqual(textMatches("A.b a.B axb", "a.b"), [{ start: 0, end: 3 }, { start: 4, end: 7 }]);
  assert.deepEqual(textMatches("[a] [a]", "[a]"), [{ start: 0, end: 3 }, { start: 4, end: 7 }]);
});

test("empty queries and missing text have no matches", () => {
  assert.deepEqual(textMatches("hello", ""), []);
  assert.deepEqual(textMatches("hello", "absent"), []);
});

test("matches keep original Unicode offsets", () => {
  assert.deepEqual(textMatches("İ café CAFÉ 🐝", "café"), [{ start: 2, end: 6 }, { start: 7, end: 11 }]);
  assert.deepEqual(textMatches("İİ", "İ"), [{ start: 0, end: 1 }, { start: 1, end: 2 }]);
});

test("occurrences do not overlap", () => {
  assert.deepEqual(textMatches("aaaaa", "aa"), [{ start: 0, end: 2 }, { start: 2, end: 4 }]);
});

test("next and previous wrap at both ends", () => {
  assert.equal(nextMatch(2, 3, 1), 0);
  assert.equal(nextMatch(0, 3, -1), 2);
  assert.equal(nextMatch(0, 0, 1), -1);
  assert.equal(nextMatch(-1, 1, 1), 0);
});

test("regex supports alternation, groups, quantifiers and case insensitive matching", () => {
  assert.deepEqual(textMatches("build TEST testing", searchPattern("build|test(?:ing)?", true)), [
    { start: 0, end: 5 }, { start: 6, end: 10 }, { start: 11, end: 18 },
  ]);
  assert.deepEqual(textMatches("item12 item3", searchPattern("item[0-9]{2}", true)), [{ start: 0, end: 6 }]);
});

test("regex anchors match individual lines", () => {
  assert.deepEqual(textMatches("first\nsecond\nthird", searchPattern("^second$", true)), [{ start: 6, end: 12 }]);
});

test("regex errors are reported and the same input stays literal by default", () => {
  assert.throws(() => searchPattern("[", true), SyntaxError);
  assert.deepEqual(textMatches("[literal]", searchPattern("[")), [{ start: 0, end: 1 }]);
  assert.equal(searchPattern("", true), null);
});

test("zero length regex matches are skipped without losing later text matches", () => {
  assert.deepEqual(textMatches("word", searchPattern("^|$|(?=o)", true)), []);
  assert.deepEqual(textMatches("ab", searchPattern("a*", true)), [{ start: 0, end: 1 }]);
  assert.deepEqual(textMatches("🐝🐝", searchPattern("(?=.)", true)), []);
});

test("compiled regexes can be reused without carrying their search position", () => {
  const pattern = searchPattern("b.+?d", true);
  assert.deepEqual(textMatches("build", pattern), [{ start: 0, end: 5 }]);
  assert.deepEqual(textMatches("BUILD", pattern), [{ start: 0, end: 5 }]);
});
