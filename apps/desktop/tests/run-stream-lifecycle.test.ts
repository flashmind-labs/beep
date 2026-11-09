import "./chat-dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { makeRuns } from "../src/project/runs.ts";
import { button, el } from "../src/ui/dom.ts";

const settle = () => new Promise<void>(done => setImmediate(done));

test("reopening Actions restores the selected workflow detail subscription", async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const invoke = (async (command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return command === "center_project_runs"
      ? { runs: [], checks: [], synced_at: null, error: null, rate_limited_until: null }
      : null;
  }) as never;
  const runs = makeRuns({ invoke, h: el, button, redraw() {}, openUrl() {}, fix() {} });

  try {
    runs.section({ project_id: "example", name: "example" });
    runs.show("example", 42);
    await settle();
    runs.stop();
    await settle();
    runs.section({ project_id: "example", name: "example" });
    await settle();
    assert.deepEqual(calls.filter(call => call.command === "follow_project_run").map(call => call.args?.runId), [42, null, 42]);
  } finally {
    runs.stop();
    await settle();
  }
});
