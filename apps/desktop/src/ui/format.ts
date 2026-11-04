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
