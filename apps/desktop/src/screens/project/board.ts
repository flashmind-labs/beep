// What the project page's list says of each kind of thing in a project: its
// count and the few rows that matter now, as pure functions the tests check.
import type { WhereRow } from "../../stores/project/projects-state.ts";
import { machineSync } from "../../parts/machines/machine-state.ts";
import type { SyncRow } from "../../parts/project/filesync.ts";

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

/// The last folder of a path.
function folderName(path: string | null | undefined) { return String(path || "").split(/[\\/]/).filter(Boolean).pop() || ""; }
