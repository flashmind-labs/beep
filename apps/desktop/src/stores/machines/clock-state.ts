// A machine whose clock is off (`machine_clocks`): file sync lets the newer
// file time keep a path when two machines edit it at once, so Beep warns.

/// A machine whose clock is off by `offset_ms`: positive when ahead of this
/// Mac's, negative when behind.
export interface ClockOff {
  id: string;
  offset_ms: number;
}

/// The size of an offset, such as "2 min" or "3 h".
function size(ms: number) {
  const s = Math.round(Math.abs(ms) / 1000);
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.round(s / 60)} min`;
  if (s < 86400) return `${Math.round(s / 3600)} h`;
  return `${Math.round(s / 86400)} d`;
}

/// "Clock is 2 min behind", or "ahead".
export function clockLine(offsetMs: number) {
  return `Clock is ${size(offsetMs)} ${offsetMs < 0 ? "behind" : "ahead"}`;
}

/// One sentence on why and what to do, for the OS (`linux`, `macos`,
/// `windows`) when known.
export function clockTitle(os: string | null) {
  const how = os === "linux" ? "for example timedatectl set-ntp true"
    : os === "macos" || os === "windows" ? "in Date and Time settings"
    : "for example timedatectl set-ntp true on Linux, or Date and Time settings on macOS and Windows";
  return `Sync keeps the newer file when two machines edit one at once, so turn on automatic time sync, ${how}.`;
}
