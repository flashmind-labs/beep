import test from "node:test";
import assert from "node:assert/strict";
import { beepViews, firstRunDue, groupShown, keyHidden, signInWay, viewHidden } from "../src/settings/subscription.ts";

test("no_server_hides_assistant_pages", () => {
  for (const name of ["chats", "research"]) {
    assert.equal(groupShown(name, false), false, name);
    assert.equal(groupShown(name, true), true, name);
  }
  // What works over the mesh stays.
  for (const name of ["pinned", "projects", "machines", "views"]) assert.equal(groupShown(name, false), true, name);
  const views = [{ view: "memories" }, { view: "routines" }, { view: "settings" }];
  assert.deepEqual(beepViews(views, false), [{ view: "settings" }]);
  assert.equal(beepViews(views, true).length, 3);
});

test("a hosted server signs in through the browser, any other starts the account here", () => {
  assert.equal(signInWay(true), "browser");
  assert.equal(signInWay(false), "here");
});

test("with no subscription the assistant's keys, pages and links do nothing and land on Home", () => {
  for (const key of ["m", "j", "l", "r", "g", "k"]) {
    assert.equal(keyHidden(key, false), true, key);
    assert.equal(keyHidden(key, true), false, key);
  }
  for (const key of ["n", ",", "q"]) assert.equal(keyHidden(key, false), false, key);
  for (const page of ["chats", "memories", "routines", "skills", "apps", "research", "model", "ginfo"]) {
    assert.equal(viewHidden("pane", page, false, true), true, page);
    assert.equal(viewHidden("pane", page, true, true), false, page);
  }
  assert.equal(viewHidden("research", undefined, false, true), true);
  assert.equal(viewHidden("researchopen", { session: "s" }, false, true), true);
  assert.equal(viewHidden("pane", { chat: { session: null } }, false, true), true);
  assert.equal(viewHidden("pane", { group: { id: "g" } }, false, true), true);
  // What works over the mesh opens.
  assert.equal(viewHidden("home", undefined, false, true), false);
  assert.equal(viewHidden("pane", "agents", false, true), false);
  assert.equal(viewHidden("pane", { agentnew: { prompt: "" } }, false, true), false);
  assert.equal(viewHidden("pane", "settings", false, true), false);
});

test("with no server the link, invite and removed pages do nothing, whatever the subscription", () => {
  for (const page of ["link", "invite", "revoked"]) {
    assert.equal(viewHidden("pane", page, false, false), true, page);
    assert.equal(viewHidden("pane", page, true, false), true, page);
  }
  assert.equal(viewHidden("pane", "link", false, true), false);
});

test("the first-run window shows once, with no subscription", () => {
  assert.equal(firstRunDue(false, null), true);
  assert.equal(firstRunDue(false, "1"), false);
  assert.equal(firstRunDue(true, null), false);
});
