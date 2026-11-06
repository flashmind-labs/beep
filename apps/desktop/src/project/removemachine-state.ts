// When a machine of a project's "Where it lives" can be taken off it, as pure
// functions. removemachine.ts draws the button and the sheet.
import type { RunningAgent } from "../agents/running-state.ts";
import type { WhereRow } from "./projects-state.ts";

export const LAST_MACHINE = "A project stays on at least one machine.";

/// The agents open in the folder `folder` of machine `machineId`, or under it.
export function workingIn(running: readonly RunningAgent[], machineId: string, folder: string): number {
  return running.filter(r => r.kind === "agent" && r.machine_id === machineId && (r.cwd === folder || r.cwd?.startsWith(folder + "/"))).length;
}

/// Why `machine` cannot be taken off `project`, or null when it can: the last
/// machine with it keeps it, and an agent working there is stopped first.
export function removeBlock(project: string, rows: readonly Pick<WhereRow, "kind">[], machine: Pick<WhereRow, "name">, working: number): string | null {
  if (rows.filter(r => r.kind === "here").length < 2) return LAST_MACHINE;
  if (working > 0) return `An agent is working in ${project} on ${machine.name}. Stop it first.`;
  return null;
}
