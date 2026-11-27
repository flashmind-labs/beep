import "./git-history-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { OverviewCardView, overviewCardModel } from "../src/screens/project/overview-card.tsx";

test("pin and activity updates are shown on the corresponding project cards", () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const project = { name: "first", url: null, display_name: null, machines: [], agents: 0, last: null, imported: true, i: 0, repo: "first", pinned: false };
  const second = { ...project, name: "second", repo: "second", i: 1 };
  const base = overviewCardModel(project, "Beep sync");
  const other = overviewCardModel(second, null);
  const open = () => {}, pin = () => {}, none = () => {};
  const windowProps = { clicks: { onClickCapture: none, onAuxClick: none, onMouseEnter: none, onMouseLeave: none }, menu: { onContextMenu: none } };
  const render = (first: typeof base, second: typeof base) => flushSync(() => root.render(createElement("div", null,
    createElement(OverviewCardView, { model: first, open, pin, windowProps }),
    createElement(OverviewCardView, { model: second, open, pin, windowProps }))));
  try {
    render(base, other);
    const firstCard = () => host.querySelector('[role="button"][title="first"]');
    const secondCard = () => host.querySelector('[role="button"][title="second"]');
    assert.match(firstCard()?.textContent ?? "", /No activity.*Beep sync/);

    const pinned = overviewCardModel({ ...project, pinned: true, agents: 2 }, "Syncthing");
    render(pinned, other);
    assert.ok(firstCard()?.querySelector('[aria-label="Unpin"]'));
    assert.match(firstCard()?.textContent ?? "", /2 agents.*Syncthing/);

    render(pinned, overviewCardModel({ ...second, last: Date.now() - 120_000 }, null));
    assert.ok(secondCard()?.querySelector('[aria-label="Pin to top"]'));
    assert.match(secondCard()?.textContent ?? "", /2 min ago/);
    assert.doesNotMatch(firstCard()?.textContent ?? "", /2 min ago/);
  } finally { root.unmount(); host.remove(); }
});
