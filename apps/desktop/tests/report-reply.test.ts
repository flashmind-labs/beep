import test from "node:test";
import assert from "node:assert/strict";
import { htmlToDOM, type Element, type DOMNode } from "html-react-parser";
import { checkOf, claimWords, outcomeOf, readReport, withHeld, readingTime, summaryAsked, summaryPrompt, toggleOriginal, reportViews } from "../src/agents/report-state.ts";

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

test("Summarize names the report by how it starts", () => {
  const prompt = summaryPrompt("\n**Done:** retries work.\n\n## Changed\n- x");
  assert.match(prompt, /^Summarize your report that starts "Done: retries work\." even further/);
  assert.match(summaryPrompt(`Done: ${"word ".repeat(30)}`), /starts "Done: (word ){10}word…"/);
});

test("a Summarize message is told apart and names its report", () => {
  const report = `**Done:** ${"long ".repeat(20)}`;
  assert.equal(summaryAsked(summaryPrompt(report)), `Done: ${"long ".repeat(11).trim()}…`);
  assert.equal(summaryAsked("Summarize the logs"), null);
});

test("a report shows its summary until its original is asked for", () => {
  assert.equal(reportViews.getState().original.has("s:4"), false);
  toggleOriginal("s:4");
  assert.equal(reportViews.getState().original.has("s:4"), true);
  toggleOriginal("s:4");
  assert.equal(reportViews.getState().original.has("s:4"), false);
});

test("a report may open with one short paragraph of the agent's own before its outcome", () => {
  const r = readReport(htmlToDOM("<p>Two claims broken. Report:</p>\n<p>Done: found 2 broken claims. The checks do run.</p>\n<h2>Needs you</h2><ol><li>Fix it.</li></ol>"));
  assert.ok(r);
  assert.equal(r.outcome, "done");
  assert.equal(text(r.lead), "Two claims broken. Report:");
  assert.equal(text(r.headline), "Found 2 broken claims.");
  assert.deepEqual(r.sections.map(s => [s.title, s.count]), [["Needs you", 1]]);
  assert.deepEqual(readReport(htmlToDOM("<p>Done: it works.</p>"))?.lead, [], "no lead without one");

  // Only one short paragraph: a long one, two, or a heading first is no report.
  const long = `${"word ".repeat(31)}.`;
  assert.equal(readReport(htmlToDOM(`<p>${long}</p><p>Done: it works.</p>`)), null);
  assert.equal(readReport(htmlToDOM("<p>One.</p><p>Two.</p><p>Done: it works.</p>")), null);
  assert.equal(readReport(htmlToDOM("<h2>Summary</h2><p>Done: it works.</p>")), null);
  assert.equal(readReport(htmlToDOM("<p>Looked around.</p><p>Nothing to report.</p>")), null);
});

test("a claim is found by its place: the headline, or the nth item of the section it names", () => {
  const r = readReport(htmlToDOM("<p>Done: the redirect keeps the query. More here.</p>\n<h2>Changed</h2>\n<ul><li><strong>Kept</strong> (<code>a.ts:4</code>): it was dropped. Now it isn't.</li><li>Second.</li></ul>\n<h2>Needs you</h2>\n<ol><li>Old links.</li></ol>"));
  assert.ok(r);
  assert.equal(claimWords(r, "Headline"), "The redirect keeps the query.");
  assert.equal(claimWords(r, "Changed 1"), "Kept (a.ts:4): it was dropped.");
  assert.equal(claimWords(r, "changed 2"), "Second.");
  assert.equal(claimWords(r, "Needs you 1"), "Old links.");
  assert.equal(claimWords(r, "Changed 3"), null);
  assert.equal(claimWords(r, "Checked 1"), null);
  assert.equal(claimWords(r, "somewhere"), null);
});

test("the claims a challenge could not break are marked held", () => {
  const r = readReport(htmlToDOM("<p>Done: it works.</p>\n<h2>Changed</h2>\n<ul><li>One.</li><li>Two.</li></ul>"));
  assert.ok(r);
  const none = withHeld(r, []);
  assert.equal(none.sections, r.sections);
  assert.equal(none.headlineHeld, false);

  const held = withHeld(r, ["Headline", "Changed 2", "Checked 9"]);
  assert.equal(held.headlineHeld, true);
  assert.equal(html(held.sections[0].nodes).replace(/\n/g, ""), '<ul><li>One.</li><li data-held="">Two.</li></ul>');
  assert.equal(html(r.sections[0].nodes).replace(/\n/g, ""), "<ul><li>One.</li><li>Two.</li></ul>");
});
