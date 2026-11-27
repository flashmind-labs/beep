// "Remove from this machine" on a row of a project's "Where it lives": the
// machine stops syncing the project and leaves its list; its folder stays,
// unless the user ticks to delete it there too.
import { invoke } from "../../ui/tauri.ts";
import { confirmSheet, type MenuItem } from "../../ui/menu.tsx";
import { state } from "../../stores/center/state.ts";
import { removeBlock, workingIn } from "../../stores/project/removemachine-state.ts";
import type { RunningAgent } from "../../stores/agents/running-state.ts";
import { plainPath } from "../../parts/project/filesync.ts";
import type { WhereRow } from "../../stores/project/projects-state.ts";

/// Why `m` cannot leave `project` now, or null.
export function blocked(project: string, rows: readonly WhereRow[], m: WhereRow) {
  const running = (state.home?.running || []).filter((r): r is RunningAgent => r.kind === "agent");
  return removeBlock(project, rows, m, m.folder ? workingIn(running, m.id, m.folder) : 0);
}

/// Asks, then takes `project` off `m`, and deletes its folder there when ticked.
export function confirmRemove(project: string, m: WhereRow) {
  const folder = m.folder ? plainPath(m.folder) : "The folder";
  confirmSheet({
    title: `Remove ${project} from ${m.name}?`,
    text: `It stops syncing to ${m.name} and leaves the project's machines.`,
    note: `${folder} on ${m.name} stays on disk as it is.`,
    tick: {
      label: `Also delete the folder on ${m.name}`,
      note: `${folder} is deleted on ${m.name} once it stops syncing there. The other machines keep their copies.`,
    },
    action: "Remove",
    dismissWhileBusy: true,
    run: deleteFolder => invoke<string | null>("project_remove_from_machine", { project, machineId: m.id, deleteFolder }),
  });
}

/// The row menu's line for it; none when it cannot, as the x says why.
export function removeMenuItems(project: string, rows: readonly WhereRow[], m: WhereRow): MenuItem[] {
  if (m.kind !== "here" || blocked(project, rows, m)) return [];
  return [{ label: "Remove from this machine", warn: true, on: () => confirmRemove(project, m) }];
}
