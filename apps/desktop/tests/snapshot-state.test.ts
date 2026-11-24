import test from "node:test";
import assert from "node:assert/strict";
import { pushedList, pushedView } from "../src/ui/snapshot-state.ts";

test("a pushed view is found by command and, when asked, by arguments", () => {
  const data = { views: [
    { command: "center_project_runs", args: { projectId: "example" }, value: { runs: [1] } },
    { command: "center_project_runs", args: { projectId: "another" }, value: { runs: [2] } },
    { command: "agent_accounts", args: {}, value: null },
  ] };
  assert.deepEqual(pushedView(data, "center_project_runs", { projectId: "another" }), { value: { runs: [2] } });
  assert.deepEqual(pushedView(data, "center_project_runs", { projectId: "example", refresh: undefined }), { value: { runs: [1] } });
  assert.deepEqual(pushedView(data, "center_project_runs"), { value: { runs: [1] } });
  assert.equal(pushedView(data, "center_project_runs", { projectId: "missing" }), null);
  assert.deepEqual(pushedView(data, "agent_accounts"), { value: null }, "null is a value");
  assert.equal(pushedView(data, "git_items"), null);
});

test("an error replaces the value of its view", () => {
  const data = { views: [{ command: "center_home", args: {}, error: "temporary source failure" }] };
  assert.deepEqual(pushedView(data, "center_home"), { error: "temporary source failure" });
});

test("a list is pushed as a list, or as why it failed", () => {
  const lists = { views: [{ command: "agent_hosts", value: [] }, { command: "agent_sessions", value: [1] }, { command: "list_machines", error: "down" }, { command: "mac_control", value: {} }] };
  assert.deepEqual(pushedList(lists, "agent_sessions"), { list: [1] });
  assert.deepEqual(pushedList(lists, "list_machines"), { error: "down" });
  assert.equal(pushedList(lists, "device_hosts"), null);
  assert.equal(pushedList(lists, "mac_control"), null, "a value that is not a list is not one");
});

test("wire views are validated before they are read", () => {
  for (const malformed of [null, [], { views: null }, { views: [null, false, 7, "snapshot"] }]) assert.equal(pushedView(malformed, "center_home"), null);
  const data = { views: [
    { command: "center_home" },
    { command: "center_home", args: null, value: "invalid" },
    { command: "center_home", args: [], value: "invalid" },
    { command: "center_home", error: 7 },
    { command: 7, value: "invalid" },
  ] };
  assert.equal(pushedView(data, "center_home"), null);
  assert.deepEqual(pushedView({ views: [{ command: "center_home", value: null, error: "source failure" }] }, "center_home"), { error: "source failure" });
});
