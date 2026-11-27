import { onPhone } from "../../ui/platform.ts";
// What a machine's card says: its abilities, disk, memory and synced
// projects, apart from the DOM so it can be tested.
import { bytes, cap, formatAgo, plural } from "../../ui/format.ts";
import { conflictCount, machineMeta, rowDetail, stateChip, syncPct, type SyncDetail, type SyncRow } from "../project/filesync.ts";

/// A tool found on a machine's `PATH` (MachineTool).
interface MachineTool {
  name: string;
  version: string | null;
}

/// A WSL distribution of a Windows machine (WslDistro).
export interface WslDistro {
  name: string;
  /// The coding agent CLIs found inside it, such as `claude`.
  agents: string[];
  /// The beep-node version installed inside it.
  node: string | null;
  /// That node's id in the mesh, once it made its key.
  machine_id: string | null;
  /// The Windows machine keeps that node running.
  kept: boolean;
  /// Why the kept node is not running.
  problem: string | null;
  /// `agents` and `node` were read; a stopped distribution is looked into
  /// only when asked.
  probed: boolean;
}

/// What a machine reported about itself (MachineEnv).
export interface MachineEnv {
  /// `linux`, `macos` or `windows`.
  os: string;
  os_version: string;
  arch: string;
  hostname: string;
  cpus: number;
  memory_mb: number;
  /// The folder commands start in.
  home: string;
  tools: MachineTool[];
  logins: { tool: string; signed_in: boolean; account: string | null }[];
  avds: string[];
  gpus: MachineGpu[];
  /// The WSL distributions of a Windows machine.
  wsl: WslDistro[];
}

/// A machine as `list_machines` gives it (MachineView).
export interface MachineView {
  id: string;
  name: string;
  online: boolean;
  /// False for phones, which control the mesh without running work.
  can_host?: boolean;
  /// In milliseconds since the Unix epoch.
  seen_at: number;
  version: string;
  /// Null until its first report.
  env: MachineEnv | null;
  /// Removed after a day offline, such as a container.
  temporary?: boolean;
}

interface MachineGpu {
  vendor: string;
  model: string;
  vram_mb: number | null;
  driver: string | null;
}

/// A volume a machine measured (MachineDisk).
interface MachineDisk {
  path: string;
  free_bytes: number;
  total_bytes: number;
}

/// What a machine last said it has free (CardStats).
export interface CardStats {
  sampled_at: number;
  /// The workspace's volume first.
  disks: MachineDisk[];
  memory_used_mb: number;
  memory_total_mb: number;
  load: number | null;
}

/// A machine on Home, with what it reported and what runs on it (MachineCard).
export interface MachineCard extends MachineView {
  apps: { name: string; version: string | null }[];
  gpus: MachineGpu[];
  capabilities: string[];
  /// Agent sessions on it that have not ended.
  agents: number;
  /// Null while offline or before its first sample.
  stats: CardStats | null;
  /// Not reached yet: shown from what the app kept, or the mesh is still
  /// dialing it after opening or losing its link.
  connecting: boolean;
}

/// How a machine's dot reads: on (busy with agents or idle), connecting, or off.
export function machineDot(m: Pick<MachineCard, "online" | "agents" | "connecting">): { className: string; title: string } {
  if (m.online) return m.agents ? { className: "ddot busy", title: `Online · ${plural(m.agents, "agent", "agents")} running` } : { className: "ddot", title: "Online" };
  return m.connecting ? { className: "ddot dial", title: "Connecting" } : { className: "ddot off", title: "Offline" };
}

/// Where the machines that run agents stand, for a list that stays empty until one answers:
/// one is online (or there are none to wait for), the app is still connecting, or all are off.
export type MachinesReach = "reached" | "connecting" | "offline";

/// How `machines` stand together; no list read yet counts as connecting.
export function machinesReach(machines: readonly Pick<MachineCard, "online" | "connecting" | "can_host">[] | null): MachinesReach {
  if (!machines) return "connecting";
  const hosts = machines.filter(m => m.can_host !== false);
  if (!hosts.length || hosts.some(m => m.online)) return "reached";
  return hosts.some(m => m.connecting) ? "connecting" : "offline";
}

/// The machines the app is still connecting to, by id, for the session rows on them.
let connecting: ReadonlySet<string> = new Set();
let reach: MachinesReach = "connecting";
const connectingWatchers = new Set<() => void>();

/// Takes which of `machines` are connecting; the watchers hear of a change.
export function keepConnecting(machines: readonly Pick<MachineCard, "id" | "online" | "connecting" | "can_host">[] | null) {
  const next = new Set((machines ?? []).filter(m => m.connecting).map(m => m.id)), nextReach = machinesReach(machines);
  if (nextReach === reach && next.size === connecting.size && [...next].every(id => connecting.has(id))) return;
  connecting = next;
  reach = nextReach;
  for (const watch of connectingWatchers) watch();
}

export const machineConnecting = (id: string) => connecting.has(id);
export const machinesReachNow = () => reach;
export function onConnecting(watch: () => void) { connectingWatchers.add(watch); }

/// Settings, This Mac (`mac_control`): whether Beep may use it, where its
/// link stands and the permissions desktop control needs.
export interface MacControl {
  id: string;
  name: string;
  enabled: boolean;
  status: "off" | "connecting" | "connected" | "failed" | "unlinked";
  /// Why the last try failed, while `status` is "failed".
  error: string | null;
  permissions: { screen_recording: boolean; accessibility: boolean };
}

const CAPS: Record<string, string> = { gpu: "GPU", cuda: "CUDA", blender: "Blender", unreal: "Unreal", xcode: "Xcode", android: "Android", docker: "Docker" };

/// What a machine can run: its abilities, graphics cards and apps, once
/// each. A named card stands in for the plain "GPU".
export function machineCaps(m: { gpus: readonly { model: string }[]; capabilities: readonly string[]; apps: readonly { name: string }[] }) {
  const out = m.capabilities.map(c => CAPS[c] || c);
  if (m.gpus.length) out.push("GPU");
  out.push(...m.apps.map(a => a.name));
  return [...new Set(out)];
}

/// What a machine does now, for the quiet line on its card: "2 agents",
/// "idle", "controller", "connecting" or "offline, last seen 2 h ago".
export function machineNow(m: Pick<MachineCard, "online" | "agents" | "can_host" | "connecting" | "seen_at">) {
  if (!m.online) return m.connecting ? "connecting" : `offline, last seen ${formatAgo(m.seen_at, "since")}`;
  if (m.can_host === false) return "controller";
  return m.agents ? plural(m.agents, "agent", "agents") : "idle";
}

/// How far machine `id` is with a synced project it fetches or scans now,
/// from the `project_sync` rows: `pct` is null while it scans. Null when it
/// is idle or in step.
export function machineSync(id: string | undefined, rows: readonly Pick<SyncRow, "machine_id" | "status" | "files" | "pending">[] | null | undefined): { pct: number | null } | null {
  const r = (rows || []).find(x => x.machine_id === id && (x.status === "syncing" || x.status === "scanning"));
  return r ? { pct: syncPct(rowDetail(r)) } : null;
}

/// One project a machine syncs, as its row under the machine shows it.
export interface MachineProject {
  project: string;
  off: boolean;
  /// How far it is while it moves files; null shows `chip`.
  pct: number | null;
  chip: { text: string; wait: boolean };
  meta: string[];
  title: string;
}

/// What a machine's folded projects say together: how many sit in each
/// state, the most common first ("3 in sync · 2 paused"), or "all in sync".
export function projectsSummary(projects: readonly Pick<MachineProject, "chip">[]) {
  const counts = new Map<string, number>();
  for (const p of projects) {
    const text = p.chip.text.toLowerCase().replace(/ \d+%$/, "");
    counts.set(text, (counts.get(text) ?? 0) + 1);
  }
  const groups = [...counts].sort((a, b) => b[1] - a[1]);
  if (groups.length === 1) return `all ${groups[0][0]}`;
  return groups.map(([text, n]) => `${n} ${text}`).join(" · ");
}

/// The projects machine `id` syncs, what needs a look first. `detailOf` gives
/// a project's read detail, which has the rate, time left, pause and
/// conflicts; a detail that no longer agrees with the newer row is left out.
export function machineProjects(id: string, rows: readonly SyncRow[], detailOf: (project: string) => SyncDetail | null | undefined): MachineProject[] {
  const out = rows.filter(r => r.machine_id === id).map(r => {
    const read = detailOf(r.project)?.machines.find(m => m.machine_id === id);
    const m = read?.status === r.status ? read : rowDetail(r);
    const chip = stateChip(m), n = m.conflicts.length;
    return {
      project: r.project,
      off: m.paused || m.status === "offline",
      pct: m.paused ? null : syncPct(m),
      chip: { text: cap(chip.text), wait: chip.wait },
      meta: [...machineMeta(m), ...(n ? [conflictCount(n)] : [])],
      title: m.error || m.folder || r.project,
    };
  });
  const look = (p: MachineProject) => p.pct != null || p.chip.wait ? 0 : 1;
  return out.sort((a, b) => look(a) - look(b));
}

/// GB from MiB, one decimal under 10: "9.1", "32".
function gb(mb: number) {
  const n = mb / 1024;
  return n >= 10 ? String(Math.round(n)) : n.toFixed(1);
}

/// A volume's short name: its drive on Windows, otherwise its folder.
function volumeName(path: string) {
  if (/^[a-z]:\\?$/i.test(path)) return path.slice(0, 2);
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

/// The Disk stat: free space where the machine keeps the user's files, the
/// fraction used (null without a reading), low under a tenth free, and the
/// other volume with the most free. The title lists each volume measured.
export function machineDisk(m: { online: boolean; stats?: { disks?: readonly MachineDisk[] } | null }) {
  const disks = (m.online && m.stats?.disks) || [];
  const d = disks[0];
  if (!d || !d.total_bytes) return { text: "–", used: null, low: false, title: "", other: "" };
  const roomiest = disks.slice(1).reduce<MachineDisk | null>((best, v) => !best || v.free_bytes > best.free_bytes ? v : best, null);
  return {
    text: `${bytes(d.free_bytes)} free`,
    used: 1 - d.free_bytes / d.total_bytes,
    low: d.free_bytes / d.total_bytes < 0.1,
    other: roomiest ? `${volumeName(roomiest.path)} ${bytes(roomiest.free_bytes)} free` : "",
    title: disks.map(d => `${d.path}: ${bytes(d.free_bytes)} free of ${bytes(d.total_bytes)}`).join("\n"),
  };
}

/// The Memory stat: used of total, the fraction used, and the load in the title.
export function machineMemory(m: { online: boolean; stats?: Pick<CardStats, "memory_used_mb" | "memory_total_mb" | "load"> | null; env?: { cpus?: number } | null }) {
  const s = m.online && m.stats;
  if (!s || !s.memory_total_mb) return { text: "–", used: null, title: "" };
  const cores = m.env?.cpus ? ` on ${m.env.cpus} cores` : "";
  return {
    text: `${gb(s.memory_used_mb)} / ${gb(s.memory_total_mb)} GB`,
    used: s.memory_used_mb / s.memory_total_mb,
    title: s.load == null ? "" : `Load ${s.load.toFixed(2)}${cores}`,
  };
}

/// The OS line under a machine's name.
export function machineOs(m: { env: Pick<MachineEnv, "os" | "os_version"> | null }) {
  return m.env ? m.env.os_version || m.env.os : "";
}

/// Home's "This Mac" row: its state, and the button that turns it on.
export interface MacRow {
  name: string;
  sub: string;
  state: string;
  title: string;
  action: "Add" | null;
  bad?: boolean;
}

/// Home's "This Mac" row while this Mac is not one of `machines`: its state
/// and, while it is off, the button that turns it on.
export function thisMacRow(mac: Pick<MacControl, "id" | "name" | "enabled" | "status" | "error"> | null | undefined, machines: readonly { id: string }[] | null | undefined): MacRow | null {
  if (!mac || !Array.isArray(machines) || mac.status === "unlinked" || onPhone()) return null;
  if (machines.some(m => m.id === mac.id)) return null;
  const row = { name: "This computer", sub: mac.name || "", action: null };
  if (!mac.enabled) return { ...row, state: "Off", title: "Let Beep use this computer is off. Add turns it on.", action: "Add" };

  if (mac.status === "failed") {
    const error = mac.error || "Could not connect";
    return { ...row, state: error, title: `${error}. Beep tries again on its own.`, bad: true };
  }

  // Connected too, until the next list has it.
  return { ...row, state: "Connecting", title: "Joining your machines" };
}

