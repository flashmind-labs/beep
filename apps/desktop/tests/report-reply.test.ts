import test from "node:test";
import assert from "node:assert/strict";
import { htmlToDOM, type Element, type DOMNode } from "html-react-parser";
import { checkOf, outcomeOf, readReport, readingTime } from "../src/agents/report-state.ts";

const text = (nodes: DOMNode[]): string => nodes.map(n => n.type === "text" ? n.data : n.type === "tag" ? text(n.children as DOMNode[]) : "").join("");
const html = (nodes: DOMNode[]): string => nodes.map(n => n.type === "text" ? n.data : n.type === "tag" ? `<${n.name}${Object.entries(n.attribs).map(([k, v]) => ` ${k}="${v}"`).join("")}>${html(n.children as DOMNode[])}</${n.name}>` : "").join("");

test("a report's first line gives its outcome, then its headline and the rest of the opening", () => {
  const r = readReport(htmlToDOM("<p><strong>Done:</strong> retries work, but <code>a. b</code> counts twice. Fix it first.</p>\n<p>More.</p>"));
  assert.ok(r);
  assert.equal(r.outcome, "done");
  assert.equal(html(r.headline), "Retries work, but <code>a. b</code> counts twice.");
  assert.equal(html(r.intro).replace(/\n/g, ""), "<p>Fix it first.</p><p>More.</p>");
  assert.deepEqual(r.sections, []);
});

test("its sections are found by their headings, with their items counted and checks marked", () => {
  const r = readReport(htmlToDOM([
    "<p>Partly done: half of it.</p>",
    "<h2>Needs you</h2><ol><li>Pick one.</li><li>Approve.</li></ol>",
    "<h2>Checked</h2><ul><li><strong><code>cargo test</code>:</strong> passes (2 tests).</li><li><strong>Race:</strong> fails 3 of 500.</li><li><strong>Not checked:</strong> live.</li><li>Read the code.</li></ul>",
  ].join("\n")));
  assert.ok(r);
  assert.equal(r.outcome, "partly");
  assert.equal(text(r.headline), "Half of it.");
  assert.deepEqual(r.sections.map(s => [s.title, s.kind, s.count]), [["Needs you", "needs", 2], ["Checked", "checked", 4]]);
  const checks = r.sections[1].nodes.filter((n): n is Element => n.type === "tag").flatMap(list => list.children)
    .filter((n): n is Element => n.type === "tag").map(li => li.attribs["data-check"] ?? null);
  assert.deepEqual(checks, ["pass", "fail", "unchecked", null]);
});

test("only an outcome and its colon open a report", () => {
  assert.equal(outcomeOf("Blocked — no access.")?.outcome, "blocked");
  assert.equal(outcomeOf("Found it: the key ignores the locale.")?.outcome, "done");
  assert.equal(outcomeOf("Done in the lock: 0.5.16.")?.outcome, "done");
  assert.equal(outcomeOf("Blocked on vet: it needs your call.")?.outcome, "blocked");
  assert.equal(outcomeOf("Found three files in src: a, b and c."), null);
  assert.equal(outcomeOf("Doneness is relative: it depends."), null);
  assert.equal(outcomeOf("Done with the parser and then a lot more words here: x"), null);
  assert.equal(outcomeOf("I've finished the review."), null);
  assert.equal(readReport(htmlToDOM("<h2>Done</h2><p>x</p>")), null);
});

test("a check reads its result after the colon", () => {
  assert.equal(checkOf("cargo test: passes (214 tests)."), "pass");
  assert.equal(checkOf("Regression test: failed before the fix, passes after."), "fail");
  assert.equal(checkOf("Not tested on iOS."), "unchecked");
  assert.equal(checkOf("Read the diff."), null);
});

test("a report says its length and reading time", () => {
  assert.deepEqual(readingTime("a b c"), { words: 3, minutes: 1 });
  assert.deepEqual(readingTime("w ".repeat(460)), { words: 460, minutes: 2 });
});
