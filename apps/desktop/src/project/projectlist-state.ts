// The Projects page's list: one row per project, grouped by whether agents
// work in it now, filtered by name. Pure, so the tests read it.
import type { Project } from "./projects-state.ts";
import type { SyncRow, SyncDetail } from "./filesync.ts";
import { syncedBySyncthing } from "./filesync.ts";
import type { GitRow } from "./gitsync-state.ts";
import type { RunRow } from "./runs.ts";
import { going } from "./runs.ts";

/// A machine chip's dot: in step, syncing, needs a look (amber) or offline.
type ChipState = "ok" | "syncing" | "conflict" | "off";

interface ListChip {
  name: string;
  state: ChipState;
  /// "build-box: /srv/app · In step".
  title: string;
}

/// The CI dot: the newest run passed, failed, is running, or there is none.
type CiState = "ok" | "bad" | "busy" | "none";

/// What the list knows of the user's machines and each project's sync, all
/// of it read already: Home's machines, `project_sync`, and the sync details
/// a project's page last read.
interface ChipInputs {
  online: ReadonlyMap<string, boolean>;
  sync: readonly SyncRow[];
  detail: SyncDetail | null | undefined;
}

/// "acme/" and "widget" for `acme/widget`; no owner for a plain folder name.
export function splitName(name: string) {
  const cut = name.lastIndexOf("/");
  return cut > 0 ? { owner: name.slice(0, cut + 1), repo: name.slice(cut + 1) } : { owner: "", repo: name };
}

/// One machine's dot and its word. A machine Home does not list counts as
/// online; a folder with no Beep sync row (one machine, or Syncthing's) is
/// in step.
export function chipState(online: boolean | undefined, sync: Pick<SyncRow, "status" | "error"> | undefined, conflicts: number): { state: ChipState; label: string } {
  if (online === false || sync?.status === "offline") return { state: "off", label: "Offline" };
  if (conflicts > 0) return { state: "conflict", label: "Conflict" };
  if (!sync || syncedBySyncthing(sync)) return { state: "ok", label: "In step" };
  if (sync.status === "scanning" || sync.status === "syncing") return { state: "syncing", label: "Syncing" };
  if (sync.status === "in_sync") return { state: "ok", label: "In step" };
  return { state: "conflict", label: "Not syncing" };
}

/// A chip per machine the project is on.
export function machineChips(p: Pick<Project, "name" | "machines">, inputs: ChipInputs): ListChip[] {
  return p.machines.map(m => {
    const sync = inputs.sync.find(r => r.project === p.name && r.machine_id === m.id);
    const conflicts = inputs.detail?.machines.find(d => d.machine_id === m.id)?.conflicts.length ?? 0;
    const { state, label } = chipState(inputs.online.get(m.id), sync, conflicts);
    return { name: m.name, state, title: `${m.name}: ${m.folder} · ${label}` };
  });
}

/// The branch checked out: this Mac's checkout's, else the first machine's
/// with one; null when not read yet or detached everywhere.
export function projectBranch(rows: readonly Pick<GitRow, "machine_id" | "branch">[] | null | undefined, mac: string | null) {
  if (!rows) return null;
  return rows.find(r => r.machine_id === mac && r.branch)?.branch ?? rows.find(r => r.branch)?.branch ?? null;
}

/// The newest run on `branch` (or the newest run at all when that branch has
/// none) as a dot. Runs come newest first.
export function ciState(runs: readonly Pick<RunRow, "state" | "branch">[] | null | undefined, branch: string | null): CiState {
  const r = (branch && runs?.find(x => x.branch === branch)) || runs?.[0];
  if (!r) return "none";
  if (going(r.state)) return "busy";
  if (r.state === "passed") return "ok";
  if (r.state === "failed") return "bad";
  return "none";
}

/// The rows whose name holds `query`, ignoring case and spaces around it.
export function filterByName<T extends { name: string }>(rows: readonly T[], query: string): T[] {
  const q = query.trim().toLowerCase();
  return q ? rows.filter(r => r.name.toLowerCase().includes(q)) : [...rows];
}

/// "Working now" (agents running in it) above "Others", each keeping the
/// order it came in; an empty group is left out.
export function listGroups<T extends { agents: number }>(rows: readonly T[]) {
  const working = rows.filter(r => r.agents > 0), others = rows.filter(r => !r.agents);
  return [{ label: "Working now", rows: working }, { label: "Others", rows: others }].filter(g => g.rows.length);
}
