// The machines of the mesh as `list_machines` answers: one copy for every screen.
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { snapshotView } from "../ui/snapshot-state.ts";
import { invoke } from "../ui/tauri.ts";
import type { Machine } from "./machines.tsx";

/// `list` is null before the first read. `failed` says the last read threw, so
/// a missing list is not an empty mesh.
export const MACHINES: { list: Machine[] | null; failed: boolean } = { list: null, failed: false };
let stale = true;
let reading: Promise<Machine[]> | null = null;
/// Orders the reads: only the newest one's answer is kept.
let generation = 0;

/// Reads the machines now. Callers that ask together share one read, unless the mesh changed
/// them since it started: a newer read then starts and wins. While the `machines` push carries
/// the list, a read is answered from it.
export function loadMachines(): Promise<Machine[]> {
  if (reading && !stale) return reading;
  const mine = ++generation;
  stale = false;
  const read = invoke<Machine[]>("list_machines")
    .then(list => { if (mine === generation) { MACHINES.list = list; MACHINES.failed = false; } return list; }, (e: unknown) => { if (mine === generation) { MACHINES.failed = true; stale = true; } throw e; })
    .finally(() => { if (reading === read) reading = null; });
  reading = read;
  return read;
}

/// The machines as last read, read again only after the mesh changed them.
export function ensureMachines(): Promise<Machine[]> {
  return MACHINES.list && !stale ? Promise.resolve(MACHINES.list) : loadMachines();
}

/// Takes `list` after a change this window made, until the push confirms it.
export function setMachines(list: Machine[]) {
  generation++;
  MACHINES.list = list;
  MACHINES.failed = false;
}

subscribeEach(topic("machines"), () => {
  stale = true;
  if (!snapshotView("list_machines")) return;
  loadMachines().catch(e => console.warn("machines:", e));
});
