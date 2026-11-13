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
  const base = { name: "first", repo: "first", index: 0, pinned: false, sync: "", agents: "idle", latest: "No recent activity", hasMachines: false, machines: "" };
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

test("overview card display models reflect activity, agents, pin and sync", () => {
  const project = { name: "example/project", url: null, display_name: null, machines: [], agents: 0, last: Date.now() - 120_000, imported: true, i: 0, repo: "project", pinned: false };
  const before = overviewCardModel(project, "Beep sync");
  assert.equal(before.latest, "Last activity 2 min ago");
  assert.equal(before.agents, "idle");
  assert.equal(before.sync, "Beep sync");
  const after = overviewCardModel({ ...project, pinned: true, agents: 2 }, "Syncthing");
  assert.equal(after.pinned, true);
  assert.equal(after.agents, "2 agents");
  assert.equal(after.sync, "Syncthing");
});
