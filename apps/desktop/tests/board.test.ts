import test from "node:test";
import assert from "node:assert/strict";
import { taskTile, prTile, machineTile, agentTile } from "../src/board.ts";
import { importedWhen, projectLine } from "../src/projects-state.ts";
import { machineSync } from "../src/home.ts";

test("the tasks tile lists open tasks, running first, and chips what waits on you", () => {
  const tile = taskTile([
    { title: "later", state: "planned" },
    { title: "check", state: "review" },
    { title: "build", state: "running" },
    { title: "shipped", state: "done" },
  ]);
  assert.equal(tile.count, "1 running");
  assert.deepEqual(tile.rows.map(r => r.text), ["build", "check", "later"]);
  assert.equal(tile.rows[1].chip.text, "review");
  assert.equal(tile.rows[0].chip, null);
  assert.equal(taskTile([{ title: "x", state: "planned" }]).count, "1 open");
  assert.equal(taskTile([{ title: "x", state: "done" }]).count, "all done");
  assert.equal(taskTile([]).empty, "No tasks yet");
});

test("the pull requests tile shows live PRs first and skips dropped tasks", () => {
  const tile = prTile([
    { title: "old", state: "done", pr_url: "https://example.com/o/r/pull/3" },
    { title: "copy", state: "review", pr_url: "https://example.com/o/r/pull/41" },
    { title: "gone", state: "dropped", pr_url: "https://example.com/o/r/pull/9" },
    { title: "no pr", state: "running" },
  ]);
  assert.equal(tile.count, "1 open");
  assert.deepEqual(tile.rows.map(r => r.code), ["#41", "#3"]);
  assert.equal(tile.rows[0].chip.text, "review");
  assert.equal(tile.rows[1].note, "done");
});

test("the machines tile counts where the project is checked out", () => {
  const tile = machineTile([
    { kind: "here", name: "build-box", folder: "/srv/project/widget" },
    { kind: "offline", name: "gpu-box" },
    { kind: "none", name: "mac-box" },
  ]);
  assert.equal(tile.count, "1/3");
  assert.deepEqual(tile.rows.map(r => r.note), ["widget", "offline", "not here"]);
  assert.deepEqual(tile.rows.map(r => r.dot), ["", "off", "off"]);
  assert.equal(machineTile([]).count, "");
});

test("a machine still fetching the project says how far it is", () => {
  const where = [{ kind: "here", id: "m1", name: "build-box", folder: "/srv/project/widget" }, { kind: "here", id: "m2", name: "mac-box", folder: "/srv/w" }];
  const sync = [
    { project: "acme/widget", machine_id: "m1", status: "syncing", files: 142, pending: 468 },
    { project: "acme/widget", machine_id: "m2", status: "in_sync", files: 610, pending: 0 },
  ];
  assert.deepEqual(machineTile(where, sync).rows.map(r => r.note), ["syncing 23%", "w"]);
  assert.deepEqual(machineSync("m1", sync), { project: "acme/widget", pct: 23, text: "Syncing, 23%", files: "142 of 610 files" });
  assert.equal(machineSync("m2", sync), null);
  assert.equal(machineSync("m1", [{ project: "p", machine_id: "m1", status: "scanning", files: 0, pending: 0 }]).text, "Scanning");
  assert.equal(machineSync("m1", null), null);
});

test("the agents tile lists running agents, then past sessions", () => {
  const tile = agentTile([{ title: "fix", machine_name: "build-box", status: "asking" }], [{ title: "", status: "ended" }]);
  assert.equal(tile.count, "1 running");
  assert.deepEqual(tile.rows.map(r => r.text), ["fix", "Untitled session"]);
  assert.equal(tile.rows[0].chip.text, "asks");
  assert.equal(agentTile([], [{ title: "a" }]).count, "1 past");
});

test("import times read as days or weeks", () => {
  const now = new Date(2026, 5, 20, 12).getTime();
  assert.equal(importedWhen(new Date(2026, 5, 20, 1).getTime(), now), "today");
  assert.equal(importedWhen(new Date(2026, 5, 19, 23).getTime(), now), "yesterday");
  assert.equal(importedWhen(new Date(2026, 5, 18).getTime(), now), "2 days ago");
  assert.equal(importedWhen(new Date(2026, 4, 30).getTime(), now), "3 weeks ago");
});

test("a project's line names its host and its first import", () => {
  const now = new Date(2026, 5, 20, 12).getTime();
  const p = { name: "acme/widget", imported: true, url: "https://github.com/acme/widget" };
  const rows = [
    { name: "acme/widget", imported_at: new Date(2026, 5, 19).getTime() },
    { name: "acme/widget", imported_at: new Date(2026, 5, 18).getTime() },
    { name: "acme/other", imported_at: new Date(2026, 0, 1).getTime() },
  ];
  assert.equal(projectLine(p, rows, now), "github.com · imported 2 days ago");
  assert.equal(projectLine({ ...p, url: "" }, [], now), "");
  assert.equal(projectLine({ name: "x", imported: false }, rows, now), "From running agents");
  assert.equal(projectLine(null, rows, now), "");
});
