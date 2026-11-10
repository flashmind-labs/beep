import type { invoke as Invoke } from "../ui/tauri.ts";
import { type ClockOff, clockLine, clockTitle } from "./clock-state.ts";

/// How long a read of the machines' clocks stays good.
export const CLOCK_MAX_AGE_MS = 30_000;
/// The wait after a failed read, doubling with each failure up to the cap.
export const CLOCK_RETRY_MS = 5_000;
export const CLOCK_RETRY_MAX_MS = 5 * 60_000;

/// The offset of each online machine whose clock is off, by machine id.
const OFF = new Map<string, number>();

/// One read at a time; the next is due at `due`.
let reading: Promise<void> | null = null;
let due = 0;
let failures = 0;

/// Reads the machines whose clock is off once the last read has expired;
/// callers at the same time share one request. A failure keeps what was
/// known and waits longer before the next read.
export function loadClocks(invoke: typeof Invoke): Promise<void> {
  if (reading) return reading;
  if (Date.now() < due) return Promise.resolve();

  reading = (async () => {
    try {
      const list = await invoke<ClockOff[]>("machine_clocks");
      OFF.clear();
      for (const c of list) OFF.set(c.id, c.offset_ms);
      failures = 0;
      due = Date.now() + CLOCK_MAX_AGE_MS;
    } catch (e) {
      console.warn("machine clocks:", e);
      failures += 1;
      due = Date.now() + Math.min(CLOCK_RETRY_MS * 2 ** (failures - 1), CLOCK_RETRY_MAX_MS);
    } finally {
      reading = null;
    }
  })();

  return reading;
}

/// The warning for a machine, or null when its clock is fine or not known.
export function clockWarning(id: string, os: string | null) {
  const offset = OFF.get(id);
  return offset === undefined ? null : { text: clockLine(offset), title: clockTitle(os) };
}
