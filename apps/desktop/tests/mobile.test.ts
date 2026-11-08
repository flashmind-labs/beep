import { test } from "node:test";
import assert from "node:assert/strict";
import { mobileLayout } from "../src/ui/platform.ts";
import { activeTab, backTo, type MobileAt } from "../src/center/mobile-state.ts";

const at = (over: Partial<MobileAt>): MobileAt => ({ page: "home", paneView: null, homeTab: "overview", picked: null, tile: false, terminal: false, ...over });

test("a phone, a narrow window and a small touch screen get the phone layout", () => {
  assert.equal(mobileLayout({ width: 1280, coarse: false, phone: false }), false);
  assert.equal(mobileLayout({ width: 699, coarse: false, phone: false }), true);
  assert.equal(mobileLayout({ width: 700, coarse: false, phone: false }), false);
  assert.equal(mobileLayout({ width: 820, coarse: true, phone: false }), true);
  assert.equal(mobileLayout({ width: 1100, coarse: true, phone: false }), false);
  assert.equal(mobileLayout({ width: 1100, coarse: false, phone: true }), true);
});

test("the tab bar lights the tab that owns the page", () => {
  assert.equal(activeTab(at({})), "home");
  assert.equal(activeTab(at({ homeTab: "machines" })), "machines");
  assert.equal(activeTab(at({ page: "pane", paneView: "agent" })), "agents");
  assert.equal(activeTab(at({ page: "pane", paneView: "chats" })), null);
  assert.equal(activeTab(at({ page: "projects", picked: "x" })), "projects");
});

test("Back goes up one step and is absent on a tab's first page", () => {
  assert.equal(backTo(at({})), null);
  assert.equal(backTo(at({ homeTab: "git" })), "overview");
  assert.equal(backTo(at({ page: "projects" })), null);
  assert.equal(backTo(at({ page: "projects", picked: "x" })), "projects");
  assert.equal(backTo(at({ page: "projects", picked: "x", tile: true })), "tile");
  assert.equal(backTo(at({ page: "terminals", terminal: true })), "terminals");
  assert.equal(backTo(at({ page: "pane", paneView: "agents" })), null);
  assert.equal(backTo(at({ page: "pane", paneView: "agent" })), "history");
  assert.equal(backTo(at({ page: "sync" })), "history");
});
