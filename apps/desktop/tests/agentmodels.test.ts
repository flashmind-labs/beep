import test from "node:test";
import assert from "node:assert/strict";
import { type AgentModels, modelField, modelMachine } from "../src/bar/agentmodels.ts";

const listed: AgentModels = {
  pick: "listed",
  current: "fast",
  models: [
    { id: "fast", name: "Fast", description: null },
    { id: "deep", name: "Deep", description: "Slower, more careful" },
  ],
};

test("a listed agent gets a dropdown with its default first", () => {
  const field = modelField(listed);
  assert.equal(field.kind, "list");
  if (field.kind !== "list") return;
  assert.deepEqual(field.choices.map(c => [c.value, c.label]), [["", "Default (Fast)"], ["fast", "Fast"], ["deep", "Deep"]]);
  assert.equal(field.choices[2].title, "deep: Slower, more careful");
});

test("an agent that lists none takes a typed name, its default as the placeholder", () => {
  assert.deepEqual(modelField({ pick: "named", current: "openai:gpt-test", models: [] }),
    { kind: "text", placeholder: "Default (openai:gpt-test)" });
  assert.deepEqual(modelField({ pick: "named", current: null, models: [] }), { kind: "text", placeholder: "Default" });
});

test("the field hides when none can be picked or the machine cannot say", () => {
  assert.deepEqual(modelField({ pick: "fixed", current: null, models: [] }), { kind: "hidden" });
  assert.deepEqual(modelField(null), { kind: "hidden" });
  assert.deepEqual(modelField(undefined), { kind: "loading" });
});

test("models are listed on the picked machine, or where Auto would start", () => {
  assert.equal(modelMachine("m1", null), "m1");
  assert.equal(modelMachine("auto", "m2"), "m2");
  assert.equal(modelMachine("auto", null), null);
});
