import test from "node:test";
import assert from "node:assert/strict";
import { sendReview } from "../src/stores/review/diff-load.ts";

test("a review send that rejects frees the Send button and shows why", async () => {
  const state: Record<string, unknown> = { sending: false, error: "", view: null, draft: [{ path: "a.ts", side: "new", line: 1, text: "x" }] };
  const v = {
    state,
    set: (change: Record<string, unknown>) => Object.assign(state, change),
    src: { review: { key: "k", heading: () => "Review", send: async () => { throw new Error("link lost"); } } },
    root: { focus() {} },
  };

  await sendReview(v as never).catch(() => {});
  assert.equal(state.sending, false);
  assert.match(String(state.error), /link lost/);
});
