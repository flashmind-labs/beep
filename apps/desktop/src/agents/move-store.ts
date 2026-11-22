// What the window knows of the moves in the mesh: the records and how each
// target says it goes. The lists and the session view both read it.

import { type MoveProgress, type MoveRecord, bySession, dedupe, effective, listLabel } from "./move-state.ts";
import { invoke } from "../ui/tauri.ts";
import { createStore } from "zustand/vanilla";

/// Records older than this are not asked about again.
const RECENT_MS = 24 * 3600 * 1000;
/// The most progress answers one read asks for.
const MAX_ASKED = 8;

/// The move records by session, and how each target says its move goes.
export const moveStore = createStore(() => ({
  records: new Map<string, MoveRecord>(),
  progress: new Map<string, MoveProgress>(),
}));
/// Machine names by id, for machines a move names.
export const machineNames = new Map<string, string>();

/// Calls `f` whenever the moves change, for the lists to draw again.
export const onMoves = (f: () => void) => void moveStore.subscribe(() => f());

export const nameOf = (id: string) => machineNames.get(id) ?? id.slice(0, 8);

/// Reads the move records and, for the recent ones, how the target says each goes.
export async function refreshMoves() {
  let records: MoveRecord[];
  try { records = await invoke<MoveRecord[]>("session_moves"); } catch (e) { console.warn("moves not read", e); return; }
  // A node of the mesh may answer with something else; then there are none.
  const read = bySession(Array.isArray(records) ? records : []);
  // Published at once, so a start or cancel made during the reads below stays.
  moveStore.setState({ records: read });
  // The latest first: a chat's move writes a record for each of its threads.
  const recent = [...read.values()].filter(r => Date.now() - r.at < RECENT_MS).sort((a, b) => b.at - a.at).slice(0, MAX_ASKED);

  if (recent.length && !machineNames.size) {
    try { for (const m of await invoke<{ machine_id: string; machine_name: string }[]>("agent_hosts")) machineNames.set(m.machine_id, m.machine_name); }
    catch (e) { console.warn("machine names not read", e); }
  }

  const answers = await Promise.all(recent.map(async r => {
    try { return [r.session, await invoke<MoveProgress>("session_move_progress", { session: r.session })] as const; }
    catch (e) { console.warn("move progress not read", e); return null; }
  }));
  const progress = new Map(moveStore.getState().progress);
  for (const answer of answers) if (answer) progress.set(...answer);
  moveStore.setState({ progress });
}

/// How the move of session `id` stands, read for the window.
export const stateOf = (id: string) => effective(moveStore.getState().progress.get(id), moveStore.getState().records.get(id), Date.now());

/// `rows` with a session that moved shown once.
export const withoutCopies = <T extends { machine_id: string; id: string; agent: string }>(rows: readonly T[]) => dedupe(rows, [...moveStore.getState().records.values()]);

/// What a row of a session in a move says: the machines, a chip and the reason.
export function moveLabel(s: { id: string; machine_id: string; machine_name: string }) {
  const r = moveStore.getState().records.get(s.id);
  if (!r || r.from !== s.machine_id) return null;
  return listLabel(r, stateOf(s.id), id => (id === s.machine_id ? s.machine_name : nameOf(id)));
}
