import type { invoke as Invoke } from "../ui/tauri.ts";
import { type ClockOff, clockLine, clockTitle } from "./clock-state.ts";

/// The offset of each online machine whose clock is off, by machine id.
const OFF = new Map<string, number>();

/// Reads the machines whose clock is off; a failure keeps what was known.
export async function loadClocks(invoke: typeof Invoke) {
  try {
    const list = await invoke<ClockOff[]>("machine_clocks");
    OFF.clear();
    for (const c of list) OFF.set(c.id, c.offset_ms);
  } catch (e) {
    console.warn("machine clocks:", e);
  }
}

/// The warning for a machine, or null when its clock is fine or not known.
export function clockWarning(id: string, os: string | null) {
  const offset = OFF.get(id);
  return offset === undefined ? null : { text: clockLine(offset), title: clockTitle(os) };
}
