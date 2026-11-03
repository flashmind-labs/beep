import test from "node:test";
import assert from "node:assert/strict";
import { EMPTY, flshChanged, flshDraft, flshInput, flshProblem, type FlshSettings } from "../src/flsh-state.ts";

const saved: FlshSettings = {
  zdr: true,
  quantizations: ["bf16", "fp8"],
  sort: "throughput",
  allow_fallbacks: false,
  autocompact: { enabled: true, tokens: 300_000 },
  models: [{ model: "openrouter:vendor/model", enabled: false, tokens: null }],
};

test("saved settings round trip through the draft", () => {
  const d = flshDraft(saved);
  // Shown and saved in the page's order.
  assert.deepEqual(d.quantizations, ["fp8", "bf16"]);
  assert.deepEqual(flshInput(d), { ...saved, quantizations: ["fp8", "bf16"] });
  assert.equal(flshChanged(saved, d), false);
  assert.equal(flshChanged(saved, { ...d, sort: null }), true);
});

test("nothing set stays nothing set", () => {
  const d = flshDraft(EMPTY);
  assert.equal(d.compact_on, true);
  assert.equal(d.compact_tokens, "");
  // On at the model's trigger is flsh's default: left unset.
  assert.deepEqual(flshInput(d), EMPTY);
  assert.deepEqual(flshInput({ ...d, compact_on: false }).autocompact, { enabled: false, tokens: null });
});

test("bad values are named before saving", () => {
  const d = flshDraft(saved);
  assert.equal(flshProblem(d), "");
  assert.match(flshProblem({ ...d, compact_tokens: "10" }), /token count/);
  assert.match(flshProblem({ ...d, models: [{ model: "vendor/model", enabled: true, tokens: "" }] }), /provider:model/);
  assert.match(flshProblem({ ...d, models: [...d.models, { model: "openrouter:vendor/model", enabled: true, tokens: "" }] }), /twice/);
  assert.match(flshProblem({ ...d, models: [{ model: "openrouter:x", enabled: true, tokens: "abc" }] }), /openrouter:x/);
});
