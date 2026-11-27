import { test } from "node:test";
import assert from "node:assert/strict";
import { mobileLayout } from "../src/ui/platform.ts";
import { activeTab, backTo, fullScreen, type MobileAt } from "../src/shell/center/mobile-state.ts";

const at = (over: Partial<MobileAt>): MobileAt => ({ page: "home", paneView: null, homeTab: "overview", picked: null, tile: false, terminal: false, ...over });

test("a phone, a narrow window and a small touch screen get the phone layout", () => {
  assert.equal(mobileLayout({ width: 1280, height: 800, coarse: false, phone: false }), false);
  assert.equal(mobileLayout({ width: 699, height: 800, coarse: false, phone: false }), true);
  assert.equal(mobileLayout({ width: 700, height: 800, coarse: false, phone: false }), false);
  assert.equal(mobileLayout({ width: 820, height: 800, coarse: true, phone: false }), true);
  assert.equal(mobileLayout({ width: 1100, height: 800, coarse: true, phone: false }), false);
  assert.equal(mobileLayout({ width: 390, height: 844, coarse: true, phone: true }), true);
  assert.equal(mobileLayout({ width: 915, height: 412, coarse: true, phone: true }), true);
});

test("an unfolded foldable gets the desktop layout", () => {
  assert.equal(mobileLayout({ width: 690, height: 830, coarse: true, phone: true }), false);
  assert.equal(mobileLayout({ width: 830, height: 690, coarse: true, phone: true }), false);
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
  assert.equal(backTo(at({ page: "providers" })), "history");
});

test("a session and an open terminal hide the tab bar", () => {
  assert.equal(fullScreen(at({ page: "pane", paneView: "agent" })), true);
  assert.equal(fullScreen(at({ page: "pane", paneView: "agents" })), false);
  assert.equal(fullScreen(at({ page: "terminals", terminal: true })), true);
  assert.equal(fullScreen(at({})), false);
});
