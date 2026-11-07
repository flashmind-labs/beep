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

test("starting a session replaces its draft and preserves back and forward", () => {
  for (const previous of ["projects", "previous-session"]) {
    const h = new PageHistory<string>();
    h.visit(previous, "draft");
    h.visit("draft", "new-session", true);
    assert.equal(h.back("new-session"), previous);
    assert.equal(h.back(previous), undefined);
    assert.equal(h.forward(previous), "new-session");
    assert.equal(h.forward("new-session"), undefined);
  }
});

test("replacing a page clears forward history", () => {
  const h = new PageHistory<string>();
  h.visit("projects", "draft");
  h.visit("draft", "other-session");
  assert.equal(h.back("other-session"), "draft");
  h.visit("draft", "new-session", true);
  assert.equal(h.forward("new-session"), undefined);
  assert.equal(h.back("new-session"), "projects");
});
