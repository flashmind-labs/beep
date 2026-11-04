import test from "node:test";
import assert from "node:assert/strict";
import { DEBOUNCE_MS, PaletteSearch } from "../src/center/palette-search.ts";

/// A finder whose answers the test hands out by hand, in any order.
function finder() {
  const asks: { query: string; answer(found: string[]): Promise<void> }[] = [];
  const find = (query: string) => new Promise<string[]>(resolve => {
    // Awaiting the answer lets the search's continuation run first.
    asks.push({ query, answer: async found => { resolve(found); await new Promise(r => setImmediate(r)); } });
  });
  return { asks, find };
}

function drawn() {
  const shown: string[] = [];
  return { shown, draw: (query: string, text: string[]) => { shown.push(`${query}: ${text.join(",")}`); } };
}

test("each keystroke draws at once, and chat text waits for typing to stop", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { asks, find } = finder();
  const { shown, draw } = drawn();
  const search = new PaletteSearch(find, draw);
  search.type("p");
  search.type("pr");
  search.type("pro");
  assert.deepEqual(shown, ["p: ", "pr: ", "pro: "]);
  t.mock.timers.tick(DEBOUNCE_MS - 1);
  assert.equal(asks.length, 0);
  t.mock.timers.tick(1);
  assert.deepEqual(asks.map(a => a.query), ["pro"]);
  search.stop();
});

test("one letter does not search chat text", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { asks, find } = finder();
  const { draw } = drawn();
  const search = new PaletteSearch(find, draw);
  search.type("p");
  t.mock.timers.tick(DEBOUNCE_MS);
  assert.equal(asks.length, 0);
  search.stop();
});

test("an answer draws, and a refresh keeps it", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { asks, find } = finder();
  const { shown, draw } = drawn();
  const search = new PaletteSearch(find, draw);
  search.type("deploy");
  t.mock.timers.tick(DEBOUNCE_MS);
  await asks[0].answer(["chat"]);
  search.refresh();
  assert.deepEqual(shown, ["deploy: ", "deploy: chat", "deploy: chat"]);
  search.stop();
});

test("one search at a time, and an answer to an earlier query never draws", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { asks, find } = finder();
  const { shown, draw } = drawn();
  const search = new PaletteSearch(find, draw);
  search.type("pr");
  t.mock.timers.tick(DEBOUNCE_MS);
  search.type("proj");
  t.mock.timers.tick(DEBOUNCE_MS);
  assert.equal(asks.length, 1, "the first still runs");
  await asks[0].answer(["about pr"]);
  assert.deepEqual(asks.map(a => a.query), ["pr", "proj"]);
  await asks[1].answer(["about proj"]);
  assert.deepEqual(shown, ["pr: ", "proj: ", "proj: about proj"]);
  search.stop();
});

test("a newer query still waiting keeps its own turn", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { asks, find } = finder();
  const { draw } = drawn();
  const search = new PaletteSearch(find, draw);
  search.type("pr");
  t.mock.timers.tick(DEBOUNCE_MS);
  search.type("proj");
  await asks[0].answer([]);
  assert.equal(asks.length, 1);
  t.mock.timers.tick(DEBOUNCE_MS);
  assert.deepEqual(asks.map(a => a.query), ["pr", "proj"]);
  search.stop();
});

test("nothing draws or asks after closing", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { asks, find } = finder();
  const { shown, draw } = drawn();
  const search = new PaletteSearch(find, draw);
  search.type("deploy");
  t.mock.timers.tick(DEBOUNCE_MS);
  search.type("deploys");
  search.stop();
  await asks[0].answer(["late"]);
  search.refresh();
  t.mock.timers.tick(DEBOUNCE_MS);
  assert.deepEqual(shown, ["deploy: ", "deploys: "]);
  assert.equal(asks.length, 1);
});
