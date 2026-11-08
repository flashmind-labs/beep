import test from "node:test";
import assert from "node:assert/strict";
import { clearViews, installViews, snapshotView, takeSnapshotView, viewWrite } from "../src/ui/snapshot-state.ts";

test("snapshots match command arguments, preserve null and keep caller changes private", t => {
  t.after(clearViews);
  installViews({ views: [
    { command: "center_project_runs", args: { projectId: "example" }, value: { runs: [1] } },
    { command: "agent_accounts", args: {}, value: null },
  ] });
  const read = snapshotView("center_project_runs", { refresh: false, projectId: "example" })!;
  (read.value as { runs: number[] }).runs.push(2);
  assert.deepEqual(snapshotView("center_project_runs", { projectId: "example" }), { value: { runs: [1] } });
  assert.equal(snapshotView("center_project_runs", { projectId: "another" }), undefined);
  assert.equal(snapshotView("center_project_runs", { projectId: "example", refresh: true }), undefined);
  assert.deepEqual(snapshotView("agent_accounts"), { value: null });
});

test("only read views are accepted and cache size stays bounded", t => {
  t.after(clearViews);
  installViews({ views: [{ command: "agent_start", args: {}, value: "unsafe" }] });
  assert.equal(snapshotView("agent_start"), undefined);
  installViews({ views: Array.from({ length: 129 }, (_, index) => ({
    command: "center_project_run", args: { projectId: "example", runId: index }, value: index,
  })) });
  assert.equal(snapshotView("center_project_run", { projectId: "example", runId: 0 }), undefined);
  assert.deepEqual(snapshotView("center_project_run", { projectId: "example", runId: 128 }), { value: 128 });
  assert.equal(viewWrite("save_agent_token"), true);
  assert.equal(viewWrite("agent_tree_edit"), true);
  assert.equal(viewWrite("center_project_runs"), false);
});

test("source errors replace successful projections, then a retry can read fresh state", t => {
  t.after(clearViews);
  const args = { projectId: "example" };
  installViews({ views: [{ command: "center_project_runs", args, value: { runs: [1] } }] });
  installViews({ views: [{ command: "center_project_runs", args, error: "temporary source failure" }] });
  assert.deepEqual(snapshotView("center_project_runs", args), { error: "temporary source failure" });
  assert.deepEqual(takeSnapshotView("center_project_runs", args), { error: "temporary source failure" });
  assert.equal(snapshotView("center_project_runs", args), undefined);
  installViews({ views: [{ command: "center_project_runs", args, value: { runs: [1] } }] });
  assert.deepEqual(snapshotView("center_project_runs", args), { value: { runs: [1] } });
});
