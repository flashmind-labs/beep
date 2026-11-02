// What Home's rows say and how its sections are laid out, apart from the
// DOM so it can be tested. The types are Home's view as `center_home` and
// `mac_control` give it.
import type { ApprovalField } from "./bar/chat.ts";
import type { ProjectRow } from "./projects-state.ts";

/// A tool found on a machine's `PATH` (MachineTool).
export interface MachineTool {
  name: string;
  version: string | null;
}

/// What a machine reported about itself (MachineEnv).
export interface MachineEnv {
  /// `linux` or `macos`.
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
}

/// A machine as `list_machines` gives it (MachineView).
export interface MachineView {
  id: string;
  name: string;
  online: boolean;
  /// In milliseconds since the Unix epoch.
  seen_at: number;
  version: string;
  /// Null until its first report.
  env: MachineEnv | null;
}

export interface MachineGpu {
  vendor: string;
  model: string;
  vram_mb: number | null;
  driver: string | null;
}

/// A volume a machine measured (MachineDisk).
export interface MachineDisk {
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
}

/// A coding agent's session running now.
export interface RunningAgent {
  kind: "agent";
  machine_id: string;
  machine_name: string;
  id: string;
  agent: string;
  title: string | null;
  cwd: string;
  status: string;
  doing: string | null;
  started_at: number;
  last_activity: number;
  external: boolean;
}

/// A research running now.
export interface RunningResearch {
  kind: "research";
  session: string;
  topic: string;
  status: string;
  round: number;
  sources: number;
  started_at: number;
}

/// A Running now row (RunningRow).
export type RunningRow = RunningAgent | RunningResearch;

/// An approval a chat waits on (PendingApproval).
export interface ChatApproval {
  run_id: string;
  query_id: string;
  approval: { app: string; action: string; fields: ApprovalField[] };
}

/// What a coding agent asks (AgentQuestion).
export interface AgentQuestion {
  id: number;
  text: string;
  detail: string | null;
  options: { id: string; name: string; allow: boolean }[];
}

/// A Needs you row.
export type NeedsYou =
  | { kind: "chat"; session_id: string; session_title: string; pending: ChatApproval }
  | { kind: "agent"; machine_id: string; machine_name: string; agent: string; session: string; title: string | null; question: AgentQuestion };

/// A routine coming up (CronRow).
export interface CronRow {
  id: string;
  label: string;
  prompt: string;
  schedule: string;
  /// RFC 3339, in UTC.
  next_run_at: string;
}

/// Something the event triage agent found in one of the user's apps (UpdateRow).
export interface UpdateRow {
  id: number;
  summary: string;
  app: string;
  app_name: string;
  at: number;
}

/// What `center_home` gives. A section is null when the server cannot list
/// it for this account.
export interface HomeView {
  machines: MachineCard[] | null;
  running: RunningRow[] | null;
  needs_you: NeedsYou[];
  coming_up: CronRow[] | null;
  updates: UpdateRow[] | null;
  projects: ProjectRow[] | null;
}

/// One machine's sync of one project, as `project_sync` lists it (SyncRow).
export interface SyncRow {
  project: string;
  machine_id: string;
  /// `offline`, `scanning`, `syncing`, `in_sync` or `failed`.
  status: string;
  files: number;
  pending: number;
  error: string | null;
}

/// Where this Mac's own link stands, as `mac_control` gives it.
export interface MacControl {
  id: string;
  name: string;
  enabled: boolean;
  status: "off" | "connecting" | "connected" | "failed" | "unlinked";
  error: string | null;
  permissions: unknown;
}

/// A coding agent's badge: its class, initials and name.
export interface AgentBadge {
  cls: string;
  ini: string;
  name: string;
}

const CAPS: Record<string, string> = { gpu: "GPU", cuda: "CUDA", blender: "Blender", unreal: "Unreal", xcode: "Xcode", android: "Android", docker: "Docker" };
const AGENTS: Record<string, AgentBadge> = { claude: { cls: "claude", ini: "CC", name: "Claude Code" }, codex: { cls: "codex", ini: "CX", name: "Codex" }, opencode: { cls: "opencode", ini: "OC", name: "OpenCode" } };

export function base(path: string | null | undefined) { return String(path || "").split("/").filter(Boolean).pop() || ""; }

export function agentOf(name: string | null | undefined): AgentBadge {
  return AGENTS[name ?? ""] || { cls: "", ini: String(name || "?").slice(0, 2).toUpperCase(), name: name || "Agent" };
}

/// What a machine can run: its abilities, graphics cards and apps, once
/// each. A named card stands in for the plain "GPU".
export function machineCaps(m: { gpus: readonly { model: string }[]; capabilities: readonly string[]; apps: readonly { name: string }[] }) {
  const gpus = m.gpus.map(g => g.model);
  const out = m.capabilities.filter(c => !(c === "gpu" && gpus.length)).map(c => CAPS[c] || c);
  out.push(...gpus, ...m.apps.map(a => a.name));
  return [...new Set(out)];
}

/// The machine table's Now column.
export function machineNow(m: Pick<MachineCard, "online" | "agents">) {
  if (!m.online) return "offline";
  return m.agents ? "working" : "idle";
}

/// What a machine is fetching or scanning for a synced project now.
export interface MachineSyncing {
  project: string;
  /// Null while it scans.
  pct: number | null;
  text: string;
  files?: string;
}

/// What machine `id` is fetching or scanning for a synced project now, from
/// the `project_sync` rows, or null when it is idle or in step.
export function machineSync(id: string | undefined, rows: readonly Pick<SyncRow, "project" | "machine_id" | "status" | "files" | "pending">[] | null | undefined): MachineSyncing | null {
  const r = (rows || []).find(x => x.machine_id === id && (x.status === "syncing" || x.status === "scanning"));
  if (!r) return null;
  if (r.status === "scanning") return { project: r.project, pct: null, text: "Scanning" };
  const total = r.files + r.pending;
  const pct = total ? Math.floor((100 * r.files) / total) : 0;
  return { project: r.project, pct, text: `Syncing, ${pct}%`, files: `${r.files} of ${total} files` };
}

/// A size in bytes, decimal as disks are sold: "850 MB", "4.3 GB",
/// "212 GB", "1.2 TB".
export function bytes(n: number) {
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let i = 0;
  while (n >= 1000 && i < units.length - 1) { n /= 1000; i++; }
  return `${n >= 10 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
}

/// GB from MiB, one decimal under 10: "9.1", "32".
function gb(mb: number) {
  const n = mb / 1024;
  return n >= 10 ? String(Math.round(n)) : n.toFixed(1);
}

/// The Disk column: free space where the machine keeps the user's files,
/// low under a tenth free. The title lists each volume measured.
export function machineDisk(m: { online: boolean; stats?: { disks?: readonly MachineDisk[] } | null }) {
  const disks = (m.online && m.stats?.disks) || [];
  const d = disks[0];
  if (!d || !d.total_bytes) return { text: "–", low: false, title: "" };
  return {
    text: `${bytes(d.free_bytes)} free`,
    low: d.free_bytes / d.total_bytes < 0.1,
    title: disks.map(d => `${d.path}: ${bytes(d.free_bytes)} free of ${bytes(d.total_bytes)}`).join("\n"),
  };
}

/// The Memory column: used of total, with the load in the title.
export function machineMemory(m: { online: boolean; stats?: Pick<CardStats, "memory_used_mb" | "memory_total_mb" | "load"> | null; env?: { cpus?: number } | null }) {
  const s = m.online && m.stats;
  if (!s || !s.memory_total_mb) return { text: "–", title: "" };
  const cores = m.env?.cpus ? ` on ${m.env.cpus} cores` : "";
  return {
    text: `${gb(s.memory_used_mb)} / ${gb(s.memory_total_mb)} GB`,
    title: s.load == null ? "" : `Load ${s.load.toFixed(2)}${cores}`,
  };
}

/// The OS line under a machine's name.
export function machineOs(m: { env: Pick<MachineEnv, "os" | "os_version"> | null }) {
  return m.env ? m.env.os_version || m.env.os : "";
}

/// A Running now row: a title and one muted line.
export function runningLine(
  r: Pick<RunningResearch, "kind" | "topic" | "round"> | Pick<RunningAgent, "kind" | "agent" | "title" | "cwd" | "machine_id" | "machine_name">,
  projects: readonly ProjectFolder[] = [],
) {
  if (r.kind === "research") return { title: r.topic, sub: `Research · round ${r.round}` };
  const ag = agentOf(r.agent), project = repoOf(r, projects);
  return { title: r.title || project || ag.name, sub: [ag.name, project, r.machine_name].filter(Boolean).join(" · ") };
}

/// An imported project's folder on one machine.
type ProjectFolder = Pick<ProjectRow, "name" | "machine_id" | "folder">;

/// The repo a session runs in: the name of the imported project whose
/// folder on its machine holds it ("repo" of "org/repo"), else its folder's.
export function repoOf(r: Pick<RunningAgent, "machine_id" | "cwd">, projects: readonly ProjectFolder[] = []) {
  const inside = (p: ProjectFolder) => p.machine_id === r.machine_id && (r.cwd === p.folder || r.cwd?.startsWith(p.folder + "/"));
  const p = projects.filter(inside).sort((a, b) => b.folder.length - a.folder.length)[0];
  return p ? p.name.split("/").pop() : base(r.cwd);
}

/// A Needs you row: a title, one muted line and its one button. A chat
/// approval is allowed from the row; an agent's question opens its session.
export function needsLine(n:
  | { kind: "chat"; session_title: string; pending: { approval: Pick<ChatApproval["approval"], "app" | "action"> } }
  | { kind: "agent"; agent: string; title: string | null; machine_name: string; question: Pick<AgentQuestion, "text"> }) {
  if (n.kind === "chat") {
    const a = n.pending.approval;
    const action = a.action ? a.action[0].toUpperCase() + a.action.slice(1) : "Allow this";
    return { title: `${action}?`, sub: [a.app, n.session_title].filter(Boolean).join(" · "), button: "Allow" };
  }
  return { title: n.question.text, sub: [agentOf(n.agent).name, n.title || n.machine_name].filter(Boolean).join(" · "), button: "Answer" };
}

/// A section of Home, by the key its layout uses.
export type HomeSection = "running" | "needs" | "updates" | "coming" | "projects" | "git" | "machines";

/// The lists Home's layout looks at, only for whether they have rows.
/// `git` is what `git_items` gave, once the window has it.
export type HomeLists = Partial<Record<"machines" | "running" | "needs_you" | "updates" | "coming_up" | "projects", readonly unknown[] | null>>
  & { git?: { items?: unknown; hosts?: readonly unknown[] | null } | null };

const has = (list: readonly unknown[] | null | undefined) => Array.isArray(list) && list.length > 0;
const keep = (list: (HomeSection | false)[]) => list.filter((s): s is HomeSection => !!s);

/// Home's rows of sections under Machines, each one or two wide, imported
/// projects, then open PRs and issues by project (`home.git`, shown once a
/// git host is saved), last. A section
/// with nothing in it is left out, and a row left empty goes too.
export function homeRows(home: HomeLists) {
  const pairs = [
    [has(home.running) && "running", has(home.needs_you) && "needs"],
    [has(home.updates) && "updates", has(home.coming_up) && "coming"],
    [has(home.projects) && "projects"],
    [has(home.git?.hosts) && "git"],
  ] satisfies (HomeSection | false)[][];
  return pairs.map(keep).filter(p => p.length);
}

/// How wide Home's view must be, in CSS pixels, to split into two columns.
export const WIDE_HOME_PX = 1280;

/// Home's two columns on a wide window: what to act on at the left (Needs
/// you, Running now, Updates, open PRs and issues), Machines, Projects and
/// Coming up at the right. Machines
/// counts whenever the server lists them, empty or not, as it offers Add a
/// machine. Null when a side would be empty, so Home keeps one column.
export function homeColumns(home: HomeLists) {
  const left = keep([has(home.needs_you) && "needs", has(home.running) && "running", has(home.updates) && "updates", has(home.git?.hosts) && "git"]);
  const right = keep([Array.isArray(home.machines) && "machines", has(home.projects) && "projects", has(home.coming_up) && "coming"]);
  return left.length && right.length ? { left, right } : null;
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
/// and, while it is off, the button that turns it on. `mac` is the app's
/// mac_control view. Null once the server lists this Mac, or when there is
/// nothing to say (no list, or the app is not linked to an account).
export function thisMacRow(mac: Pick<MacControl, "id" | "name" | "enabled" | "status" | "error"> | null | undefined, machines: readonly { id: string }[] | null | undefined): MacRow | null {
  if (!mac || !Array.isArray(machines) || mac.status === "unlinked") return null;
  if (machines.some(m => m.id === mac.id)) return null;
  const row = { name: "This Mac", sub: mac.name || "", action: null };
  if (!mac.enabled) return { ...row, state: "Off", title: "Let Beep use this Mac is off. Add turns it on.", action: "Add" };
  if (mac.status === "failed") {
    const error = mac.error || "Could not connect";
    return { ...row, state: error, title: `${error}. Beep tries again on its own.`, bad: true };
  }
  // Connected too, until the next list has it.
  return { ...row, state: "Connecting", title: "Joining your machines" };
}
