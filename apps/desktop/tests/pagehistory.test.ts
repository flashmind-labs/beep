import test from "node:test";
import assert from "node:assert/strict";
import { PageHistory } from "../src/center/pagehistory.ts";

test("back and forward walk the pages, and a new page ends what was ahead", () => {
  const h = new PageHistory<string>();
  h.visit("home", "chat");
  h.visit("chat", "project");
  assert.equal(h.back("project"), "chat");
  assert.equal(h.back("chat"), "home");
  assert.equal(h.back("home"), undefined);
  assert.equal(h.forward("home"), "chat");
  h.visit("chat", "settings");
  assert.equal(h.forward("settings"), undefined);
  assert.equal(h.back("settings"), "chat");
});

test("opening the page already showing is not a step", () => {
  const h = new PageHistory<string>();
  h.visit("home", "home");
  assert.equal(h.back("home"), undefined);
});
