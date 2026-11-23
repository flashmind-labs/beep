import test from "node:test";
import assert from "node:assert/strict";
import { challengeDefault, challengePrompt, findingDraft, parseFindings } from "../src/agents/challenge-state.ts";
import { parseReview, reviewPrompt } from "../src/agents/review-state.ts";

const challenger = `Partly done: one claim broke, two held.

## Needs you

1. **[confirmed] Changed 1**
   > it was dropped when \`next\` was read.
   It is still dropped on a redirect from the old sign-on page.
   Evidence: \`npm test -- login\` with an SSO fixture failed 1 of 1.
2. **[plausible] Headline**
   > The redirect keeps the query string.
   A fragment is lost.
3. Keep the old route for a release.

## Checked
- **Checked 1:** held. Ran \`npm test\`, 48 pass.`;

test("a challenger's findings are read from its Needs you items", () => {
  assert.deepEqual(parseFindings(challenger), [
    { tag: "confirmed", place: "Changed 1", quote: "it was dropped when next was read.", text: "It is still dropped on a redirect from the old sign-on page.", evidence: "`npm test -- login` with an SSO fixture failed 1 of 1." },
    { tag: "plausible", place: "Headline", quote: "The redirect keeps the query string.", text: "A fragment is lost.", evidence: "" },
  ]);
  assert.deepEqual(parseFindings("Done: nothing broke.\n\n## Checked\n- **Changed 1:** held."), []);
});

test("a finding goes to the agent first, as a challenge finding", () => {
  const [finding] = parseFindings(challenger);
  const draft = findingDraft(finding, 3, 7, "Done: the redirect keeps the query string.");
  assert.equal(draft.ask, "finding");
  assert.equal(draft.text, "Confirmed: It is still dropped on a redirect from the old sign-on page.\nEvidence: `npm test -- login` with an SSO fixture failed 1 of 1.");

  const prompt = reviewPrompt([{ ...draft, id: 1, ask: "question", text: "Why?" }, draft], "");
  assert.ok(prompt.indexOf("## Challenge findings") < prompt.indexOf("## Questions"), prompt);
  assert.match(prompt, /## Challenge findings\nAnother agent tried to prove your report wrong/);
  assert.deepEqual(parseReview(prompt)?.items.map(i => [i.ask, i.place]), [["finding", "Changed 1"], ["question", "Changed 1"]]);
});

test("a challenger is told the skill, where the work is and the report", () => {
  const prompt = challengePrompt("Done: it works.\n", "/srv/project");
  assert.match(prompt, /Beep's `challenge` skill/);
  assert.match(prompt, /its work in `\/srv\/project`; you run in a throwaway copy/);
  assert.match(prompt, /<report>\nDone: it works\.\n<\/report>$/);
});

test("a challenge opens on another harness than the session's, then on the last pick", () => {
  assert.equal(challengeDefault(["claude", "codex"], "claude", null).agent, "codex");
  assert.equal(challengeDefault(["claude", "codex"], "codex", null).agent, "claude");
  assert.equal(challengeDefault(["codex"], "codex", null).agent, "codex", "the only one");
  const last = { agent: "claude", model: "opus", effort: "high", fast: null };
  assert.deepEqual(challengeDefault(["claude", "codex"], "claude", last), last);
  assert.equal(challengeDefault(["codex"], "codex", last).agent, "codex", "not offered here");
});
