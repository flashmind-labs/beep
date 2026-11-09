import "./git-history-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { makeRuns, type RunsRow } from "../src/project/runs.ts";
import { el, button } from "../src/ui/dom.ts";
import { patchChildren } from "../src/ui/patch.ts";
import { PageCache } from "../src/ui/pagecache.ts";
import { invoke } from "../src/ui/tauri.ts";

test("unchanged Actions redraws keep the visible run rows", () => {
  const kept = new PageCache(null).section<RunsRow | null>("runs");
  kept.put("project", {
    runs: [{ id: 1, number: 1, workflow: "CI", title: "Build", branch: "main",
      event: "push", actor: "", state: "passed", url: "", started_at: null,
      finished_at: null, progress: null, pull_requests: [] }],
    checks: [], synced_at: null, error: null, rate_limited_until: null,
  });
  const runs = makeRuns({ invoke, h: el, button, kept, redraw() {}, openUrl() {}, fix() {} });
  const container = el("div");
  document.body.append(container);
  try {
    const draw = () => patchChildren(container, [runs.section({ project_id: "project", name: "Project" })]);
    draw();
    const row = container.querySelector(".pj-runrow");
    assert.ok(row);
    for (let i = 0; i < 3; i++) {
      draw();
      assert.equal(container.querySelectorAll(".pj-runrow").length, 1);
      assert.equal(container.querySelector(".pj-runrow"), row);
    }
  } finally {
    runs.stop();
    container.remove();
  }
});
