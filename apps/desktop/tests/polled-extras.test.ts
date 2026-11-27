import "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { forgetUnfollowedExtras } from "../src/parts/session/session-follow.ts";
import { transcriptKey, transcripts } from "../src/stores/agents/transcript-cache-state.ts";

test("the modes and tasks of a session read by polling go when it leaves the screen", () => {
  const o = { machine_id: "m1", agent: "codex", session: "polled" };
  const key = transcriptKey(o);
  transcripts.setModes(key, { current: "plan", modes: [{ id: "plan", name: "Plan" }], plan: "" });
  transcripts.setTasks(key, []);
  assert.equal(transcripts.modes(key)?.current, "plan");

  forgetUnfollowedExtras(o);

  assert.equal(transcripts.modes(key), null);
});
