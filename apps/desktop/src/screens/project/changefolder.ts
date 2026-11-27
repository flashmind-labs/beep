// "Change folder" on a machine of a project: points the project there at
// another folder. Nothing is moved, and the folder it had stays on disk as
// it is.
import type { MenuItem } from "../../ui/menu.tsx";
import { openAddToMachine } from "./addmachine.tsx";

/// Opens the sheet for `project` on `machine`, whose folder is `from` now.
export function changeFolder(project: string, machine: { id: string; name: string }, from: string) {
  openAddToMachine({
    project,
    machine,
    from,
  });
}

/// The row menu's line for it, on a machine with the project that is online.
export function changeFolderItems(project: string, m: { id: string; name: string; kind: string; folder: string | null }): MenuItem[] {
  if (m.kind !== "here" || !m.folder) return [];
  const from = m.folder;
  return [{ label: "Change folder", on: () => changeFolder(project, m, from) }];
}
