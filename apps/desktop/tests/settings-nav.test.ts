import test from "node:test";
import assert from "node:assert/strict";
import { agentSubs, back, CODING_AGENTS, crumbs, FIRST_PAGE, fitPath, navEntries, openSub, parsePath, parseSub, pathKey, recallPath, REMEMBER_KEY, rememberPath, samePath, selectPage, type SettingsPage } from "../src/settings-nav.ts";

/// A Storage that keeps what it is given.
function memory(): Pick<Storage, "getItem" | "setItem"> & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return { data, getItem: k => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
}

const ALL: SettingsPage[] = ["general", "models", "apps", "mac", "machines", "agents", "creds", "instr", "devices"];

test("picking a page shows it with no subpage", () => {
  assert.deepEqual(selectPage("machines"), { page: "machines", sub: null });
  // Picking a page from a subpage closes the subpage.
  const onApp = openSub({ kind: "app", id: "gmail" });
  assert.deepEqual(selectPage(onApp.page), { page: "apps", sub: null });
});

test("a subpage opens under the page it belongs to", () => {
  assert.deepEqual(openSub({ kind: "app", id: "slack" }), { page: "apps", sub: { kind: "app", id: "slack" } });
  assert.equal(openSub({ kind: "mcp", id: "m1" }).page, "apps");
  assert.equal(openSub({ kind: "machine", id: "build" }).page, "machines");
  assert.equal(openSub({ kind: "agent", id: "claude" }).page, "agents");
});

test("back goes from a subpage to its page, and nowhere from a page", () => {
  assert.deepEqual(back(openSub({ kind: "agent", id: "claude" })), { page: "agents", sub: null });
  assert.equal(back(selectPage("agents")), null);
  assert.ok(samePath(back(openSub({ kind: "machine", id: "x" }))!, selectPage("machines")));
  assert.ok(!samePath(openSub({ kind: "app", id: "a" }), openSub({ kind: "app", id: "b" })));
});

test("the breadcrumb names the page, then the subpage", () => {
  const name = () => "Slack";
  assert.deepEqual(crumbs(openSub({ kind: "app", id: "slack" }), name), ["Apps", "Slack"]);
  assert.deepEqual(crumbs(selectPage("agents"), name), ["Agent accounts"]);
});

test("the last picked page and subpage come back", () => {
  const store = memory();
  assert.deepEqual(recallPath(store), FIRST_PAGE);
  rememberPath(store, openSub({ kind: "app", id: "google-calendar" }));
  assert.equal(store.data.get(REMEMBER_KEY), "apps/app:google-calendar");
  assert.deepEqual(recallPath(store), { page: "apps", sub: { kind: "app", id: "google-calendar" } });
  rememberPath(store, selectPage("creds"));
  assert.deepEqual(recallPath(store), { page: "creds", sub: null });
});

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

test("the coding agents nest under Agent accounts once it is picked", () => {
  const subs = (page: SettingsPage) => (page === "agents" ? agentSubs() : []);
  const ids = (path: Parameters<typeof navEntries>[1]) => navEntries(ALL, path, subs).map(e => (e.sub ? `  ${e.sub.id}` : e.page));
  // Picked elsewhere, Agent accounts shows no harness under it.
  assert.ok(!ids(selectPage("general")).some(id => id.startsWith("  ")));
  // Picked, each harness follows it in order, before Credentials.
  const open = ids(selectPage("agents"));
  assert.deepEqual(open.slice(open.indexOf("agents"), open.indexOf("creds") + 1), ["agents", ...CODING_AGENTS.map(a => `  ${a}`), "creds"]);
  // On Agent accounts itself the page is current; on a harness, only that harness.
  assert.deepEqual(navEntries(ALL, selectPage("agents"), subs).filter(e => e.current), [{ page: "agents", sub: null, current: true }]);
  assert.deepEqual(navEntries(ALL, openSub({ kind: "agent", id: "flsh" }), subs).filter(e => e.current),
    [{ page: "agents", sub: { kind: "agent", id: "flsh" }, current: true }]);
  // Each one opens under Agent accounts and survives being kept.
  for (const sub of agentSubs()) assert.deepEqual(parsePath(pathKey(openSub(sub))), { page: "agents", sub });
});
