// What the window knows of the moves in the mesh: the records and how each
// target says it goes. The lists and the session view both read it.

import { type MoveProgress, type MoveRecord, bySession, dedupe, effective, listLabel } from "./move-state.ts";
import { invoke } from "../ui/tauri.ts";

/// Records older than this are not asked about again.
const RECENT_MS = 24 * 3600 * 1000;
/// The most progress answers one read asks for.
const MAX_ASKED = 8;

export const MV = {
  records: new Map<string, MoveRecord>(),
  progress: new Map<string, MoveProgress>(),
  names: new Map<string, string>(),
};
const listeners = new Set<() => void>();

/// Calls `f` whenever the moves change, for the lists to draw again.
export const onMoves = (f: () => void) => void listeners.add(f);
export const moved = () => { for (const f of listeners) f(); };

export const nameOf = (id: string) => MV.names.get(id) ?? id.slice(0, 8);

/// Reads the move records and, for the recent ones, how the target says each goes.
export async function refreshMoves() {
  let records: MoveRecord[];
  try { records = await invoke<MoveRecord[]>("session_moves"); } catch (e) { console.warn("moves not read", e); return; }
  // A node of the mesh may answer with something else; then there are none.
  MV.records = bySession(Array.isArray(records) ? records : []);
  const recent = [...MV.records.values()].filter(r => Date.now() - r.at < RECENT_MS).slice(0, MAX_ASKED);
  if (recent.length && !MV.names.size) {
    try { for (const m of await invoke<{ machine_id: string; machine_name: string }[]>("agent_hosts")) MV.names.set(m.machine_id, m.machine_name); }
    catch (e) { console.warn("machine names not read", e); }
  }
  await Promise.all(recent.map(async r => {
    try { MV.progress.set(r.session, await invoke<MoveProgress>("session_move_progress", { session: r.session })); }
    catch (e) { console.warn("move progress not read", e); }
  }));
  moved();
}

/// How the move of session `id` stands, read for the window.
export const stateOf = (id: string) => effective(MV.progress.get(id), MV.records.get(id), Date.now());

/// `rows` with a session that moved shown once.
export const withoutCopies = <T extends { machine_id: string; id: string; agent: string }>(rows: readonly T[]) => dedupe(rows, [...MV.records.values()]);

/// What a row of a session in a move says: the machines, a chip and the reason.
export function moveLabel(s: { id: string; machine_id: string; machine_name: string }) {
  const r = MV.records.get(s.id);
  if (!r || r.from !== s.machine_id) return null;
  return listLabel(r, stateOf(s.id), id => (id === s.machine_id ? s.machine_name : nameOf(id)));
}
