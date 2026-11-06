// "Remove from this machine" on a row of a project's "Where it lives": the
// machine stops syncing the project and leaves its list; its folder stays.
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
import type { WhereRow } from "./projects-state.ts";

/// Why `m` cannot leave `project` now, or null.
function blocked(project: string, rows: readonly WhereRow[], m: WhereRow) {
  const running = (state.home?.running || []).filter((r): r is RunningAgent => r.kind === "agent");
  return removeBlock(project, rows, m, m.folder ? workingIn(running, m.id, m.folder) : 0);
}

/// Asks, then takes `project` off `m`.
function confirmRemove(project: string, m: WhereRow) {
  confirmSheet({
    title: `Remove ${project} from ${m.name}?`,
    text: `It stops syncing to ${m.name} and leaves the project's machines.`,
    note: `The folder on ${m.name} stays on disk as it is.`,
    action: "Remove",
    run: async () => {
      await invoke("project_remove_from_machine", { project, machineId: m.id });
      refresh();
      await loadWhere(project);
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
