import test from "node:test";
import assert from "node:assert/strict";
import {
  type CatalogModel, FEATURES, contextLabel, detailLine, filterModels, providerFor, stepSel, toggleFeature,
} from "../src/settings/modelsearch-state.ts";

const model = (over: Partial<CatalogModel> = {}): CatalogModel => ({
  id: "acme/alpha", name: "Alpha", created: 100, context_length: 200_000, features: ["chat", "tools"],
  tools: true, images: false, input_per_m: 3, output_per_m: 15, ...over,
});

const ids = (list: CatalogModel[]) => list.map(m => m.id);

test("every typed word must match the name or the id, case aside", () => {
  const list = [model(), model({ id: "other/beta-pro", name: "Beta Pro" })];
  assert.deepEqual(ids(filterModels(list, "ACME alp", ["chat"])), ["acme/alpha"]);
  assert.deepEqual(ids(filterModels(list, "beta other", ["chat"])), ["other/beta-pro"]);
  assert.deepEqual(ids(filterModels(list, "beta alpha", ["chat"])), []);
  assert.equal(filterModels(list, "  ", ["chat"]).length, 2);
});

test("results come newest first, then the better match, then by id", () => {
  const list = [
    model({ id: "a/old", name: "Old", created: 1 }),
    model({ id: "b/mid-x", name: "Mid X", created: 5 }),
    model({ id: "c/x-mid", name: "X Mid", created: 5 }),
    model({ id: "d/none", name: "None", created: null }),
  ];
  assert.deepEqual(ids(filterModels(list, "", ["chat"])), ["b/mid-x", "c/x-mid", "a/old", "d/none"]);
  assert.deepEqual(ids(filterModels(list, "x", ["chat"])), ["c/x-mid", "b/mid-x"]);
});

test("a feature toggle keeps only the models that have it, and chat stays on", () => {
  const list = [model(), model({ id: "b/pics", features: ["chat", "vision"], images: true })];
  assert.deepEqual(ids(filterModels(list, "", ["chat", "vision"])), ["b/pics"]);
  assert.deepEqual(toggleFeature(["chat"], "vision"), ["chat", "vision"]);
  assert.deepEqual(toggleFeature(["chat", "vision"], "vision"), ["chat"]);
  assert.deepEqual(toggleFeature(["chat"], "chat"), ["chat"]);
});

test("the toggles keep the order of the flashmind catalog", () => {
  assert.equal(FEATURES.length, 14);
  assert.deepEqual(FEATURES.slice(0, 3), ["chat", "tools", "reasoning"]);
  assert.equal(FEATURES[FEATURES.length - 1], "style-reference");
});

test("arrow keys wrap around the rows", () => {
  assert.equal(stepSel(0, 3, "ArrowUp"), 2);
  assert.equal(stepSel(2, 3, "ArrowDown"), 0);
  assert.equal(stepSel(1, 3, "ArrowDown"), 2);
  assert.equal(stepSel(5, 0, "ArrowDown"), 0);
});

test("a row says its context and price", () => {
  assert.equal(contextLabel(1_048_576), "1M");
  assert.equal(contextLabel(200_000), "200k");
  assert.equal(contextLabel(null), "");
  assert.equal(detailLine(model()), "200k context, $3 in, $15 out per M tokens");
  assert.equal(detailLine(model({ context_length: null, input_per_m: null })), "No price listed");
});

test("a provider counts only for the model it was picked for", () => {
  const pref = { model: "acme/alpha", tag: "deepinfra" };
  assert.equal(providerFor(pref, "acme/alpha"), "deepinfra");
  assert.equal(providerFor(pref, "other/beta"), null);
  assert.equal(providerFor(null, "acme/alpha"), null);
});
