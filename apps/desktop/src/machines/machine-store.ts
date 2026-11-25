// The machines of the mesh as the `machines` push carries them: one copy for every screen.
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedList, pushedView } from "../ui/snapshot-state.ts";
import { invoke } from "../ui/tauri.ts";
import { takeClocks } from "./clock.ts";
import type { ClockOff } from "./clock-state.ts";
import type { MacControl } from "./machine-state.ts";
import type { Machine } from "./machines.tsx";
import type { MachineTraffic } from "./traffic-state.ts";

/// `list` is null before the first value. `failed` says the last read failed, so a missing list
/// is not an empty mesh.
/// `traffic` is what each machine moved, from the `telemetry` topic, whichever screen follows it.
/// `mac` is this computer's own state and `updatable` the machines that update over `update/1`,
/// both from the `machines` push: none until it comes.
export const MACHINES: { list: Machine[] | null; failed: boolean; traffic: MachineTraffic[]; mac: MacControl | null; updatable: Set<string> } = { list: null, failed: false, traffic: [], mac: null, updatable: new Set() };
/// Machines this window removed that a pushed list still has: Home hides them until the push drops them.
export const REMOVED = new Set<string>();
/// Called after the list changed, from a push or a rename or removal made here.
export const machinesChanged = new Set<() => void>();
let first: Promise<Machine[]> | null = null;

let macFirst: Promise<MacControl> | null = null;

/// This computer's state; before the first push arrives, one read answers. A pushed value that
/// comes while that read is out wins over its answer.
export function ensureMac(): Promise<MacControl> {
  if (MACHINES.mac) return Promise.resolve(MACHINES.mac);

  macFirst ??= invoke<MacControl>("mac_control")
    .then(mac => { if (!MACHINES.mac) takeMac(mac); return MACHINES.mac ?? mac; })
    .finally(() => { macFirst = null; });

  return macFirst;
}

/// Takes `mac` as this computer's state, from a push or from the answer to a change made here.
export function takeMac(mac: MacControl) {
  if (JSON.stringify(mac) === JSON.stringify(MACHINES.mac)) return;

  MACHINES.mac = mac;
  for (const f of machinesChanged) f();
}

/// The machines held; before the first push arrives, one read answers. A pushed list that comes
/// while that read is out wins over its answer.
export function ensureMachines(): Promise<Machine[]> {
  if (MACHINES.list) return Promise.resolve(MACHINES.list);

  first ??= invoke<Machine[]>("list_machines")
    .then(list => { if (!MACHINES.list) take(list); MACHINES.failed = false; return MACHINES.list ?? list; }, (e: unknown) => { MACHINES.failed = true; throw e; })
    .finally(() => { first = null; });

  return first;
}

function take(list: Machine[]) {
  MACHINES.list = list;
  MACHINES.failed = false;
  for (const f of machinesChanged) f();
}

/// Shows machine `id` under `name` after this window renamed it, until the push confirms it.
export function renameListed(id: string, name: string) {
  if (MACHINES.list) take(MACHINES.list.map(m => (m.id === id ? { ...m, name } : m)));
}

/// Drops machine `id` after this window removed it, until the push confirms it. A push that
/// already dropped it needs no mark, which only a later push without the id would clear.
export function removeListed(id: string) {
  if (!MACHINES.list || MACHINES.list.some(m => m.id === id)) REMOVED.add(id);
  if (MACHINES.list) take(MACHINES.list.filter(m => m.id !== id));
  else for (const f of machinesChanged) f();
}

subscribeEach(topic("machines"), event => {
  const mac = pushedView<MacControl>(event.data, "mac_control");
  const updatable = pushedList<{ id: string }>(event.data, "updatable_machines");
  if (mac && "error" in mac) console.warn("this computer's state:", mac.error);
  if (mac && "value" in mac) takeMac(mac.value);
  if (updatable && "list" in updatable) MACHINES.updatable = new Set(updatable.list.map(u => u.id));

  const clocks = pushedList<ClockOff>(event.data, "machine_clocks");
  if (clocks && "error" in clocks) console.warn("machine clocks:", clocks.error);
  if (clocks && "list" in clocks) takeClocks(clocks.list);

  const pushed = pushedList<Machine>(event.data, "list_machines");
  if (!pushed) return;
  if ("error" in pushed) { MACHINES.failed = true; return; }

  const ids = new Set(pushed.list.map(m => m.id));
  for (const id of REMOVED) if (!ids.has(id)) REMOVED.delete(id);
  take(pushed.list);
});

/// Takes the traffic a `telemetry` update carries; false when it holds none or none changed.
export function takeTraffic(data: unknown): boolean {
  if (typeof data !== "object" || data === null || !("traffic" in data) || !Array.isArray(data.traffic)) return false;

  const traffic = data.traffic.filter((t): t is MachineTraffic => typeof t === "object" && t !== null && "id" in t && typeof t.id === "string" && "counters" in t && Array.isArray(t.counters));
  if (JSON.stringify(traffic) === JSON.stringify(MACHINES.traffic)) return false;

  MACHINES.traffic = traffic;
  return true;
}
