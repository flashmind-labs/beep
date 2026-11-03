import test from "node:test";
import assert from "node:assert/strict";
import { isOpen, menuKey, menuOf, splitTyped, suggest } from "../src/foldercomplete.ts";

test("splitTyped refuses what the machine cannot read", () => {
  assert.equal(splitTyped(""), null);
  assert.equal(splitTyped("~"), null);
  assert.equal(splitTyped("srv/code"), null);
  assert.equal(splitTyped(" /srv"), null);
  assert.equal(splitTyped("/srv/../etc/"), null);
  assert.equal(splitTyped("/srv/a\tb"), null);
});

test("suggest hides dot folders unless the name typed starts with a dot", () => {
  const folders = [".config", ".cache", "code"];
  assert.deepEqual(suggest(folders, ""), ["code"]);
  assert.deepEqual(suggest(folders, "c"), ["code"]);
  assert.deepEqual(suggest(folders, "."), [".cache", ".config"]);
  assert.deepEqual(suggest(folders, ".co"), [".config"]);
});

test("suggest offers nothing when the only match is the name typed", () => {
  assert.deepEqual(suggest(["widget", "other"], "widget"), []);
  assert.deepEqual(suggest(["widget", "widgets"], "widget"), ["widget", "widgets"]);
  assert.deepEqual(suggest(["Widget"], "widget"), ["Widget"]);
});

test("menuKey moves, accepts and closes with the menu open", () => {
  const menu = menuOf("~/Projects/", ["acme", "beta", "gamma"]);
  assert.equal(isOpen(menu), true);
  assert.deepEqual(menuKey(menu, "ArrowDown"), { type: "move", index: 1 });
  assert.deepEqual(menuKey({ ...menu, index: 2 }, "ArrowDown"), { type: "move", index: 0 });
  assert.deepEqual(menuKey(menu, "ArrowUp"), { type: "move", index: 2 });
  assert.deepEqual(menuKey(menu, "Tab"), { type: "accept", text: "~/Projects/acme/" });
  assert.deepEqual(menuKey({ ...menu, index: 1 }, "Enter"), { type: "accept", text: "~/Projects/beta/" });
  assert.deepEqual(menuKey(menu, "Escape"), { type: "close" });
  assert.deepEqual(menuKey(menu, "a"), { type: "none" });
});

test("menuKey keeps a stale highlight inside the list", () => {
  const menu = { dir: "/srv/", items: ["a", "b"], index: 5 };
  assert.deepEqual(menuKey(menu, "Enter"), { type: "accept", text: "/srv/b/" });
});
