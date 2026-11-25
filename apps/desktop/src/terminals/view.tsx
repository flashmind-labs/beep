// The command center's Terminals page, and the terminals Home lists.
import { formatAgo } from "../ui/format.ts";
import { Button } from "../ui/kit.tsx";
import { invoke, indexedChannel } from "../ui/tauri.ts";
import { lazyScreen, makeTerminals } from "./terminals.tsx";
import { ALL, byMachine, terminalsCount } from "./terminal-list.ts";
import { type CenterView, keptSection, state } from "../center/state.ts";
import { draw } from "../center/home.tsx";
import { currentLocation, drawPage, go, drawTop } from "../center/nav.ts";
import { drawSide } from "../center/sidebar.tsx";
import { ensureHome } from "../center/refresh.ts";

/// Every terminal on every machine: the Terminals page, and Home's section.
export const allTerms = makeTerminals({
  invoke, channel: indexedChannel, makeScreen: () => lazyScreen(() => import("./termview.ts").then(m => m.makeScreen)), ago: ms => formatAgo(ms, "short"), kept: keptSection("terminals"),
  redraw: () => {
    if (currentLocation()?.view === "terminals") { drawTerminals(); drawTop(); }
    else if (currentLocation()?.view === "home") draw();

    drawSide();
  },
});

/// Reads every terminal once in this window, behind the list kept from
/// before; the terminals topic keeps it current after that.
let allRead = false;
export function ensureAllTerms() {
  if (!allRead) void readAllTerms();
}

/// Reads every terminal; a read already on its way answers for this one.
export function readAllTerms() {
  allRead = true;
  return allTerms.load(ALL);
}

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
  drawPage("terminals");
}

export function TerminalsPage() {
  return <div className="cc-view tl-view">{allTerms.machinesPanel(termMachines())}</div>;
}

export const terminalsView: CenterView<"terminals"> = {
  title() {
    const list = allTerms.list(ALL);
    const sub = list ? terminalsCount(byMachine(termMachines(), list)) : "";
    const add = <Button look="primary" small disabled={!termMachines().some(m => m.online)} onClick={() => allTerms.newOn(termMachines().find(m => m.online)?.id || null)}>New terminal</Button>;
    return { title: "Terminals", sub, reactTools: list === null ? null : add };
  },
  mount() {
    // The list the window holds draws at once; the terminals topic keeps it.
    drawTerminals();
    ensureAllTerms();
    ensureHome();
  },
  unmount() {
    // Detached, not stopped: they keep running on their machines.
    allTerms.stop();
    drawPage("terminals", false);
  },
};
