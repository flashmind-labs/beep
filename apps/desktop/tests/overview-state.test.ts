import test from "node:test";
import assert from "node:assert/strict";
import { overviewGroups, projectActivity, projectSortTime, parseProjectSort, searchOverview } from "../src/stores/project/overview-state.ts";

const row = (name: string, folder: string, url: string | null = null) => ({ name, url, machines: [{ id: "m", name: "build-box", folder }] });
const rows = [
  row("acme/alpha", "/srv/acme/alpha", "https://example.test/acme/alpha"),
  row("acme/beta", "/srv/acme/beta"),
  row("acme/gamma", "/srv/acme/gamma"),
  row("other/delta", "/srv/work/delta", "https://forge.test/team/delta-remote"),
];
const names = (groups: { cards: { name: string }[] }[]) => groups.map(g => g.cards.map(c => c.name));

test("cards are ordered pinned first, then by recent activity, then by name", () => {
  const activity = new Map([["acme/gamma", 300], ["acme/alpha", 100], ["other/delta", 50]]);
  assert.deepEqual(names(overviewGroups(rows, [], [], activity)), [["acme/gamma", "acme/alpha", "acme/beta"], ["other/delta"]]);
  assert.deepEqual(names(overviewGroups(rows, ["acme/beta"], [], activity)), [["acme/beta", "acme/gamma", "acme/alpha"], ["other/delta"]]);
});

test("equal activity keeps the name order and groups keep their place", () => {
  const groups = overviewGroups(rows, [], [{ id: "g", name: "Work", paths: ["/srv/work"] }], new Map([["acme/beta", 5], ["acme/alpha", 5]]));
  assert.deepEqual(groups.map(g => g.group?.name ?? g.org), ["Work", "acme"]);
  assert.deepEqual(names(groups), [["other/delta"], ["acme/alpha", "acme/beta", "acme/gamma"]]);
});

test("activity is the latest of agents, sessions and visits", () => {
  assert.equal(projectActivity({ last: null }, null, null), 0);
  assert.equal(projectActivity({ last: 10 }, [{ last_activity: 40 }, { last_activity: 20 }], 30), 40);
  assert.equal(projectActivity({ last: 10 }, [], 30), 30);
});

test("search matches name, folder and remote across groups, ignoring case", () => {
  const groups = overviewGroups(rows, [], []);
  assert.deepEqual(names(searchOverview(groups, "BETA")), [["acme/beta"]]);
  assert.deepEqual(names(searchOverview(groups, "/srv/work")), [["other/delta"]]);
  assert.deepEqual(names(searchOverview(groups, "delta-remote")), [["other/delta"]]);
  assert.deepEqual(names(searchOverview(groups, "a")).flat().length, 4);
  assert.deepEqual(searchOverview(groups, "nothing here"), []);
  assert.equal(searchOverview(groups, "  ").length, groups.length);
});

test("search hides a group with no match, including an empty one", () => {
  const groups = overviewGroups(rows, [], [{ id: "empty", name: "Empty", paths: ["/nowhere"] }]);
  assert.equal(groups[0].group?.name, "Empty");
  assert.deepEqual(searchOverview(groups, "alpha").map(g => g.org), ["acme"]);
});


test("sort choices separate visits from agent activity without changing pins or groups", () => {
  const data = [
    { ...row("acme/alpha", "/srv/a"), last: 300, visit: 0 },
    { ...row("acme/beta", "/srv/b"), last: 100, visit: 400 },
    { ...row("acme/gamma", "/srv/c"), last: null, visit: 0 },
  ];
  const ordered = (sort: "default" | "activity" | "opened", pins: string[] = []) => names(overviewGroups(data, pins, [], new Map(data.map(p => [p.name, projectSortTime(sort, p, null, p.visit)]))));
  assert.deepEqual(ordered("default"), [["acme/beta", "acme/alpha", "acme/gamma"]]);
  assert.deepEqual(ordered("activity"), [["acme/alpha", "acme/beta", "acme/gamma"]]);
  assert.deepEqual(ordered("opened"), [["acme/beta", "acme/alpha", "acme/gamma"]]);
  assert.deepEqual(ordered("opened", ["acme/gamma"]), [["acme/gamma", "acme/beta", "acme/alpha"]]);
  assert.equal(projectSortTime("activity", { last: 100 }, [{ last_activity: 500 }], 900), 500);
  assert.equal(projectSortTime("opened", { last: 100 }, [{ last_activity: 500 }], null), 0);
  assert.equal(parseProjectSort(null), "default");
  assert.equal(parseProjectSort("invalid"), "default");
  assert.equal(parseProjectSort("opened"), "opened");
});
