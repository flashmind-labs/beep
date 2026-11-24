// What the window knows of the moves in the mesh: the records and how each
// target says it goes. The lists and the session view both read it.

import { type MoveProgress, type MoveRecord, awaitingMove, bySession, dedupe, effective, going, listLabel } from "./move-state.ts";
import { single, SLOW_MS } from "../ui/active.ts";
import { UNWATCHED_MS } from "../ui/pagecache.ts";
import { invoke } from "../ui/tauri.ts";
import { ensureMachines } from "../machines/machine-store.ts";
import { createStore } from "zustand/vanilla";

/// Records older than this are not asked about again.
const RECENT_MS = 24 * 3600 * 1000;
/// The most progress answers one read asks for.
const MAX_ASKED = 8;
const RETRY_MS = 30_000;

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

// Each record is one move attempt. A later move of the same session gets a new deadline.
const nextCheck = new WeakMap<MoveRecord, number>();

/// Lists recent moves and shares progress reads across the views that requested them.
export const refreshMoves = single(async () => {
  const before = moveStore.getState();
  let records: MoveRecord[];
  try { records = await invoke<MoveRecord[]>("session_moves"); } catch (e) { console.warn("moves not read", e); return; }
  if (before.records !== moveStore.getState().records) return;

  const read = bySession(Array.isArray(records) ? records : []);
  let changed = read.size !== before.records.size;

  for (const [id, old] of before.records) {
    const record = read.get(id);
    if (record && old.at === record.at && old.from === record.from && old.to === record.to
      && old.agent === record.agent && old.project === record.project) {
      read.set(id, old);
    } else {
      changed = true;
    }
  }

  if (changed) {
    const progress = new Map(before.progress);
    for (const id of progress.keys()) {
      if (!read.has(id) || read.get(id) !== before.records.get(id)) progress.delete(id);
    }
    moveStore.setState({ records: read, progress });
  }

  const now = Date.now();
  const due = (r: MoveRecord) => nextCheck.get(r) ?? 0;
  const recent = [...read.values()].filter(r => now - r.at < RECENT_MS && due(r) <= now)
    .sort((a, b) => due(a) - due(b) || b.at - a.at).slice(0, MAX_ASKED);
  if (!recent.length) return;

  if (!machineNames.size) {
    try { for (const m of await ensureMachines()) machineNames.set(m.id, m.name); }
    catch (e) { console.warn("machine names not read", e); }
  }

  const answers = await Promise.all(recent.map(readProgress));
  const current = moveStore.getState();
  let updated: Map<string, MoveProgress> | null = null;
  for (const result of answers) {
    if (!result?.answer || current.records.get(result.record.session) !== result.record) continue;
    updated ??= new Map(current.progress);
    updated.set(result.record.session, result.answer);
  }
  if (updated) moveStore.setState({ progress: updated });
});

async function readProgress(record: MoveRecord) {
  try {
    const answer = await invoke<MoveProgress>("session_move_progress", { session: record.session });
    // Released targets answer by session ID. Check fresh records again soon in case
    // the target still described the previous move, or only just received this one.
    const now = Date.now();
    let interval = SLOW_MS;
    if (!awaitingMove(record, now) && !going(answer)) interval = answer?.answer === "known" ? UNWATCHED_MS : RETRY_MS;
    nextCheck.set(record, now + interval);
    return { record, answer };
  } catch (e) {
    nextCheck.set(record, Date.now() + RETRY_MS);
    console.warn("move progress not read", e);
    return null;
  }
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
