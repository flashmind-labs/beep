import { isNewer } from "../ui/format.ts";
// What a row of the Machines list shows, as pure functions so they can be
// tested without Tauri. machines.ts draws it.

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

/// The bare version in what tool `name` printed: `0.162.1` for `codex-cli
/// 0.162.1`, `2.2.5` for `opencode v2.2.5` or `opencode opencode v2.2.5`.
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

/// The beep-node machines are offered: the newest release when it could be
/// read, or the newest among the machines that update, whichever is newer.
export function offeredNode(released: string | null, fleet: readonly string[]): string | null {
  return newestNode(released ? [released, ...fleet] : fleet);
}

/// Whether beep-node `version` is behind `newest`.
export function nodeBehind(version: string, newest: string | null) {
  return !!newest && /^\d/.test(version) && isNewer(newest, version);
}

/// Where Update on a machine's page stands: installing, waiting for the
/// node to come back on `to` (`note` when it waits for its sessions
/// first), or stopped with what went wrong. `note` is why an installed
/// node did not restart.
export type NodeUpdate =
  | { step: "updating" }
  | { step: "restarting"; to: string; note: string }
  | { step: "failed"; error: string }
  | { step: "note"; text: string };

/// An error's text without the API's "bad request: " in front.
export const errorText = (e: unknown) => String(e).replace(/^bad request: /, "");

/// Whether a node restarting on `to` is back: online on `to` or newer.
export function updatedTo(m: { online: boolean; version: string }, to: string) {
  return m.online && /^\d/.test(m.version) && !isNewer(to, m.version);
}

/// The version a tool's Update installed this session, by "<machine> <tool>".
export type Installed = ReadonlyMap<string, string>;

/// The version to show for `name`: what was installed this session while
/// the machine still reports an older one.
export function shownVersion(installed: Installed, key: string, name: string, raw: string | null | undefined) {
  const bare = bareVersion(name, raw), to = installed.get(key);
  return to && (!bare || isNewer(to, bare)) ? to : bare;
}

/// Whether Update to `latest` is still worth offering: not when this
/// session installed `latest` or something newer.
export function updateOffered(installed: Installed, key: string, latest: string) {
  const to = installed.get(key);
  return !to || isNewer(latest, to);
}

/// "Updated flsh to 0.2.6" for each tool of a machine installed this
/// session that the machine does not report yet.
export function updatedNotes(installed: Installed, machine: string, tools: readonly ReportedTool[]): string[] {
  return tools.flatMap(t => {
    const to = installed.get(`${machine} ${t.name}`);
    return to && shownVersion(installed, `${machine} ${t.name}`, t.name, t.version) === to && bareVersion(t.name, t.version) !== to ? [`Updated ${t.name} to ${to}`] : [];
  });
}

/// The tag after a machine's name: "container" for one that leaves the mesh
/// when it stops, else none.
export function machineTag(m: { temporary?: boolean }): string | null {
  return m.temporary ? "container" : null;
}
