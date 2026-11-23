import test from "node:test";
import assert from "node:assert/strict";
import { challengePrompt, challengeResult, challengeStep, findingDraft, parseFindings } from "../src/agents/challenge-state.ts";
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

test("a challenger's session status moves its challenge on", () => {
  for (const status of ["starting", "working", "running"]) assert.equal(challengeStep(status, false), "running", status);
  for (const status of ["asking", "waiting"]) assert.equal(challengeStep(status, false), "asking", status);
  for (const status of ["idle", "ended"]) assert.equal(challengeStep(status, false), "finished", status);
  assert.equal(challengeStep(null, true), "running", "a start its machine has not named yet");
  assert.equal(challengeStep(null, false), "gone", "a named session no longer listed");
});

test("whatever a finished challenger answered is its result; without an answer it failed", () => {
  assert.deepEqual(challengeResult(challenger, null), { state: "done", findings: parseFindings(challenger) });
  assert.deepEqual(challengeResult(`Two claims broken. Report:\n\n${challenger}`, null), { state: "done", findings: parseFindings(challenger) });
  assert.deepEqual(challengeResult("Every claim held.", null), { state: "done", findings: [] });
  assert.deepEqual(challengeResult(null, null), { state: "failed", error: "It stopped without answering." });
  assert.deepEqual(challengeResult("  ", "codex is not signed in "), { state: "failed", error: "codex is not signed in" });
});

test("findings are read as serpis reads them", () => {
  const report = "Done: two claims don't hold.\n\n## Needs you\n\n1. **[confirmed] Changed 1**\n   > This is the likely cause of the flaky `retries_respect_limit` test in CI.\n   The race is still there: `push_front` runs first.\n   It reads the old count.\n   Evidence: 8 workers failed 3 of 500 runs.\n2. **[Plausible] Checked 2:** The loop ran on one thread.\n   > passed every time.\n3. Something else, not a finding.\n   > not a quote\n\n## Checked\n\n- **Changed 2:** held. Read the backoff.\n";
  assert.deepEqual(parseFindings(report), [
    { tag: "confirmed", place: "Changed 1", quote: "This is the likely cause of the flaky retries_respect_limit test in CI.", text: "The race is still there: `push_front` runs first. It reads the old count.", evidence: "8 workers failed 3 of 500 runs." },
    { tag: "plausible", place: "Checked 2", quote: "passed every time.", text: "The loop ran on one thread.", evidence: "" },
  ]);
  assert.deepEqual(parseFindings("- **[confirmed]** Headline\n"), [], "outside Needs you");
  assert.deepEqual(parseFindings("## Needs you\n- [CONFIRMED] Headline\n  > it works\n  **Evidence:** ran it,\n  twice.").map(f => [f.tag, f.place, f.quote, f.evidence]), [["confirmed", "Headline", "it works", "ran it, twice."]]);
});

test("a real challenger's answer, a line of its own before its report, gives its findings", () => {
  const said = "Two claims broken. Report:\n\nDone: found 2 broken claims — the checks do run (all pass), and the import count is 65 files, not 68.\n\n## Needs you\n\n1. **[confirmed] Headline**\n   > but the rename isn't compiled, since there's no Go toolchain on this machine\n\n   Go 1.27.4 is on PATH and everything passes.\n   Evidence: `go vet ./...` exit 0, `go test ./...` all 6 packages ok.\n\n2. **[confirmed] Changed 1**\n   > Every import in 68 files\n\n   65 files change their imports, not 68.\n   Evidence: `git show HEAD` lists 65 files with changed import lines.\n\n## Checked\n\n- **Changed 2:** held.";
  assert.deepEqual(parseFindings(said).map(f => [f.tag, f.place, f.quote, f.text]), [
    ["confirmed", "Headline", "but the rename isn't compiled, since there's no Go toolchain on this machine", "Go 1.27.4 is on PATH and everything passes."],
    ["confirmed", "Changed 1", "Every import in 68 files", "65 files change their imports, not 68."],
  ]);
});

