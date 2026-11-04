import test from "node:test";
import assert from "node:assert/strict";
import { beepViews, groupShown, signInWay } from "../src/settings/subscription.ts";

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
