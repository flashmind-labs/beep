// Words for numbers and times, as the command center window says them.

const ESCAPES: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
/// `s` as HTML text or an attribute value.
export const esc = (s: unknown) => String(s ?? "").replace(/[&<>"']/g, c => ESCAPES[c]);

/// `s` as HTML text with its web addresses as links. Punctuation right after
/// an address belongs to the sentence.
export function linkified(s: string) {
  let html = "", last = 0;

  for (const m of s.matchAll(/https?:\/\/[^\s<>"']+/g)) {
    const url = m[0].replace(/[.,;:!?)\]]+$/, "");
    html += `${esc(s.slice(last, m.index))}<a href="${esc(url)}">${esc(url)}</a>`;
    last = m.index + url.length;
  }

  return html + esc(s.slice(last));
}

/// How each place says how long ago `ms` was:
/// - `short`: "now", "12 min", "3 h", "2 d"
/// - `since`: "just now", "12 min ago", "3 h ago", "2 d ago"
/// - `long`: "just now", "12 min ago", "3 h ago", "yesterday", "3 days ago", "last week", "2 weeks ago"
/// - `rough`: "under 2 min", "12 min", "3 h", "4 days"
/// - `chat`: "now", "12 min ago", "3 h ago" the same day, "Yesterday", then the date
/// - `day`: "09:41" today, "Yesterday", "Thu" this week, then the date
export type AgoStyle = "short" | "since" | "long" | "rough" | "chat" | "day";

/// "12 min" or "now": how long ago a time in milliseconds was, as the window says it.
export type Ago = (ms: number) => string;

export function formatAgo(ms: number, style: AgoStyle, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));

  switch (style) {
    case "short":
      if (s < 60) return "now";
      if (s < 3600) return `${Math.round(s / 60)} min`;
      if (s < 86400) return `${Math.round(s / 3600)} h`;
      return `${Math.round(s / 86400)} d`;
    case "since": {
      const short = formatAgo(ms, "short", now);
      return short === "now" ? "just now" : `${short} ago`;
    }
    case "long": {
      if (s < 60) return "just now";
      if (s < 3600) return `${Math.round(s / 60)} min ago`;
      if (s < 86400) return `${Math.round(s / 3600)} h ago`;
      const d = Math.round(s / 86400);
      if (d === 1) return "yesterday";
      if (d < 7) return `${d} days ago`;
      return d < 14 ? "last week" : `${Math.round(d / 7)} weeks ago`;
    }
    case "rough":
      if (s < 90) return "under 2 min";
      if (s < 3600 * 1.5) return `${Math.round(s / 60)} min`;
      if (s < 86400 * 1.5) return `${Math.round(s / 3600)} h`;
      return `${Math.round(s / 86400)} days`;
    case "chat": {
      const at = new Date(ms), mins = Math.round((now - ms) / 60000);
      if (mins < 1) return "now";
      if (mins < 60) return `${mins} min ago`;
      const days = dayDiff(at, new Date(now));
      if (days === 0) return `${Math.round(mins / 60)} h ago`;
      if (days === 1) return "Yesterday";
      return at.toLocaleDateString(undefined, { day: "numeric", month: "short" });
    }
    case "day": {
      const at = new Date(ms), days = dayDiff(at, new Date(now));
      if (days <= 0) return at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
      if (days === 1) return "Yesterday";
      if (days < 7) return at.toLocaleDateString([], { weekday: "short" });
      return at.toLocaleDateString([], { day: "numeric", month: "short" });
    }
  }
}

/// How many days from `from` to `to`, by the calendar.
function dayDiff(from: Date, to: Date) {
  const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  return Math.round((day(to) - day(from)) / 86400000);
}

/// "1 file", "3 files".
/// An accessible name holding every word a control shows: its label, then
/// its counts and shortcut. Zero counts are left out, as on screen.
export function spoken(...parts: (string | number | null | undefined)[]) { return parts.filter(part => part !== null && part !== undefined && part !== "" && part !== 0).join(" "); }
export function plural(n: number, one: string, many: string) { return `${n} ${n === 1 ? one : many}`; }

/// A time of day as HH:MM, in local time.
export function clock(ms: number) {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/// A refusal as the desktop said it, without what the bridge puts before it.
export function reason(e: unknown) {
  const said = e instanceof Error ? e.message : typeof e === "object" && e && "message" in e ? e.message : e;
  return String(said).replace(/^(bad request|not found|conflict|error):\s*/i, "");
}


export function base(path: string | null | undefined) { return String(path || "").split("/").filter(Boolean).pop() || ""; }

/// A size in bytes, decimal as disks are sold: "850 MB", "4.3 GB",
/// "212 GB", "1.2 TB".
export function bytes(n: number) {
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let i = 0;
  while (n >= 1000 && i < units.length - 1) { n /= 1000; i++; }
  return `${n >= 10 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
}

/// "$4.20".
export function money(usd: number) {
  return `$${(Number.isFinite(usd) ? usd : 0).toFixed(2)}`;
}

/// "2.4T", "1.1B", "1.3M", "12K", "800".
export function tokenCount(n: number) {
  if (n >= 999_950_000_000) return `${(n / 1_000_000_000_000).toFixed(1).replace(/\.0$/, "")}T`;
  if (n >= 999_950_000) return `${(n / 1_000_000_000).toFixed(1).replace(/\.0$/, "")}B`;
  if (n >= 100_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}K`;
  return String(n);
}

/// Whether version `latest` is newer than `current`, number by number.
export function isNewer(latest: string, current: string) {
  const nums = (v: string) => v.split(/[-+]/)[0].split(".").map(n => Number.parseInt(n, 10) || 0);
  const a = nums(latest), b = nums(current);

  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0, y = b[i] ?? 0;
    if (x !== y) return x > y;
  }

  return false;
}

/// `s` with its first letter in capitals.
export const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
