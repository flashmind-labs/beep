// What a terminal list shows: grouping by machine, where, and how each stands.
import type { Ago } from "../../ui/format.ts";
import type { TileData } from "../../screens/project/board.ts";
import type { MachineTerminals, TerminalMachine, TerminalRow } from "./terminal-state.ts";

/// The scope of the Terminals page: every terminal, of any project or none.
export const ALL = "";

/// Every machine with its terminals, in the machines' order; a terminal of
/// a machine no longer listed goes under its own name at the end, offline.
export function byMachine<T extends Pick<TerminalRow, "machine_id" | "machine_name">>(machines: readonly TerminalMachine[], list: readonly T[]): MachineTerminals<T>[] {
  const out: MachineTerminals<T>[] = machines.map(m => ({ id: m.id, name: m.name, online: m.online, terms: [] }));

  for (const t of list) {
    let m = out.find(x => x.id === t.machine_id);
    if (!m) out.push(m = { id: t.machine_id, name: t.machine_name || "Removed machine", online: false, terms: [] });
    m.terms.push(t);
  }

  return out;
}

/// Where a terminal runs: its project and folder, or the folder alone.
export function terminalWhere(t: Pick<TerminalRow, "project" | "cwd">) {
  return t.project ? `${t.project} · ${t.cwd}` : t.cwd;
}

/// How a terminal stands in a list: how long it has run, or how it ended,
/// with its exit code.
export function terminalNote(t: Pick<TerminalRow, "state" | "started_at"> & Partial<Pick<TerminalRow, "code">>, ago: Ago) {
  if (t.state === "running") return ago(t.started_at);
  if (t.state === "exited" && t.code != null) return `exited ${t.code}`;
  return stateLabel(t);
}

/// "N on M machines", counting the machines that have any.
export function terminalsCount(groups: readonly MachineTerminals<unknown>[]) {
  const n = groups.reduce((a, m) => a + m.terms.length, 0);
  const on = groups.filter(m => m.terms.length).length;
  if (!n) return "None open";
  return `${n} on ${on} ${on === 1 ? "machine" : "machines"}`;
}

/// "running", "exited 1", "stopped", "offline", "gone".
export function stateLabel(t: Pick<TerminalRow, "state"> & Partial<Pick<TerminalRow, "code">>) {
  if (t.state === "exited") return t.code == null ? "ended" : t.code === 0 ? "done" : `exited ${t.code}`;
  return t.state;
}

/// The board's Terminals tile from the project's terminals; undefined while
/// they load.
export function terminalTile(
  list: readonly (Pick<TerminalRow, "name" | "state" | "started_at"> & Partial<Pick<TerminalRow, "code" | "machine_name" | "command" | "cwd">>)[] | undefined,
  ago: Ago,
): TileData {
  if (list === undefined) return { count: "", rows: [], empty: "Loading…" };
  const running = list.filter(t => t.state === "running").length;
  return {
    count: running ? `${running} running` : list.length ? String(list.length) : "",
    rows: list.map(t => ({
      code: t.name,
      dot: t.state === "running" ? "" : "off",
      note: t.state === "running" ? ago(t.started_at) : stateLabel(t),
      title: `${t.machine_name}: ${t.command} in ${t.cwd}`,
    })),
    empty: "No terminals yet",
  };
}
