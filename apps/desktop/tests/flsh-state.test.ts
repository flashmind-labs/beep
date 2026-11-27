import test from "node:test";
import assert from "node:assert/strict";
import { EMPTY, NEW_MODEL, flshChanged, flshDraft, flshInput, flshProblem, toggleQuantization, type FlshSettings } from "../src/stores/settings/flsh-state.ts";

const saved: FlshSettings = {
  zdr: true,
  stream_idle_timeout_secs: 120,
  quantizations: ["bf16", "fp8"],
  sort: "throughput",
  allow_fallbacks: false,
  autocompact: { enabled: true, tokens: 300_000 },
  models: [
    { model: "openrouter:vendor/model", inherits_compaction: false, enabled: false, tokens: null, quantizations: null },
    { model: "openrouter:vendor/fp4-only", inherits_compaction: true, enabled: true, tokens: null, quantizations: ["fp4"] },
  ],
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
  for (const idle_seconds of ["0", "-1", "2.5", "5m", "4294967296"]) {
    assert.match(flshProblem({ ...d, idle_seconds }), /stream idle timeout/);
  }
  assert.match(flshProblem({ ...d, compact_tokens: "10" }), /token count/);
  assert.match(flshProblem({ ...d, models: [{ ...NEW_MODEL, model: "vendor/model" }] }), /provider:model/);
  assert.match(flshProblem({ ...d, models: [...d.models, { ...NEW_MODEL, model: "openrouter:vendor/model" }] }), /twice/);
  assert.match(flshProblem({ ...d, models: [{ ...NEW_MODEL, model: "openrouter:x", compact: "on", tokens: "abc" }] }), /openrouter:x/);
  // A threshold left from an own rule does not count once it inherits.
  assert.equal(flshProblem({ ...d, models: [{ ...NEW_MODEL, model: "openrouter:x", tokens: "abc" }] }), "");
  assert.match(flshProblem({ ...d, models: [{ ...NEW_MODEL, model: "anthropic:x", quantizations: [] }] }), /not an OpenRouter model/);
});

test("a model's own settings save as it set them", () => {
  const d = flshDraft(EMPTY);
  const input = flshInput({ ...d, models: [
    { ...NEW_MODEL, model: " openrouter:vendor/a ", tokens: "90k", quantizations: [] },
    { ...NEW_MODEL, model: "openrouter:vendor/b", compact: "on", tokens: "90k", quantizations: toggleQuantization(["fp8"], "fp4") },
    { ...NEW_MODEL, model: "openrouter:vendor/c", compact: "off" },
  ] });
  assert.deepEqual(input.models, [
    { model: "openrouter:vendor/a", inherits_compaction: true, enabled: true, tokens: null, quantizations: [] },
    { model: "openrouter:vendor/b", inherits_compaction: false, enabled: true, tokens: 90_000, quantizations: ["fp4", "fp8"] },
    { model: "openrouter:vendor/c", inherits_compaction: false, enabled: false, tokens: null, quantizations: null },
  ]);
});
