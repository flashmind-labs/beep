// "Change folder…" on a machine of a project: points the project there at
// another folder, empty or missing (the others fill it) or a checkout of the
// same repository (it joins the sync). Nothing is moved, and the folder it
// had stays on disk as it is.
import { state } from "../center/state.ts";
import { refresh } from "../center/refresh.ts";
import type { MenuItem } from "../ui/menu.ts";
import { openAddToMachine } from "./addmachine.ts";
import { loadWhere } from "./loads.ts";

/// Opens the sheet for `project` on `machine`, whose folder is `from` now.
export function changeFolder(project: string, machine: { id: string; name: string }, from: string) {
  openAddToMachine({
    project,
    machine,
    from,
    os: state.home?.machines?.find(x => x.id === machine.id)?.env?.os,
    macId: state.mac?.id || null,
    added: () => { refresh(); loadWhere(project); },
  });
}

/// The row menu's line for it, on a machine with the project that is online.
export function changeFolderItems(project: string, m: { id: string; name: string; kind: string; folder: string | null }): MenuItem[] {
  if (m.kind !== "here" || !m.folder) return [];
  const from = m.folder;
  return [{ label: "Change folder…", on: () => changeFolder(project, m, from) }];
}
