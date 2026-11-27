import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { keepProjectPick, lastProjectPick } from "../src/stores/agents/agentmodels.ts";

const dom = new JSDOM("", { url: "https://app.test" });
Object.defineProperty(globalThis, "localStorage", { value: dom.window.localStorage, configurable: true });

const codex = { agent: "codex", model: "model-a", effort: "high", fast: null };
const claude = { agent: "claude", model: "model-b", effort: "max", fast: null };

test("projects retain separate harness, model and thinking choices across reads", () => {
  localStorage.clear();
  keepProjectPick("org/first", codex);
  keepProjectPick("org/second", claude);
  assert.deepEqual(lastProjectPick("org/first"), codex);
  assert.deepEqual(lastProjectPick("org/second"), claude);
  assert.equal(lastProjectPick("org/new"), null);

  keepProjectPick("org/first", { ...codex, model: "", effort: null, fast: null });
  assert.deepEqual(lastProjectPick("org/first"), { ...codex, model: "", effort: null, fast: null });
  assert.deepEqual(lastProjectPick("org/second"), claude);
});

test("the form without a project has a separate preference", () => {
  localStorage.clear();
  keepProjectPick(null, claude);
  keepProjectPick("org/first", codex);
  assert.deepEqual(lastProjectPick(null), claude);
  assert.deepEqual(lastProjectPick("org/first"), codex);
});

test("invalid stored preferences are ignored without losing valid projects", () => {
  localStorage.setItem("beep.projectModels.v1", JSON.stringify([
    ["org/first", codex], ["org/second", { agent: "claude", model: 42, effort: null, fast: null }],
    ["org/third", { agent: "claude", model: "model-b", effort: false }], null,
  ]));
  assert.deepEqual(lastProjectPick("org/first"), codex);
  assert.equal(lastProjectPick("org/second"), null);
  assert.equal(lastProjectPick("org/third"), null);
  localStorage.setItem("beep.projectModels.v1", "{");
  assert.equal(lastProjectPick("org/first"), null);
  keepProjectPick("org/second", claude);
  assert.deepEqual(lastProjectPick("org/second"), claude);
});

test("fast mode keeps explicit off and defaults older choices", () => {
  keepProjectPick("speed", { agent: "codex", model: "model-a", effort: "ultra", fast: false });
  assert.equal(lastProjectPick("speed")?.fast, false);
});
