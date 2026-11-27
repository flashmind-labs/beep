import { placeWindowParts } from "../../parts/center/window-root.tsx";
import { windowHost } from "../../parts/center/window-shell.tsx";
import { UsageWidget } from "./usage-view.tsx";
import { subscribeChanges, topic } from "../../ui/mesh-events.ts";
import { AMBIENT_MS } from "../../ui/active.ts";
import { pushedView } from "../../ui/snapshot-state.ts";
// The pill at the right end of every command center header: how much of each
// coding agent's plan is used and what the agents cost today.

import { hasUsage, type PeriodKey, type UsageView } from "../../stores/center/usage-state.ts";

let allProjects = false;

/// Draws the header's usage pill and keeps it current.
export function mountUsage(read: (refresh: boolean) => Promise<UsageView | null>) {
  let view: UsageView | null = null;
  let loaded = false;
  let open = false;
  let period: PeriodKey = "today";
  const pick = (key: PeriodKey) => { period = key; draw(); };
  const draw = () => {
    if (!hasUsage(view)) { placeWindowParts({ usage: null }); return; }
    const current = view;
    placeWindowParts({ usage: <UsageWidget view={current} open={open} period={period} allProjects={allProjects}
      toggle={() => toggle(!open)} pick={pick} more={() => { allProjects = !allProjects; draw(); }} /> });
  };

  const take = (next: UsageView | null) => {
    if (loaded && JSON.stringify(next) === JSON.stringify(view)) return;
    loaded = true;
    view = next;
    draw();
  };

  const load = async (refresh: boolean) => {
    try {
      take(await read(refresh));
    } catch (e) {
      console.warn("reading usage:", e);
    }
  };

  const toggle = (to: boolean) => {
    open = to;
    draw();
    if (!open) return;
    void load(true);
  };

  document.addEventListener("click", e => { if (open && !e.composedPath().includes(windowHost("usage"))) toggle(false); });
  document.addEventListener("keydown", e => { if (open && e.key === "Escape") toggle(false); });
  // Spending changes with every turn; the pill follows at most once a second.
  subscribeChanges(topic("usage"), { deliver: "each", receive: event => {
    const pushed = pushedView<UsageView | null>(event.data, "usage_get");
    if (!pushed) return;
    if ("error" in pushed) console.warn("reading usage:", pushed.error);
    else take(pushed.value);
  } }, { wait: AMBIENT_MS });
  void load(false);
}
