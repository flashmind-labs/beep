// The machines of the mesh as the `machines` push carries them: one copy for every screen.
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedList } from "../ui/snapshot-state.ts";
import { invoke } from "../ui/tauri.ts";
import type { Machine } from "./machines.tsx";

/// `list` is null before the first value. `failed` says the last read failed, so a missing list
/// is not an empty mesh.
export const MACHINES: { list: Machine[] | null; failed: boolean } = { list: null, failed: false };
let first: Promise<Machine[]> | null = null;

/// The machines held; before the first push arrives, one read answers. A pushed list that comes
/// while that read is out wins over its answer.
export function ensureMachines(): Promise<Machine[]> {
  if (MACHINES.list) return Promise.resolve(MACHINES.list);

  first ??= invoke<Machine[]>("list_machines")
    .then(list => { MACHINES.list ??= list; MACHINES.failed = false; return MACHINES.list; }, (e: unknown) => { MACHINES.failed = true; throw e; })
    .finally(() => { first = null; });

  return first;
}

/// Takes `list` after a change this window made, until the push confirms it.
export function setMachines(list: Machine[]) {
  MACHINES.list = list;
  MACHINES.failed = false;
}

subscribeEach(topic("machines"), event => {
  const pushed = pushedList<Machine>(event.data, "list_machines");
  if (!pushed) return;
  if ("error" in pushed) { MACHINES.failed = true; return; }

  MACHINES.list = pushed.list;
  MACHINES.failed = false;
});
