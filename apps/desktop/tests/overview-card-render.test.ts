import "./git-history-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { OverviewCardView, overviewCardModel } from "../src/project/overview-card.tsx";

test("overview cards update independently when another project's display changes", () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const base = { name: "first", repo: "first", index: 0, pinned: false, sync: "", workers: "idle", hasSummary: false, open: "", rest: "", running: 0, review: 0, blocked: 0, planned: 0, done: 0, latest: "No recent activity", latestAge: "", hasMachines: false, machines: "" };
  const other = { ...base, name: "second", repo: "second" };
  let nameReads = 0;
  const open = () => {}, pin = () => {}, none = () => {};
  const windowProps = { clicks: { onClickCapture: none, onAuxClick: none, onMouseEnter: none, onMouseLeave: none }, menu: { onContextMenu: none } };
  const render = (first: typeof base, second: typeof base) => flushSync(() => root.render(createElement("div", null,
    createElement(OverviewCardView, { model: first, open, pin, windowProps }),
    createElement(OverviewCardView, { model: second, open, pin, windowProps }))));
  try {
    render(base, other);
    const unchanged = host.querySelector('[title="second"]');
    const tracked = { ...other };
    Object.defineProperty(tracked, "name", { enumerable: true, get: () => { nameReads++; return "second"; } });
    render({ ...base, pinned: true }, tracked);
    assert.equal(host.querySelector('[title="second"]'), unchanged);
    assert.equal(nameReads, 1, "the unchanged card model was compared but not rendered");
    assert.ok(host.querySelector('[title="first"] [aria-label="Unpin"]'));
    render({ ...base, pinned: true }, { ...other, latest: "Last activity 2 min ago" });
    assert.match(host.textContent ?? "", /Last activity 2 min ago/);
  } finally { root.unmount(); host.remove(); }
});

test("overview card display models reflect live summaries, pin and relative age", () => {
  const project = { name: "example/project", url: null, machines: [], agents: 0, last: Date.now() - 120_000, imported: true, i: 0, repo: "project", pinned: false, sum: null };
  const before = overviewCardModel(project, "Beep sync");
  assert.equal(before.latest, "Last activity 2 min ago");
  assert.equal(before.sync, "Beep sync");
  const after = overviewCardModel({ ...project, pinned: true, sum: {
    project_id: "project", name: project.name, workers: 2,
    tasks: { running: 1, review: 0, blocked: 0, planned: 0, done: 2 },
    latest: { slug: "task", title: "Build", state: "running", pr_url: null, updated_at: Date.now() - 120_000 },
  } }, "Syncthing");
  assert.equal(after.pinned, true);
  assert.equal(after.running, 1);
  assert.equal(after.workers, "2 workers");
  assert.equal(after.latest, "Working on Build");
  assert.equal(after.latestAge, "2 min");
  assert.equal(after.sync, "Syncthing");
});
