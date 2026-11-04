import test from "node:test";
import assert from "node:assert/strict";
import { DEBOUNCE_MS, PaletteSearch } from "../src/center/palette-search.ts";

/// A find whose answers the test hands out by hand, in any order.
function finder() {
  const asks: { query: string; full: boolean; answer(hits: string[]): Promise<void> }[] = [];
  const find = (query: string, full: boolean) => new Promise<string[]>(resolve => {
    // Awaiting the answer lets the search's continuation run first.
    asks.push({ query, full, answer: async hits => { resolve(hits); await new Promise(r => setImmediate(r)); } });
  });
  return { asks, find };
}

function drawn() {
  const shown: string[] = [];
  return { shown, draw: (query: string, hits: string[]) => { shown.push(`${query}: ${hits.join(",")}`); } };
}

test("an answer to an earlier query never draws over a later one", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { asks, find } = finder();
  const { shown, draw } = drawn();
  const search = new PaletteSearch(find, draw);
  search.type("pr");
  t.mock.timers.tick(DEBOUNCE_MS);
  search.type("proj");
  const [prLocal, prFull, projLocal] = asks;
  await projLocal.answer(["projects"]);
  // The slow full search for "pr" comes back after "proj" drew.
  await prFull.answer(["pull request"]);
  await prLocal.answer(["pr"]);
  assert.deepEqual(shown, ["proj: projects"]);
  t.mock.timers.tick(DEBOUNCE_MS);
  await asks[3].answer(["projects", "chat about projects"]);
  assert.deepEqual(shown, ["proj: projects", "proj: projects,chat about projects"]);
  search.stop();
});

test("a late local answer does not replace the full one for the same query", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { asks, find } = finder();
  const { shown, draw } = drawn();
  const search = new PaletteSearch(find, draw);
  search.type("deploy");
  t.mock.timers.tick(DEBOUNCE_MS);
  const [local, full] = asks;
  await full.answer(["deploy", "chat text"]);
  await local.answer(["deploy"]);
  assert.deepEqual(shown, ["deploy: deploy,chat text"]);
  search.stop();
});

test("a refresh asks again for the same query, and nothing draws after closing", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { asks, find } = finder();
  const { shown, draw } = drawn();
  const search = new PaletteSearch(find, draw);
  search.type("");
  search.refresh();
  assert.deepEqual(asks.map(a => [a.query, a.full]), [["", false], ["", false]]);
  await asks[1].answer(["new lists"]);
  // The answer from before the lists came in is older, so it stays hidden.
  await asks[0].answer(["old lists"]);
  assert.deepEqual(shown, [": new lists"]);
  t.mock.timers.tick(DEBOUNCE_MS);
  assert.equal(asks.length, 3);
  search.stop();
  await asks[2].answer(["after close"]);
  t.mock.timers.tick(DEBOUNCE_MS);
  assert.deepEqual(shown, [": new lists"]);
  assert.equal(asks.length, 3);
});
