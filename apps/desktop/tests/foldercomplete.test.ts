import test from "node:test";
import assert from "node:assert/strict";
import { MAX_SUGGESTIONS, NO_MENU, completed, isOpen, menuKey, menuOf, splitTyped, suggest } from "../src/foldercomplete.ts";

test("splitTyped lists the folder before the last slash", () => {
  assert.deepEqual(splitTyped("/srv/code/wid"), { dir: "/srv/code/", prefix: "wid" });
  assert.deepEqual(splitTyped("/srv/code/"), { dir: "/srv/code/", prefix: "" });
  assert.deepEqual(splitTyped("/"), { dir: "/", prefix: "" });
  assert.deepEqual(splitTyped("/sr"), { dir: "/", prefix: "sr" });
});

test("splitTyped takes ~/ as the home folder", () => {
  assert.deepEqual(splitTyped("~/"), { dir: "~/", prefix: "" });
  assert.deepEqual(splitTyped("~/Pro"), { dir: "~/", prefix: "Pro" });
  assert.deepEqual(splitTyped("~/Projects/acme/"), { dir: "~/Projects/acme/", prefix: "" });
});

test("splitTyped refuses what the machine cannot read", () => {
  assert.equal(splitTyped(""), null);
  assert.equal(splitTyped("~"), null);
  assert.equal(splitTyped("srv/code"), null);
  assert.equal(splitTyped(" /srv"), null);
  assert.equal(splitTyped("/srv/../etc/"), null);
  assert.equal(splitTyped("/srv/a\tb"), null);
});

test("suggest matches the start of a name, ignoring case, sorted by name", () => {
  const folders = ["projects", "Pictures", "Public", "Documents", "proto"];
  assert.deepEqual(suggest(folders, "p"), ["Pictures", "projects", "proto", "Public"]);
  assert.deepEqual(suggest(folders, "PRO"), ["projects", "proto"]);
  assert.deepEqual(suggest(folders, "x"), []);
  assert.deepEqual(suggest(folders, ""), ["Documents", "Pictures", "projects", "proto", "Public"]);
});

test("suggest hides dot folders unless the name typed starts with a dot", () => {
  const folders = [".config", ".cache", "code"];
  assert.deepEqual(suggest(folders, ""), ["code"]);
  assert.deepEqual(suggest(folders, "c"), ["code"]);
  assert.deepEqual(suggest(folders, "."), [".cache", ".config"]);
  assert.deepEqual(suggest(folders, ".co"), [".config"]);
});

test("suggest caps the list", () => {
  const folders = Array.from({ length: 30 }, (_, i) => `d${String(i).padStart(2, "0")}`);
  const got = suggest(folders, "d");
  assert.equal(got.length, MAX_SUGGESTIONS);
  assert.equal(got[0], "d00");
  assert.equal(got[MAX_SUGGESTIONS - 1], "d11");
  assert.equal(suggest(folders, "d", 3).length, 3);
});

test("suggest offers nothing when the only match is the name typed", () => {
  assert.deepEqual(suggest(["widget", "other"], "widget"), []);
  assert.deepEqual(suggest(["widget", "widgets"], "widget"), ["widget", "widgets"]);
  assert.deepEqual(suggest(["Widget"], "widget"), ["Widget"]);
});

test("completed adds the folder and a slash", () => {
  assert.equal(completed("/srv/", "code"), "/srv/code/");
  assert.equal(completed("~/", "Projects"), "~/Projects/");
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

test("menuKey leaves keys to the field with no menu or a modifier held", () => {
  assert.equal(isOpen(NO_MENU), false);
  for (const key of ["ArrowDown", "ArrowUp", "Tab", "Enter", "Escape"]) {
    assert.deepEqual(menuKey(NO_MENU, key), { type: "none" });
  }
  const menu = menuOf("/srv/", ["code"]);
  assert.deepEqual(menuKey(menu, "Tab", true), { type: "none" });
  assert.deepEqual(menuKey(menu, "Enter", true), { type: "none" });
});

test("menuKey keeps a stale highlight inside the list", () => {
  const menu = { dir: "/srv/", items: ["a", "b"], index: 5 };
  assert.deepEqual(menuKey(menu, "Enter"), { type: "accept", text: "/srv/b/" });
});
