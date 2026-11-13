import test from "node:test";
import assert from "node:assert/strict";
import { FIRST_PAGE, fitPath, openSub, parsePath, parseSub, pathKey, recallPath, rememberPath, selectPage, type SettingsPage } from "../src/settings/settings-nav.ts";

const ALL: SettingsPage[] = ["general", "appearance", "mac", "machines", "agents", "creds", "instr"];

test("a kept page that makes no sense opens General, or its page", () => {
  assert.equal(parsePath("nonsense"), null);
  assert.equal(parsePath(""), null);
  assert.deepEqual(parsePath("machines/weird"), { page: "machines", sub: null });
  // A subpage kept under the wrong page is dropped.
  assert.deepEqual(parsePath("creds/machine:abc"), { page: "creds", sub: null });
  assert.equal(parseSub("constructor:x"), null);
  // An id may hold colons of its own.
  assert.deepEqual(parsePath(pathKey(openSub({ kind: "machine", id: "a:b" }))), openSub({ kind: "machine", id: "a:b" }));
  const broken = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
  assert.deepEqual(recallPath(broken), FIRST_PAGE);
  assert.doesNotThrow(() => rememberPath(broken, selectPage("machines")));
  assert.deepEqual(recallPath(null), FIRST_PAGE);
});

test("a restored page falls back when it is not shown or its subpage is gone", () => {
  const shown = ALL.filter(p => p !== "mac");
  assert.deepEqual(fitPath(selectPage("mac"), shown), { page: "general", sub: null });
  const machine = openSub({ kind: "machine", id: "box" });
  assert.deepEqual(fitPath(machine, shown, () => false), { page: "machines", sub: null });
  assert.deepEqual(fitPath(machine, shown, () => true), machine);
});
