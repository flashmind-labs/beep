import test from "node:test";
import assert from "node:assert/strict";
import { nextSessionIntro } from "../src/stores/agents/session-intro.ts";

test("welcome sentences exhaust the deck before repeating and differ across cycles", () => {
  let previous = "";

  for (let cycle = 0; cycle < 3; cycle++) {
    const seen = new Set<string>();

    for (let i = 0; i < 60; i++) {
      const sentence = nextSessionIntro();
      assert.notEqual(sentence, previous);
      assert.ok(!seen.has(sentence));
      seen.add(sentence);
      previous = sentence;
    }

    assert.equal(seen.size, 60);
  }
});
