import "./git-history-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { makeRuns, type RunsRow } from "../src/project/runs.ts";
import { el, button } from "../src/ui/dom.ts";
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
  const draw = () => runs.section({ project_id: "project", name: "Project" });
  const host = draw();
  document.body.append(host);
  try {
    const row = host.querySelector(".pj-runrow");
    assert.ok(row);
    for (let i = 0; i < 3; i++) {
      assert.equal(draw(), host);
      assert.equal(host.querySelectorAll(".pj-runrow").length, 1);
      assert.equal(host.querySelector(".pj-runrow"), row);
    }
  } finally {
    runs.stop();
    host.remove();
  }
});
