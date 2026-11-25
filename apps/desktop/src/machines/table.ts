// The command center's machines table: one row per machine, its terminals
// under it, and the menu that renames or removes one.
import { invoke } from "../ui/tauri.ts";
import { confirmSheet, openMenu, renameSheet, type MenuItem } from "../ui/menu.tsx";
import { pages, state, homeDataChanged } from "../center/state.ts";
import { currentLocation } from "../center/nav.ts";
import { openTerminals } from "../center/allterms.ts";
import { fsync } from "../project/sections.ts";
import { removeListed, renameListed, MACHINES, takeMac } from "./machine-store.ts";
import { type MacControl } from "./machine-state.ts";

/// How old the table lets a moving project's detail get.
/// Reads the sync detail of the projects in `state.sync` not read yet, then
/// draws them, while Home shows.
export function readSyncDetails() {
  if (currentLocation()?.view !== "home") return;
  const reads = [...new Set(state.sync.map(r => r.project))].map(p => fsync.ensure(p)).filter(r => r);
  if (reads.length) Promise.all(reads).then(() => homeDataChanged.notify());
}

/// Turns on Let Beep use this Mac, so it joins as a machine.
export async function useThisMac() {
  try {
    takeMac(await invoke<MacControl>("set_mac_control", { enabled: true }));
  } catch (e) {
    console.warn("turning on this computer:", e);
  }

  homeDataChanged.notify();
}

/// A machine the menu can rename, or remove unless it is this Mac, which
/// unlinks from Settings.
interface Removable {
  id: string;
  name: string;
  online?: boolean;
  can_host?: boolean;
}

/// The menu for machine `m`, with the lines `first` gives on top: a terminal
/// on it when it is online, renaming it, and removing it, which this Mac cannot do to itself.
export function showMachineMenu(m: Removable, first: () => MenuItem[], e: MouseEvent) {
  if (!m.id) return;
  const items: MenuItem[] = [...first()];
  if (m.online !== false && m.can_host !== false) items.push({ label: "New terminal", on: () => openTerminals({ newOn: m.id }) });
  items.push({ label: "Rename", on: () => renameMachine(m) });
  if (m.id !== MACHINES.mac?.id) items.push({ label: "Remove", warn: true, on: () => removeMachine(m) });
  if (items.length) openMenu(e, m.name, items);
}

/// Asks for a new name for `m` and shows it on every machine.
function renameMachine(m: Removable) {
  renameSheet({
    title: `Rename ${m.name}`,
    value: m.name,
    run: async name => {
      await invoke("rename_machine", { id: m.id, name });
      renameListed(m.id, name);
    },
  });
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
      removeListed(m.id);
      pages.drop("where");
    },
  });
}

