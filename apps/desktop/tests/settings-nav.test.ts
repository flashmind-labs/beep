import test from "node:test";
import assert from "node:assert/strict";
import { FIRST_PAGE, fitPath, openSub, parsePath, parseSub, pathKey, recallPath, rememberPath, selectPage, type SettingsPage } from "../src/settings-nav.ts";

const ALL: SettingsPage[] = ["general", "models", "apps", "mac", "machines", "agents", "creds", "instr", "devices"];

test("a kept page that makes no sense opens General, or its page", () => {
  assert.equal(parsePath("nonsense"), null);
  assert.equal(parsePath(""), null);
  assert.deepEqual(parsePath("apps/weird"), { page: "apps", sub: null });
  // A subpage kept under the wrong page is dropped.
  assert.deepEqual(parsePath("creds/app:gmail"), { page: "creds", sub: null });
  assert.equal(parseSub("constructor:x"), null);
  // An id may hold colons of its own.
  assert.deepEqual(parsePath(pathKey(openSub({ kind: "machine", id: "a:b" }))), openSub({ kind: "machine", id: "a:b" }));
  const broken = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
  assert.deepEqual(recallPath(broken), FIRST_PAGE);
  assert.doesNotThrow(() => rememberPath(broken, selectPage("apps")));
  assert.deepEqual(recallPath(null), FIRST_PAGE);
});

test("a restored page falls back when it is not shown or its subpage is gone", () => {
  const shown = ALL.filter(p => p !== "models");
  assert.deepEqual(fitPath(selectPage("models"), shown), { page: "general", sub: null });
  const app = openSub({ kind: "app", id: "notion" });
  assert.deepEqual(fitPath(app, shown, () => false), { page: "apps", sub: null });
  assert.deepEqual(fitPath(app, shown, () => true), app);
  // Unlinked, Settings shows General and Account only.
  assert.deepEqual(fitPath(openSub({ kind: "agent", id: "codex" }), ["general", "devices"]), { page: "general", sub: null });
});
