import type { invoke as Invoke } from "../../ui/tauri.ts";
import { type ClockOff, clockLine, clockTitle } from "./clock-state.ts";

/// The offset of each online machine whose clock is off, by machine id.
const OFF = new Map<string, number>();
/// Whether the clocks are known: from a `machines` push, or from the one read before it.
let known = false;
let reading: Promise<void> | null = null;

/// Takes the clocks a `machines` push carries.
export function takeClocks(list: ClockOff[]) {
  OFF.clear();
  for (const c of list) OFF.set(c.id, c.offset_ms);
  known = true;
}

/// Reads the clocks once, before the first push brings them; callers at the same time share the
/// request. A failed read leaves them unknown, and the next push brings them.
export function loadClocks(invoke: typeof Invoke): Promise<void> {
  if (known) return Promise.resolve();
  if (reading) return reading;

  reading = invoke<ClockOff[]>("machine_clocks")
    .then(list => { if (!known) takeClocks(list); })
    .catch(e => { console.warn("machine clocks:", e); })
    .finally(() => { reading = null; });

  return reading;
}

/// The warning for a machine, or null when its clock is fine or not known.
export function clockWarning(id: string, os: string | null) {
  const offset = OFF.get(id);
  return offset === undefined ? null : { text: clockLine(offset), title: clockTitle(os) };
}
