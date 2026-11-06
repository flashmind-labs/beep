import test from "node:test";
import assert from "node:assert/strict";
import { START, answerLabel, canSend, issueHint, next, sizeLabel, type Draft, type ReportState } from "../src/report/report-state.ts";

const manifest = {
  version: 1, sender: "3f9a2c", mesh: null, app: "desktop", app_version: "0.2.0", text: "", period: "today", since: 0, made_at: 0,
  machines: [{ id: "a1", name: "build-box", kind: "node", answer: "Answered", version: "0.4.1", os: "linux", os_version: "6.8" }],
  files: [{ path: "build-box/log.txt", bytes: 421888 }],
} as const;
const draft: Draft = { manifest: { ...manifest, machines: [...manifest.machines], files: [...manifest.files] }, files: [{ path: "build-box/log.txt", text: "INFO node started" }] };

function run(events: Parameters<typeof next>[1][]): ReportState {
  return events.reduce(next, START);
}

test("defaults to today", () => {
  assert.equal(START.period, "today");
  assert.equal(START.step, "form");
  assert.equal(next(START, { type: "period", period: "hour" }).period, "hour");
});

test("collect then send then sent offers github only with gh", () => {
  for (const gh of [false, true]) {
    const s = run([{ type: "text", text: "The sync stalls" }, { type: "collect" }, { type: "collected", draft }, { type: "send" }, { type: "sent", reference: "R-AB", gh }]);
    assert.equal(s.step, "sent");
    assert.equal(s.gh, gh);
    assert.equal(s.reference, "R-AB");
    assert.equal(s.text, "The sync stalls");
  }
  const s = run([{ type: "collect" }, { type: "collected", draft }]);
  assert.equal(s.step, "files");
  assert.equal(s.draft, draft);
  assert.equal(next(s, { type: "send" }).step, "sending");
  const issued = next(run([{ type: "collect" }, { type: "collected", draft }, { type: "send" }, { type: "sent", reference: "R-AB", gh: true }]), { type: "issued", url: "https://github.com/x/y/issues/1" });
  assert.equal(issued.step, "issue");
  assert.equal(issued.issueUrl, "https://github.com/x/y/issues/1");
});

test("a refusal shows the server's words", () => {
  const words = "This computer's clock is off by 12 minutes; set it right and send again.";
  const s = run([{ type: "collect" }, { type: "collected", draft }, { type: "send" }, { type: "failed", error: words }]);
  assert.equal(s.step, "failed");
  assert.equal(s.error, words);
  // Back keeps the draft to send again.
  const back = next(s, { type: "back" });
  assert.equal(back.step, "files");
  assert.equal(back.draft, draft);
  assert.equal(back.error, undefined);
  // A collection that failed goes back to the form.
  assert.equal(next(run([{ type: "collect" }, { type: "failed", error: "offline" }]), { type: "back" }).step, "form");
});

test("an issue that timed out says to check GitHub first", () => {
  const sent = run([{ type: "collect" }, { type: "collected", draft }, { type: "send" }, { type: "sent", reference: "R-1", gh: true }]);
  const timedOut = next(sent, { type: "failed", error: "the report was sent, but the GitHub issue did not open: gh did not finish in time" });
  assert.equal(issueHint(timedOut), "Check GitHub before trying again: the issue may have opened.");
  assert.equal(issueHint(next(sent, { type: "failed", error: "gh failed: auth required" })), null);
  // A send that timed out has no issue to check.
  assert.equal(issueHint(run([{ type: "collect" }, { type: "collected", draft }, { type: "send" }, { type: "failed", error: "did not finish in time" }])), null);
});

test("labels", () => {
  assert.equal(answerLabel("Answered"), "answered");
  assert.equal(answerLabel("Silent"), "did not answer");
  assert.equal(answerLabel({ Offline: { reason: "offline" } }), "offline");
  assert.equal(answerLabel("OlderNode"), "older node");
  assert.equal(answerLabel({ Failed: { reason: "no log" } }), "failed: no log");
  assert.equal(sizeLabel(421888), "412 KB");
  assert.equal(sizeLabel(1_258_291), "1.2 MB");
  assert.equal(sizeLabel(512), "512 B");
});

test("cannot send an empty draft", () => {
  const empty: Draft = { manifest: draft.manifest, files: [] };
  const s = run([{ type: "collect" }, { type: "collected", draft: empty }]);
  assert.equal(canSend(s), false);
  assert.equal(next(s, { type: "send" }), s);
  assert.equal(canSend(run([{ type: "collect" }, { type: "collected", draft }])), true);
  assert.equal(canSend(START), false);
});
