// What a row of the Machines list shows, as pure functions so they can be
// tested without Tauri. bar/machines.ts draws it.
import { isNewer } from "./providers-state.ts";

/// A tool a machine reported, with what its version command printed.
export interface ReportedTool {
  name: string;
  version: string | null;
}

/// The coding agents Beep can drive, in the order their columns show.
export const CODERS = ["claude", "codex", "opencode", "gemini", "pi", "flsh", "cursor-agent", "amp", "aider"];
const IS_CODER = new Set(CODERS);
export const isCoder = (name: string) => IS_CODER.has(name);

/// The other tools worth seeing first; the rest follow in reported order.
const USEFUL = ["gh", "teacli", "docker", "xcodebuild", "adb", "kubectl", "brew"];
/// How many tool chips a row shows before "+N".
export const TOOL_CHIPS = 7;

/// The bare version in what tool `name` printed: `0.162.1` for
/// `codex-cli 0.162.1`, `2.2.5` for `opencode v2.2.5` or `opencode opencode
/// v2.2.5`. Nodes from 0.1.22 send it bare already; older ones send the
/// whole line. A line with no version in it is kept as it is.
export function bareVersion(name: string, raw: string | null | undefined): string | null {
  const line = String(raw ?? "").trim();
  if (!line) return null;
  for (let word of line.split(/\s+/)) {
    word = word.replace(/^[("',;]+|[)"',;:]+$/g, "");
    if (word.startsWith(name) && /^\d/.test(word.slice(name.length))) word = word.slice(name.length);
    if (/^v\d/i.test(word)) word = word.slice(1);
    if (/^\d[\w.+-]*$/.test(word) && word.includes(".")) return word;
  }
  return line;
}

/// The coding agents among `tools`, in column order.
export function coders<T extends ReportedTool>(tools: readonly T[]): T[] {
  return tools.filter(t => isCoder(t.name)).sort((a, b) => CODERS.indexOf(a.name) - CODERS.indexOf(b.name));
}

/// The columns a list of machines needs: each coding agent any of them has.
export function coderColumns(machines: readonly (readonly ReportedTool[])[]): string[] {
  const have = new Set(machines.flatMap(tools => tools.map(t => t.name)));
  return CODERS.filter(c => have.has(c));
}

/// The other tools, the useful ones first, cut to `max` chips: `shown`, and
/// `more` the names "+N" stands for.
export function toolChips(tools: readonly ReportedTool[], max = TOOL_CHIPS) {
  const rest = tools.filter(t => !isCoder(t.name)).map(t => t.name);
  const rank = (n: string) => (USEFUL.includes(n) ? USEFUL.indexOf(n) : USEFUL.length);
  const sorted = rest.map((n, i) => ({ n, i })).sort((a, b) => rank(a.n) - rank(b.n) || a.i - b.i).map(x => x.n);
  // "+1" would take the room of the chip it hides.
  const cut = sorted.length > max ? max - 1 : sorted.length;
  return { shown: sorted.slice(0, cut), more: sorted.slice(cut) };
}

/// The compact spec on the right of a row: "macOS 26.3 · arm64 · 14 CPU · 36 GB".
export function specLine(env: { os_version: string; arch: string; cpus: number; memory_mb: number }) {
  const arch = env.arch === "aarch64" ? "arm64" : env.arch;
  const parts = [env.os_version, arch, env.cpus ? `${env.cpus} CPU` : "", env.memory_mb ? `${Math.round(env.memory_mb / 1024)} GB` : ""];
  return parts.filter(Boolean).join(" · ");
}

/// The newest beep-node among `versions`, the ones that read as versions.
export function newestNode(versions: readonly string[]): string | null {
  return versions.filter(v => /^\d/.test(v)).reduce<string | null>((best, v) => (!best || isNewer(v, best) ? v : best), null);
}

/// Whether beep-node `version` is behind `newest`.
export function nodeBehind(version: string, newest: string | null) {
  return !!newest && /^\d/.test(version) && isNewer(newest, version);
}
