import test from "node:test";
import assert from "node:assert/strict";
import { keepWriting, parseReview, placeWords, reviewCount, reviewPrompt, type Review, type ReviewComment } from "../src/agents/review-state.ts";

const comment = (id: number, ask: ReviewComment["ask"], place: string, quote: string, text: string): ReviewComment => ({ id, entry: 4, quote, place, opening: "Done: the redirect keeps the query string.", ask, text });

test("a review's message reads back as its comments", () => {
  const comments = [comment(1, "change", "Changed 1", "it was dropped when next was read.", "Also keep the fragment."), comment(2, "question", "Headline", "keeps the query", "Even on errors?"), comment(3, "question", "", "a\nb", "Why?")];
  const prompt = reviewPrompt(comments, "Then open a PR.");
  assert.match(prompt, /^Review comments on what you wrote\.\n\nReply with a report/);
  assert.match(prompt, /## Questions\nAnswer each, quoting it\. Change nothing for these\.\n\nIn your message that starts "Done: the redirect keeps the query string\.", its first line:\n> keeps the query\nEven on errors\?\n/);
  assert.match(prompt, /, item 1 under "Changed":\n> it was dropped when next was read\.\nAlso keep the fragment\.\n/);
  assert.match(prompt, /## Also\n\nThen open a PR\.\n$/);

  const sent = parseReview(prompt);
  assert.deepEqual(sent?.items.map(i => [i.ask, i.place, i.quote, i.text]), [
    ["question", "Headline", "keeps the query", "Even on errors?"],
    ["question", "", "a\nb", "Why?"],
    ["change", "Changed 1", "it was dropped when next was read.", "Also keep the fragment."],
  ]);
  assert.equal(sent?.also, "Then open a PR.");
  assert.equal(parseReview("Fix the login test"), null);
});

test("a place reads as a place, not a heading", () => {
  assert.equal(placeWords("Needs you 2"), 'item 2 under "Needs you"');
  assert.equal(placeWords("Summary"), 'under "Summary"');
  assert.equal(placeWords(""), "");
});

test("saving a comment adds it, changes it, or drops it when emptied", () => {
  const base: Review = { comments: [comment(1, "change", "", "x", "Fix x")], writing: null, listed: false };
  const added = keepWriting({ ...base, writing: { ...comment(0, "question", "", "y", " Why y? "), id: null } });
  assert.deepEqual(added.comments.map(c => [c.id, c.text]), [[1, "Fix x"], [2, "Why y?"]]);
  assert.equal(added.writing, null);
  const changed = keepWriting({ ...added, writing: { ...added.comments[0], text: "Fix x now" } });
  assert.deepEqual(changed.comments.map(c => c.text), ["Fix x now", "Why y?"]);
  const dropped = keepWriting({ ...changed, writing: { ...changed.comments[0], text: "  " } });
  assert.deepEqual(dropped.comments.map(c => c.id), [2]);
  assert.equal(reviewCount(added.comments), "1 question · 1 change");
});
