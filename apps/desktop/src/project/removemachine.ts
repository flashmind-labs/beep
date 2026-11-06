// "Remove from this machine" on a row of a project's "Where it lives": the
// machine stops syncing the project and leaves its list; its folder stays,
// unless the user ticks to delete it there too.
import { invoke } from "../ui/tauri.ts";
import { el } from "../ui/dom.ts";
import { dbtn, svgEl } from "../ui/parts.ts";
import { confirmSheet, type MenuItem } from "../ui/menu.ts";
import { IC } from "../ui/icons.ts";
import { state } from "../center/state.ts";
import { refresh } from "../center/refresh.ts";
import { loadWhere } from "./loads.ts";
import { removeBlock, workingIn } from "./removemachine-state.ts";
import type { RunningAgent } from "../agents/running-state.ts";
import { plainPath, type WhereRow } from "./projects-state.ts";

/// Why `m` cannot leave `project` now, or null.
function blocked(project: string, rows: readonly WhereRow[], m: WhereRow) {
  const running = (state.home?.running || []).filter((r): r is RunningAgent => r.kind === "agent");
  return removeBlock(project, rows, m, m.folder ? workingIn(running, m.id, m.folder) : 0);
}

/// Asks, then takes `project` off `m`, and deletes its folder there when ticked.
function confirmRemove(project: string, m: WhereRow) {
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
    run: async deleteFolder => {
      const said = await invoke<string | null>("project_remove_from_machine", { project, machineId: m.id, deleteFolder });
      refresh();
      await loadWhere(project);
      return said;
    },
  });
}

/// The quiet x at the end of a row with the project: off, with why, on the
/// last machine and while an agent works there.
export function removeButton(project: string, rows: readonly WhereRow[], m: WhereRow) {
  const why = blocked(project, rows, m);
  const b = dbtn(svgEl(IC.x) ?? "", "quiet small rmx", () => confirmRemove(project, m));
  b.setAttribute("aria-label", `Remove from ${m.name}`);
  if (!why) {
    b.title = `Remove from ${m.name}`;
    return b;
  }
  b.disabled = true;
  // A disabled button shows no tooltip, so the span around it does.
  const wrap = el("span", "rmx-off", b);
  wrap.title = why;
  return wrap;
}

/// The row menu's line for it; none when it cannot, as the x says why.
export function removeMenuItems(project: string, rows: readonly WhereRow[], m: WhereRow): MenuItem[] {
  if (m.kind !== "here" || blocked(project, rows, m)) return [];
  return [{ label: "Remove from this machine…", warn: true, on: () => confirmRemove(project, m) }];
}
