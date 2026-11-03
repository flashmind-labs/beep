// Words for numbers and times, as the command center window says them.

/// "12 min", "3 h", "2 d" or "now": how long ago `ms` was.
export function ago(ms: number, now = Date.now()) {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return "now";
  if (s < 3600) return `${Math.round(s / 60)} min`;
  if (s < 86400) return `${Math.round(s / 3600)} h`;
  return `${Math.round(s / 86400)} d`;
}

/// "12 min ago", or "just now".
export function since(ms: number, now = Date.now()) {
  const a = ago(ms, now);
  return a === "now" ? "just now" : `${a} ago`;
}
