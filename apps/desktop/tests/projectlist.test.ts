import test from "node:test";
import assert from "node:assert/strict";
import { chipState, ciState, filterByName, listGroups, machineChips, projectBranch, splitName } from "../src/project/projectlist-state.ts";
import type { SyncRow } from "../src/project/filesync.ts";

const sync = (machine_id: string, status: string, error: string | null = null): SyncRow => ({ project: "acme/widget", machine_id, status, files: 0, pending: 0, error });

test("the name splits into a muted owner and the repo", () => {
  assert.deepEqual(splitName("acme/widget"), { owner: "acme/", repo: "widget" });
  assert.deepEqual(splitName("group/sub/app"), { owner: "group/sub/", repo: "app" });
  assert.deepEqual(splitName("notes"), { owner: "", repo: "notes" });
});

test("a chip is offline, in conflict, syncing or in step", () => {
  assert.equal(chipState(false, sync("m1", "in_sync"), 0).state, "off");
  assert.equal(chipState(true, sync("m1", "offline"), 0).state, "off");
  assert.deepEqual(chipState(true, sync("m1", "in_sync"), 2), { state: "conflict", label: "Conflict" });
  assert.equal(chipState(true, sync("m1", "syncing"), 0).state, "syncing");
  assert.equal(chipState(true, sync("m1", "scanning"), 0).state, "syncing");
  assert.equal(chipState(true, sync("m1", "in_sync"), 0).state, "ok");
  assert.deepEqual(chipState(true, sync("m1", "failed"), 0), { state: "conflict", label: "Not syncing" });
  // One machine, or Syncthing keeps it: no Beep sync to wait on.
  assert.equal(chipState(true, undefined, 0).state, "ok");
  assert.equal(chipState(undefined, sync("m1", "failed", "Already synced by Syncthing"), 0).state, "ok");
});

test("each machine's chip says its folder and state in the tooltip", () => {
  const chips = machineChips(
    { name: "acme/widget", machines: [{ id: "m1", name: "build-box", folder: "/srv/widget" }, { id: "m2", name: "mac", folder: "/path/to/widget" }, { id: "m3", name: "gpu-box", folder: "/srv/widget" }] },
    {
      online: new Map([["m1", true], ["m2", true], ["m3", false]]),
      sync: [sync("m1", "in_sync"), sync("m2", "syncing"), { ...sync("m1", "failed"), project: "other/repo" }],
      detail: { project: "acme/widget", pinned: false, paused: false, machines: [] },
    });
  assert.deepEqual(chips.map(c => c.state), ["ok", "syncing", "off"]);
  assert.equal(chips[0].title, "build-box: /srv/widget · In step");
  assert.equal(chips[2].title, "gpu-box: /srv/widget · Offline");
});

test("the branch is this Mac's, else the first one checked out", () => {
  const rows = [{ machine_id: "m1", branch: null }, { machine_id: "m2", branch: "main" }, { machine_id: "mac", branch: "signup" }];
  assert.equal(projectBranch(rows, "mac"), "signup");
  assert.equal(projectBranch(rows, "elsewhere"), "main");
  assert.equal(projectBranch(undefined, "mac"), null);
  assert.equal(projectBranch([{ machine_id: "m1", branch: null }], null), null);
});

test("the CI dot follows the newest run on the branch", () => {
  const runs = [{ branch: "fix", state: "running" }, { branch: "main", state: "failed" }, { branch: "main", state: "passed" }];
  assert.equal(ciState(runs, "main"), "bad");
  assert.equal(ciState(runs, "fix"), "busy");
  assert.equal(ciState(runs, "other"), "busy");
  assert.equal(ciState(runs, null), "busy");
  assert.equal(ciState([{ branch: "main", state: "passed" }], "main"), "ok");
  assert.equal(ciState([{ branch: "main", state: "cancelled" }], "main"), "none");
  assert.equal(ciState([], "main"), "none");
  assert.equal(ciState(undefined, "main"), "none");
});

test("the filter matches the name in any case", () => {
  const rows = [{ name: "acme/Widget" }, { name: "acme/relay" }, { name: "northwind/pines" }];
  assert.deepEqual(filterByName(rows, " WID ").map(r => r.name), ["acme/Widget"]);
  assert.deepEqual(filterByName(rows, "acme").map(r => r.name), ["acme/Widget", "acme/relay"]);
  assert.equal(filterByName(rows, "").length, 3);
  assert.equal(filterByName(rows, "zzz").length, 0);
});

test("projects with agents go under Working now, the rest under Others", () => {
  const rows = [{ name: "a", agents: 0 }, { name: "b", agents: 2 }, { name: "c", agents: 0 }, { name: "d", agents: 1 }];
  assert.deepEqual(listGroups(rows).map(g => [g.label, g.rows.map(r => r.name)]), [["Working now", ["b", "d"]], ["Others", ["a", "c"]]]);
  assert.deepEqual(listGroups(rows.filter(r => !r.agents)).map(g => g.label), ["Others"]);
  assert.deepEqual(listGroups([]), []);
});
