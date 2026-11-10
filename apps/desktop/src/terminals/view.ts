// The command center's Terminals page, and the terminals Home lists.
import { ago } from "../ui/format.ts";
import { createElement } from "react";
import { invoke, Channel } from "../ui/tauri.ts";
import { ALL, byMachine, makeTerminals, terminalsCount } from "./terminals.ts";
import { makeScreen } from "./termview.ts";
import { state, views } from "../center/state.ts";
import { draw } from "../center/home.ts";
import { currentLocation, registerView, go, drawTop } from "../center/views.ts";
import { drawSide } from "../center/sidebar.ts";
import { refresh } from "../center/refresh.ts";
import { removeWindowPart, showWindowParts } from "../center/window-root.ts";

/// Every terminal on every machine: the Terminals page, and Home's section.
export const allTerms = makeTerminals({
  invoke, Channel, makeScreen, ago,
  redraw: () => {
    if (currentLocation()?.view === "terminals") { drawTerminals(); drawTop(); }
    else if (currentLocation()?.view === "home") draw();

    drawSide();
  },
});

/// The machines terminals are listed under, as Home has them.
export const termMachines = () => (state.home?.machines || []).filter(m => m.can_host !== false).map(m => ({ id: m.id, name: m.name, online: m.online }));

/// Opens the Terminals page with terminal `id`, or the New terminal form on
/// machine `newOn`.
export function openTerminals(to: { id: string } | { newOn: string | null } | null) {
  go("terminals");
  if (to && "id" in to) allTerms.show(to.id);
  else if (to) allTerms.newOn(to.newOn);
}

/// The Terminals page into its view, keeping the list's scroll.
function drawTerminals() {
  const el = views.get("terminals")?.el;
  if (!el) return;
  showWindowParts([{ part: "terminal-page", host: el, content: createElement("div", { className: "cc-view tl-view" }, allTerms.machinesPanel(termMachines())) }]);
}

registerView("terminals", {
  title() {
    const list = allTerms.list(ALL);
    const sub = list ? terminalsCount(byMachine(termMachines(), list)) : "";
    const add = createElement("button", { type: "button", className: "dbtn pri small", disabled: !termMachines().some(m => m.online), onClick: () => allTerms.newOn(termMachines().find(m => m.online)?.id || null) }, "New terminal");
    return { title: "Terminals", sub, reactTools: list === null ? null : add };
  },
  mount() {
    drawTerminals();
    allTerms.load(ALL);
    refresh();
  },
  unmount() {
    // Detached, not stopped: they keep running on their machines.
    allTerms.stop();
    removeWindowPart("terminal-page");
  },
});
