import test from "node:test";
import assert from "node:assert/strict";
import { FIRST_PAGE, fitPath, navEntries, openSub, pageGroup, parsePath, parseSub, pathKey, recallPath, rememberPath, searchEntries, selectPage, type SettingsPage, type SettingsSub } from "../src/settings/settings-nav.ts";

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

test("search finds pages by name or search word, and subpages by name", () => {
  const subs = (page: SettingsPage): SettingsSub[] => page === "agents" ? [{ kind: "agent", id: "codex" }] : [];
  const find = (query: string) => searchEntries(ALL, FIRST_PAGE, query, subs, () => "Codex").map(e => e.setting ?? e.sub?.id ?? e.page);
  assert.deepEqual(find("cred"), ["creds"]);
  assert.deepEqual(find("TOKENS"), ["creds"]);
  assert.deepEqual(find("this comp"), ["mac", "Let Beep use this computer"]);
  assert.deepEqual(find("dev mode"), ["Developer mode"]);
  // A row's title comes before a page found only by its search words.
  assert.deepEqual(find("prompt"), ["Prompt marks", "instr"]);
  assert.deepEqual(find("system theme"), ["System theme", "appearance"]);
  assert.deepEqual(find("codex"), ["codex"]);
  assert.deepEqual(find("accounts"), ["agents"]);
  assert.deepEqual(find("xyz"), []);
});

test("navigation groups pages into four sections and keeps the parent active when subpages stay out of the rail", () => {
  assert.equal(pageGroup("general"), "App");
  assert.equal(pageGroup("machines"), "Fleet");
  assert.equal(pageGroup("agents"), "Agents");
  assert.equal(pageGroup("plugins"), "Extensions");
  const entries = navEntries(["general", "agents"], openSub({ kind: "agent", id: "codex" }), () => []);
  assert.deepEqual(entries.map(e => [e.page, e.current]), [["general", false], ["agents", true]]);
});
