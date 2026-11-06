import test from "node:test";
import assert from "node:assert/strict";
import { runningRow, type RunningProcess } from "../src/agentpane/running-state.ts";

const process: RunningProcess = {
  pid: 42, command: "npm run dev", started_at: new Date(2026, 0, 2, 9, 4).getTime() / 1000,
  ports: [3000, 8080], left_group: true,
};

test("running rows show a padded local time and escaped group label", () => {
  assert.deepEqual(runningRow(process, true), {
    command: "npm run dev", detail: "since 09:04 · left the group",
    ports: [{ label: ":3000", url: "http://localhost:3000" }, { label: ":8080", url: "http://localhost:8080" }],
  });
});

test("remote ports are plain text and group members need no group label", () => {
  const row = runningRow({ ...process, left_group: false, ports: [3000, -1, 0, 65536, 1.5] }, false);
  assert.equal(row.detail, "since 09:04");
  assert.deepEqual(row.ports, [{ label: ":3000", url: null }]);
});
