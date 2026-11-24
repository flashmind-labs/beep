import test from "node:test";
import assert from "node:assert/strict";
import { clearViews, installViews, snapshotView, takeSnapshotView, viewWrite, writeScope } from "../src/ui/snapshot-state.ts";

test("snapshots match command arguments, preserve null and keep caller changes private", t => {
  t.after(clearViews);
  installViews({ views: [
    { command: "center_project_runs", args: { projectId: "example" }, value: { runs: [1] } },
    { command: "agent_accounts", args: {}, value: null },
  ] });
  const read = snapshotView("center_project_runs", { refresh: false, projectId: "example" });
  assert.ok(read && read.kind === "value");
  assert.ok(read.value && typeof read.value === "object" && "runs" in read.value && Array.isArray(read.value.runs));
  read.value.runs.push(2);
  assert.deepEqual(snapshotView("center_project_runs", { projectId: "example" }), { kind: "value", value: { runs: [1] } });
  assert.equal(snapshotView("center_project_runs", { projectId: "another" }), null);
  assert.equal(snapshotView("center_project_runs", { projectId: "example", refresh: true }), null);
  assert.deepEqual(snapshotView("agent_accounts"), { kind: "value", value: null });
});

test("only read views are accepted and cache size stays bounded", t => {
  t.after(clearViews);
  installViews({ views: [{ command: "agent_start", args: {}, value: "unsafe" }] });
  assert.equal(snapshotView("agent_start"), null);
  installViews({ views: Array.from({ length: 129 }, (_, index) => ({
    command: "center_project_run", args: { projectId: "example", runId: index }, value: index,
  })) });
  assert.equal(snapshotView("center_project_run", { projectId: "example", runId: 0 }), null);
  assert.deepEqual(snapshotView("center_project_run", { projectId: "example", runId: 128 }), { kind: "value", value: 128 });
  assert.equal(viewWrite("save_agent_token"), true);
  assert.equal(viewWrite("agent_tree_edit"), true);
  assert.equal(viewWrite("center_project_runs"), false);
  assert.equal(viewWrite("session_move_progress"), false, "move progress is a read");
  assert.deepEqual(writeScope("agent_steer"), { kind: "views", commands: ["center_home", "agent_sessions"] });
  assert.deepEqual(writeScope("chat_move"), { kind: "views", commands: ["center_home", "agent_sessions"] }, "moving a chat changes the session lists");
  installViews({ views: [{ command: "agent_sessions", value: [] }] });
  assert.deepEqual(snapshotView("agent_sessions"), { kind: "value", value: [] }, "the all-sessions list is served from the push");
  assert.deepEqual(writeScope("set_zoom_level"), { kind: "none" });
  assert.deepEqual(writeScope("save_credential"), { kind: "all" });
});

test("source errors replace successful projections, then a retry can read fresh state", t => {
  t.after(clearViews);
  const args = { projectId: "example" };
  installViews({ views: [{ command: "center_project_runs", args, value: { runs: [1] } }] });
  installViews({ views: [{ command: "center_project_runs", args, error: "temporary source failure" }] });
  assert.deepEqual(snapshotView("center_project_runs", args), { kind: "error", error: "temporary source failure" });
  assert.deepEqual(takeSnapshotView("center_project_runs", args), { kind: "error", error: "temporary source failure" });
  assert.equal(snapshotView("center_project_runs", args), null);
  installViews({ views: [{ command: "center_project_runs", args, value: { runs: [1] } }] });
  assert.deepEqual(snapshotView("center_project_runs", args), { kind: "value", value: { runs: [1] } });
});

test("wire snapshots are validated before entering the cache", t => {
  t.after(clearViews);
  for (const malformed of [null, [], { views: null }, { views: [null, false, 7, "snapshot"] }]) installViews(malformed);
  installViews({ views: [
    { command: "center_home" },
    { command: "center_home", args: null, value: "invalid" },
    { command: "center_home", args: [], value: "invalid" },
    { command: "center_home", error: 7 },
    { command: 7, value: "invalid" },
  ] });
  assert.equal(snapshotView("center_home"), null);
  installViews({ views: [{ command: "center_home", value: null, error: "source failure" }] });
  assert.deepEqual(takeSnapshotView("center_home"), { kind: "error", error: "source failure" });
  assert.equal(takeSnapshotView("center_home"), null);
});

test("oversized projections evict the replaced value without exceeding the cache budget", t => {
  t.after(clearViews);
  installViews({ views: [{ command: "center_home", value: "small" }, { command: "agent_accounts", value: [] }] });
  installViews({ views: [{ command: "center_home", value: "x".repeat(8 * 1024 * 1024) }] });
  assert.equal(snapshotView("center_home"), null);
  assert.deepEqual(snapshotView("agent_accounts"), { kind: "value", value: [] });
});
