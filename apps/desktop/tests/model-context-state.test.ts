import test from "node:test";
import assert from "node:assert/strict";
import { modelDraft, modelProblem, modelRules } from "../src/stores/settings/model-context-state.ts";

test("model thresholds accept percentages, tokens and off; clearing inherits", () => {
  const draft = [
    { model: "model-large", threshold: " 240000 " },
    { model: "model-small", threshold: "60%" },
    { model: "model-manual", threshold: "off" },
    { model: "model-inherited", threshold: "" },
  ];
  assert.equal(modelProblem(draft), "");
  const rules = modelRules(draft);
  assert.deepEqual(rules.map(r => r.compact), [
    { enabled: true, percent: null, tokens: 240000 },
    { enabled: true, percent: 60, tokens: null },
    { enabled: false, percent: null, tokens: null },
  ]);
  assert.deepEqual(modelRules(modelDraft(rules)), rules);
  assert.deepEqual(modelRules([{ model: "model-large", threshold: "" }]), []);
});

test("invalid and duplicate model overrides cannot be saved", () => {
  for (const threshold of ["0", "101%", "no", "9007199254740992", "-1", "1.5"]) {
    assert.notEqual(modelProblem([{ model: "model", threshold }]), "", threshold);
  }

  assert.match(modelProblem([{ model: "", threshold: "50%" }]), /model id/);
  assert.match(modelProblem([{ model: "m", threshold: "50%" }, { model: " m ", threshold: "off" }]), /twice/);
});
