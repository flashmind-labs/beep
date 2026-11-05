// What the project page's list says of each kind of thing in a project: its
// count and the few rows that matter now, as pure functions the tests check.
import type { TaskRow, WorkerRow } from "./agent.ts";
import type { SessionRow, WhereRow } from "./projects-state.ts";
import { machineSync } from "../machines/machine-state.ts";
import type { RunningAgent } from "../agents/running-state.ts";
import type { SyncRow } from "./filesync.ts";

/// One row of a tile: an optional dot, its text with an optional code
/// before it, and a chip or a muted note. A row with `url` opens it.
export interface TileRow {
  text?: string;
  code?: string;
  url?: string;
  chip?: { cls: string; text: string } | null;
  note?: string;
  dot?: string;
  title?: string;
  /// What the row is about, such as a machine's id, for `TileOptions.menu`.
  id?: string;
}

/// A tile's count, its rows, and what it says without any.
export interface TileData {
  count: string;
  rows: TileRow[];
  empty: string;
}

/// Open task states, in the order the tile lists them.
const OPEN: Record<string, number> = { running: 0, blocked: 1, review: 2, planned: 3 };

/// A coding-agent session's key in `askingSessions`.
export function sessionKey(machineId: string, session: string) { return `${machineId}\n${session}`; }

/// The sessions that wait on the user: running agents that ask, and the
/// agent questions Home lists.
export function askingSessions(
  running: readonly Partial<Pick<RunningAgent, "machine_id" | "id" | "status">>[] | null | undefined,
  needs?: readonly { kind: string; machine_id?: string; session?: string }[] | null,
) {
  const out = new Set<string>();
  for (const r of running || []) if (r.status === "asking" && r.machine_id && r.id) out.add(sessionKey(r.machine_id, r.id));
  for (const n of needs || []) if (n.kind === "agent" && n.machine_id && n.session) out.add(sessionKey(n.machine_id, n.session));
  return out;
}

type TileTask = Pick<TaskRow, "title" | "state"> & Partial<Pick<TaskRow, "id" | "worker">>;
type TileWorker = Pick<WorkerRow, "id" | "machine_id" | "session">;

/// The Tasks tile: open tasks, running ones first.
export function taskTile(tasks: readonly TileTask[] | null | undefined, workers?: readonly TileWorker[] | null, asking?: ReadonlySet<string> | null): TileData {
  const list = tasks || [];
  const open = list.filter(t => t.state in OPEN).sort((a, b) => OPEN[a.state] - OPEN[b.state]);
  const running = list.filter(t => t.state === "running").length;
  const count = running ? `${running} running` : open.length ? `${open.length} open` : list.length ? "all done" : "";
  const asks = (t: TileTask) => {
    const w = t.worker && (workers || []).find(x => x.id === t.worker);
    return !!(w && w.session && asking?.has(sessionKey(w.machine_id, w.session)));
  };
  const rows = open.map(t => ({
    id: t.id,
    text: t.title,
    chip: asks(t) ? { cls: "wait", text: "asks" }
      : t.state === "review" ? { cls: "wait", text: "review" } : t.state === "blocked" ? { cls: "wait", text: "blocked" } : null,
  }));
  return { count, rows, empty: list.length ? "Nothing open" : "No tasks yet" };
}

/// A machine row as the tile reads it: `whereRows` gives them.
type Where = Pick<WhereRow, "kind" | "name"> & Partial<Pick<WhereRow, "id" | "folder" | "text">>;

/// The Machines tile: every machine with where the project is there.
export function machineTile(rows: readonly Where[] | null | undefined, sync?: readonly Pick<SyncRow, "project" | "machine_id" | "status" | "files" | "pending">[] | null): TileData {
  const list = rows || [];
  const here = list.filter(r => r.kind === "here").length;
  const note = (r: Where) => {
    const s = machineSync(r.id, sync);
    if (s) return s.pct == null ? "scanning" : `syncing ${s.pct}%`;
    return r.kind === "here" ? folderName(r.folder) : r.kind === "checking" ? "checking" : r.kind === "offline" ? "offline" : "not here";
  };
  return {
    count: list.length ? `${here}/${list.length}` : "",
    rows: list.map(r => ({
      id: r.id,
      text: r.name,
      dot: r.kind === "here" ? "" : "off",
      note: note(r),
      title: r.folder ? `${r.name}: ${r.folder}` : r.text || r.name,
    })),
    empty: "No machine has it yet",
  };
}

/// The Agents tile: the coding agents running in the project's folders
/// now, then its past sessions, newest first.
export function agentTile(
  running: readonly Partial<Pick<RunningAgent, "title" | "cwd" | "agent" | "machine_name" | "status">>[] | null | undefined,
  past: readonly Partial<Pick<SessionRow, "title" | "status">>[] | null | undefined,
): TileData {
  const now = running || [], before = past || [];
  const rows: TileRow[] = [
    ...now.map(r => ({ text: r.title || folderName(r.cwd) || r.agent, note: r.machine_name, chip: r.status === "asking" ? { cls: "wait", text: "asks" } : null })),
    ...before.map(s => ({ text: s.title || "Untitled session", note: s.status === "ended" ? "ended" : s.status || "" })),
  ];
  const count = now.length ? `${now.length} running` : before.length ? `${before.length} past` : "";
  return { count, rows, empty: "No agent has worked here yet" };
}

/// The last folder of a path.
function folderName(path: string | null | undefined) { return String(path || "").split(/[\\/]/).filter(Boolean).pop() || ""; }
