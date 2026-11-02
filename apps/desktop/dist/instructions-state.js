// The Instructions view's words and sums, as pure functions so they can be
// tested without Tauri. instructions.js draws the view.

/// The longest instructions the server takes, in bytes.
export const MAX_BYTES = 16 * 1024;

const START = "<!-- beep:start -->";
const END = "<!-- beep:end -->";

/// What each coding agent is called on screen.
const AGENTS = { claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI", pi: "pi" };
export function agentName(agent) { return AGENTS[agent] || String(agent || ""); }

/// Bytes of `text` as UTF-8.
export function bytes(text) { return new TextEncoder().encode(String(text || "")).length; }

/// "Markdown · 0.6 of 16 KB".
export function sizeLabel(text) {
  const kb = bytes(text) / 1024;
  return `Markdown · ${kb < 10 ? kb.toFixed(1) : Math.round(kb)} of ${MAX_BYTES / 1024} KB`;
}

/// "241 lines", "1 line", or "empty".
export function linesLabel(text) {
  const t = String(text || "").replace(/\s+$/, "");
  if (!t) return "empty";
  const n = t.split("\n").length;
  return n === 1 ? "1 line" : `${n} lines`;
}

/// An agent file without the block Beep wrote into it, so importing it
/// never brings Beep's own text back in.
export function withoutBeepBlock(text) {
  const t = String(text || "");
  const start = t.indexOf(START);
  if (start < 0) return t.trim();
  const end = t.indexOf(END, start);
  if (end < 0) return t.trim();
  return (t.slice(0, start) + t.slice(end + END.length)).replace(/\n{3,}/g, "\n\n").trim();
}

/// `mine` with `extra` added after a blank line, or `extra` alone.
export function appended(mine, extra) {
  const a = String(mine || "").replace(/\s+$/, ""), b = String(extra || "").trim();
  if (!b) return a;
  return a ? `${a}\n\n${b}\n` : `${b}\n`;
}

/// How the files on the machines stand: `{ machines, synced }`, a machine
/// counting as synced when every agent file it has holds the newest text.
/// Machines that could not be read or have no agent are left out.
export function syncSummary(read) {
  const withFiles = (read || []).filter(m => !m.error && m.files.length);
  return { machines: withFiles.length, synced: withFiles.filter(m => m.files.every(f => f.synced)).length };
}

/// What a file's row says on the Sync tab.
export function fileState(file) {
  if (file.synced) return { cls: "ok", text: "Synced" };
  if (file.text == null) return { cls: "", text: "Not written yet" };
  return { cls: "warn", text: "Not synced" };
}

/// "Version 7 · Restored version 3", the line over the editor and on a
/// History row.
export function versionLine(v) {
  if (!v) return "Not saved yet";
  return [`Version ${v.version}`, v.note].filter(Boolean).join(" · ");
}
