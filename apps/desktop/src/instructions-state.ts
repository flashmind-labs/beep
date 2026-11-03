// The Instructions view's words and sums, as pure functions so they can be
// tested without Tauri. instructions.ts draws the view.
import { plural } from "./components/format.ts";

/// One saved version of the user's instructions (VersionRow).
export interface VersionRow {
  version: number;
  text: string;
  note: string;
  /// In milliseconds since the Unix epoch.
  saved_at: number;
}

/// One coding agent's instructions file on a machine (AgentFile).
export interface AgentFile {
  /// `claude`, `codex`, `gemini` or `pi`.
  agent: string;
  /// Under the home folder, such as `~/.claude/CLAUDE.md`.
  path: string;
  /// Null when the agent's folder is there but the file is not.
  text: string | null;
  /// It holds the newest instructions between Beep's markers.
  synced: boolean;
}

/// The agent files on one machine (MachineAgentFiles).
export interface MachineAgentFiles {
  machine_id: string;
  machine_name: string;
  files: AgentFile[];
  /// Why the machine could not be read or written; `files` is then empty.
  error: string | null;
}

/// How the files on the machines stand.
export interface SyncSummary {
  machines: number;
  synced: number;
}

/// The longest instructions the server takes, in bytes.
export const MAX_BYTES = 16 * 1024;

const START = "<!-- beep:start -->";
const END = "<!-- beep:end -->";

/// What each coding agent is called on screen.
const AGENTS: Record<string, string> = { claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI", pi: "pi" };
export function agentName(agent: string | null | undefined) { return AGENTS[agent ?? ""] || String(agent || ""); }

/// Bytes of `text` as UTF-8.
export function bytes(text: string | null | undefined) { return new TextEncoder().encode(String(text || "")).length; }

/// "Markdown · 0.6 of 16 KB".
export function sizeLabel(text: string) {
  const kb = bytes(text) / 1024;
  return `Markdown · ${kb < 10 ? kb.toFixed(1) : Math.round(kb)} of ${MAX_BYTES / 1024} KB`;
}

/// "241 lines", "1 line", or "empty".
export function linesLabel(text: string | null | undefined) {
  const t = String(text || "").replace(/\s+$/, "");
  if (!t) return "empty";
  const n = t.split("\n").length;
  return plural(n, "line", "lines");
}

/// An agent file without the block Beep wrote into it, so importing it
/// never brings Beep's own text back in.
export function withoutBeepBlock(text: string | null | undefined) {
  const t = String(text || "");
  const start = t.indexOf(START);
  if (start < 0) return t.trim();
  const end = t.indexOf(END, start);
  if (end < 0) return t.trim();
  return (t.slice(0, start) + t.slice(end + END.length)).replace(/\n{3,}/g, "\n\n").trim();
}

/// `mine` with `extra` added after a blank line, or `extra` alone.
export function appended(mine: string | null | undefined, extra: string | null | undefined) {
  const a = String(mine || "").replace(/\s+$/, ""), b = String(extra || "").trim();
  if (!b) return a;
  return a ? `${a}\n\n${b}\n` : `${b}\n`;
}

/// How the files on the machines stand, a machine counting as synced when
/// every agent file it has holds the newest text. Machines that could not
/// be read or have no agent are left out.
export function syncSummary(read: readonly { error: string | null; files: readonly Pick<AgentFile, "synced">[] }[] | null | undefined): SyncSummary {
  const withFiles = (read || []).filter(m => !m.error && m.files.length);
  return { machines: withFiles.length, synced: withFiles.filter(m => m.files.every(f => f.synced)).length };
}

/// What a file's row says on the Sync tab. `saved` is whether any version
/// was saved: before the first there is nothing to write, so a file is
/// never "Not synced", only empty or holding its own text.
export function fileState(file: Pick<AgentFile, "synced"> & Partial<Pick<AgentFile, "text">>, saved = true) {
  if (file.text == null) return { cls: "", text: "Not written yet" };
  if (!saved) return { cls: "", text: "Has its own text" };
  if (file.synced) return { cls: "ok", text: "Synced" };
  return { cls: "warn", text: "Not synced" };
}

/// The machines whose files miss the newest version: the ones "Write to
/// all machines" writes. Machines that could not be read are left out.
export function behind(read: readonly MachineAgentFiles[] | null | undefined) {
  return (read || []).filter(m => !m.error && m.files.some(f => !f.synced)).map(m => m.machine_id);
}

/// "Version 7 · Restored version 3", the line over the editor and on a
/// History row.
export function versionLine(v: Pick<VersionRow, "version" | "note"> | null | undefined) {
  if (!v) return "Not saved yet";
  return [`Version ${v.version}`, v.note].filter(Boolean).join(" · ");
}

/// The "Your instructions" line under How agents see it: which version
/// agents get and on how many machines, once the files were read.
export function mineLayer(newest: Pick<VersionRow, "version"> | null | undefined, summary: SyncSummary | null | undefined) {
  if (!newest) return "Nothing saved yet, so agents get only the other layers";
  const ver = `Version ${newest.version}, between Beep's markers in each agent's file`;
  if (!summary) return `${ver}. Sync shows which machines have it`;
  if (!summary.machines) return `${ver}. No machine with a coding agent is online`;
  if (summary.synced === summary.machines) return `${ver}. On ${summary.machines === 1 ? "your 1 machine" : `all ${summary.machines} machines`}`;
  return `${ver}. On ${summary.synced} of ${summary.machines} machines; Sync writes the rest`;
}
