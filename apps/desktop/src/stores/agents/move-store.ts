// What the window knows of the moves in the mesh: the records and how each
// target says it goes. The lists and the session view both read it.

import { type MoveProgress, type MoveRecord, bySession, dedupe, effective, listLabel } from "./move-state.ts";
import { single } from "../../ui/active.ts";
import { subscribeEach, topic } from "../../ui/mesh-events.ts";
import { pushedEach } from "../../ui/snapshot-state.ts";
import { invoke } from "../../ui/tauri.ts";
import { MACHINES } from "../machines/machine-store.ts";
import { createStore } from "zustand/vanilla";

/// The move records by session, and how each target says its move goes.
export const moveStore = createStore(() => ({
  records: new Map<string, MoveRecord>(),
  progress: new Map<string, MoveProgress>(),
}));

/// Calls `f` whenever the moves change, for the lists to draw again.
export const onMoves = (f: () => void) => void moveStore.subscribe(() => f());

export const nameOf = (id: string) => MACHINES.list?.find(m => m.id === id)?.name ?? id.slice(0, 8);

/// Whether a `home` push carried the moves yet; later ones keep them current.
let pushed = false;

/// Takes the moves a `home` push carried, so a move started on another desktop shows at once.
export function takePushedSessions(records: readonly MoveRecord[]) {
  pushed = true;
  takeMoves(records);
}

/// Reads the move records once, for the window's first draw before any push; reads at the same
/// time share it.
export const refreshMoves = single(async () => {
  if (pushed) return;
  const before = moveStore.getState().records;
  let records: MoveRecord[];
  try { records = await invoke<MoveRecord[]>("session_moves"); } catch (e) { console.warn("moves not read", e); return; }
  if (before !== moveStore.getState().records) return;

  takeMoves(records);
});

/// Takes `list` as the move records. Progress comes with the `moves` push, not from here.
function takeMoves(list: readonly MoveRecord[]) {
  const before = moveStore.getState();
  const read = bySession(Array.isArray(list) ? list : []);
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

  if (!changed) return;

  // A move that ended or began again is not the one its progress told of; one that has no record
  // yet keeps what the push told, for the record on its way.
  const progress = new Map(before.progress);
  for (const id of progress.keys()) {
    if (!read.has(id) || (before.records.has(id) && read.get(id) !== before.records.get(id))) progress.delete(id);
  }
  moveStore.setState({ records: read, progress });
}

// How far each move in flight is comes with the `moves` push, as `session_move_progress` views.
subscribeEach(topic("moves"), event => takeProgress(event.data));

/// Takes the progress an update of the `moves` topic carries.
export function takeProgress(data: unknown) {
  let progress: Map<string, MoveProgress> | null = null;

  for (const { args, pushed } of pushedEach<MoveProgress>(data, "session_move_progress")) {
    if (typeof args.session !== "string") continue;
    if ("error" in pushed) { console.warn("move progress not read", pushed.error); continue; }

    progress ??= new Map(moveStore.getState().progress);
    progress.set(args.session, pushed.value);
  }

  if (progress) moveStore.setState({ progress });
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
