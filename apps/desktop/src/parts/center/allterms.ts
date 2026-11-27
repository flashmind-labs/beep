// Every terminal on every machine, held for the Terminals page, Home's section and the sidebar.
import { formatAgo } from "../../ui/format.ts";
import { invoke, indexedChannel } from "../../ui/tauri.ts";
import { lazyScreen, makeTerminals } from "../../stores/terminals/terminal-state.ts";
import { ALL } from "../../stores/terminals/terminal-list.ts";
import { keptSection, state } from "../../stores/center/state.ts";
import { currentLocation, drawPage, go, drawTop } from "./nav.ts";
import { signal } from "../../ui/signal.ts";

/// The terminals on any machine changed, for Home's section and the sidebar.
export const termsChanged = signal();

export const allTerms = makeTerminals({
  invoke, channel: indexedChannel, makeScreen: () => lazyScreen(() => import("../terminals/termview.ts").then(m => m.makeScreen)), ago: ms => formatAgo(ms, "short"), kept: keptSection("terminals"),
  redraw: () => {
    if (currentLocation()?.view === "terminals") { drawPage("terminals"); drawTop(); }

    termsChanged.notify();
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
