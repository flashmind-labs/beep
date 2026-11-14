// The command center's machines table: one row per machine, its terminals
// under it, and the menu that removes one.
import { invoke } from "../ui/tauri.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.tsx";
import { state } from "../center/state.ts";
import { currentLocation } from "../center/views.tsx";
import { draw } from "../center/home.tsx";
import { openTerminals } from "../terminals/view.tsx";
import { reloadGroup } from "../center/sidebar.tsx";
import { fsync } from "../project/sections.ts";
import { refresh } from "../center/refresh.ts";
import { type MacControl } from "./machine-state.ts";

/// How old the table lets a moving project's detail get.
const MOVING_MAX_AGE_MS = 5000;

/// Reads the sync detail of the projects in `state.sync` that are due, then
/// draws them, while Home shows.
export function readSyncDetails() {
  if (currentLocation()?.view !== "home") return;
  const moving = new Set(state.sync.filter(r => r.status === "syncing" || r.status === "scanning").map(r => r.project));
  const reads = [...new Set(state.sync.map(r => r.project))].map(p => fsync.ensure(p, moving.has(p) ? MOVING_MAX_AGE_MS : undefined)).filter(r => r);
  if (reads.length) Promise.all(reads).then(draw);
}

/// Turns on Let Beep use this Mac, so it joins as a machine.
export async function useThisMac() {
  try {
    state.mac = await invoke<MacControl>("set_mac_control", { enabled: true });
  } catch (e) {
    console.warn("turning on this computer:", e);
  }

  draw();
  // The Mac joining changes the `machines` topic, which reads the machines again.
  reloadGroup("machines");
}

/// A machine the window can offer to remove: any but this Mac, which
/// unlinks from Settings.
interface Removable {
  id: string;
  name: string;
  online?: boolean;
  can_host?: boolean;
}

/// The menu for machine `m`, with the lines `first` gives on top: a terminal
/// on it when it is online, and removing it, which this Mac cannot do to itself.
export function showMachineMenu(m: Removable, first: () => MenuItem[], e: MouseEvent) {
  if (!m.id) return;
  const items: MenuItem[] = [...first()];
  if (m.online !== false && m.can_host !== false) items.push({ label: "New terminal", on: () => openTerminals({ newOn: m.id }) });
  if (m.id !== state.mac?.id) items.push({ label: "Remove…", warn: true, on: () => removeMachine(m) });
  if (items.length) openMenu(e, m.name, items);
}

/// Asks, then removes `m` from the mesh.
function removeMachine(m: Removable) {
  confirmSheet({
    title: `Remove ${m.name}?`,
    text: m.can_host === false || m.online === false ? "It leaves the mesh." : "It leaves the mesh and agents running there stop.",
    note: m.can_host === false ? "Pair it again to reconnect." : "Its files stay on the machine.",
    action: "Remove",
    run: async () => {
      await invoke("remove_machine", { id: m.id });
      if (state.home) state.home = { ...state.home, machines: state.home.machines?.filter(x => x.id !== m.id) ?? null };
      state.where.clear();
      draw();
      reloadGroup("machines");
      refresh();
    },
  });
}

