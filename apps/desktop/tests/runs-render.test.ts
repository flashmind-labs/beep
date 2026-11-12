import "./git-history-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { makeRuns, type RunsRow } from "../src/project/runs.ts";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { PageCache } from "../src/ui/pagecache.ts";
import { invoke } from "../src/ui/tauri.ts";

test("unchanged Actions redraws keep the visible run rows", () => {
  const kept = new PageCache(null).section<RunsRow>("runs");
  kept.put("project", {
    runs: [{ id: 1, number: 1, workflow: "CI", title: "Build", branch: "main",
      event: "push", actor: "", state: "passed", url: "", started_at: null,
      finished_at: null, progress: null, pull_requests: [] }],
    checks: [], synced_at: null, error: null, rate_limited_until: null,
  });
  const runs = makeRuns({ invoke, kept, redraw() {}, openUrl() {}, fix() {} });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const draw = () => flushSync(() => root.render(runs.section({ project_id: "project", name: "Project" })));
  draw();
  try {
    const row = host.querySelector(".pj-runrow");
    assert.ok(row);
    for (let i = 0; i < 3; i++) {
      draw();
      assert.equal(host.querySelectorAll(".pj-runrow").length, 1);
      assert.equal(host.querySelector(".pj-runrow"), row);
    }
  } finally {
    runs.stop();
    root.unmount();
    host.remove();
  }
});
