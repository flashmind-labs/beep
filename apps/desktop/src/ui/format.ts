// Words for numbers and times, as the command center window says them.

const ESCAPES: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
/// `s` as HTML text or an attribute value.
export const esc = (s: unknown) => String(s ?? "").replace(/[&<>"']/g, c => ESCAPES[c]);

/// A `dswitch` button as markup. `label` is already safe; `attrs` adds ids,
/// data attributes or `disabled`.
export const switchHtml = (on: unknown, label: string, attrs = "") =>
  `<button class="dswitch" role="switch" aria-checked="${on}" aria-label="${label}"${attrs && " " + attrs}></button>`;

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

/// "1 file", "3 files".
export function plural(n: number, one: string, many: string) { return `${n} ${n === 1 ? one : many}`; }

/// A refusal as the server said it, without what the bridge puts before it.
export function reason(e: unknown) {
  return String(e).replace(/^(bad request|error):\s*/i, "");
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
