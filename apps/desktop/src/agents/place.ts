// The place of each session the rows show, asked of its machine in one call
// for all its sessions waiting, and kept. Rows read `placeOf` while they
// draw; what it does not know yet is asked for once, and the views that
// registered redraw once the answers are in. A place changes with the files
// of its folder: the `files` update of a folder followed pushes it.

import { moveLabel } from "./move-store.ts";
import { type PlaceAsk, type WorkPlace, placeKey, placeSub } from "./place-state.ts";
import { type RunningAgent, runningLine } from "./running-state.ts";
import { pushedEach } from "../ui/snapshot-state.ts";
import { invoke } from "../ui/tauri.ts";

/// How long a read that failed waits before it is asked again, and more on each failure.
const FAILED_MS = 30_000;
/// How long a machine that did not answer yet, as while it starts, waits
/// before it is asked again.
const UNREACHABLE_MS = 5_000;
/// The most sessions asked of a machine in one call.
const BATCH = 50;
/// The most answers kept, the least recently read going first. Far more
/// than any view draws: a view with more rows than this would ask again for
/// the rows that fell out on every redraw.
const KEPT = 5000;

/// What `agent_places` answers: each session's place in order, or that the
/// machine did not answer yet.
type PlacesAnswer = { kind: "found"; places: (WorkPlace | null)[] } | { kind: "unreachable" };

interface KnownPlace {
  place: WorkPlace | null;
  at: number;
  failures: number;
  retryAt: number;
}
const known = new Map<string, KnownPlace>();
const waiting = new Map<string, PlaceAsk>();
const asking = new Set<string>();
/// The machines asked now.
const askingOn = new Set<string>();
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setTimeout> | undefined;
let pumping = false;

/// `redraw` runs when new places arrive.
export function onPlaces(redraw: () => void) {
  listeners.add(redraw);
}

/// The place of `r` as last answered or pushed, asking when it is unknown or its last read failed.
export function placeOf(r: PlaceAsk) {
  const key = placeKey(r);
  const got = known.get(key);
  // Read last, so it goes last.
  if (got) { known.delete(key); known.set(key, got); }

  if (!got || (got.at === 0 && Date.now() >= got.retryAt)) {
    if (!asking.has(key) && !waiting.has(key)) { waiting.set(key, r); pumpSoon(); }
  }

  return got?.place ?? null;
}

/// Asks once the draw that read the places is done, so its rows go together.
function pumpSoon() {
  if (pumping) return;
  pumping = true;
  queueMicrotask(() => { pumping = false; pump(); });
}

function pump() {
  const batches = new Map<string, [string, PlaceAsk][]>();

  for (const [key, ask] of waiting) {
    if (askingOn.has(ask.machine_id)) continue;
    const batch = batches.get(ask.machine_id) ?? [];
    if (batch.length < BATCH) batch.push([key, ask]);
    batches.set(ask.machine_id, batch);
  }

  for (const [machine, batch] of batches) ask(machine, batch);
}

/// Asks `machine` where the sessions of `batch` work, and asks again for
/// what waited meanwhile.
function ask(machine: string, batch: [string, PlaceAsk][]) {
  askingOn.add(machine);
  for (const [key] of batch) { waiting.delete(key); asking.add(key); }
  const asks = batch.map(([, r]) => ({ agent: r.agent, session: r.id, external: !!r.external }));
  invoke<PlacesAnswer>("agent_places", { machineId: machine, asks })
    .then(answer => answer, () => null)
    .then(answer => {
      askingOn.delete(machine);
      let moved = false;
      batch.forEach(([key], i) => {
        asking.delete(key);
        if (answer?.kind === "found") moved = settle(key, answer.places[i] ?? null, null) || moved;
        else settle(key, null, answer ? UNREACHABLE_MS : FAILED_MS);
      });
      if (moved) changed();
      pump();
    });
}

/// Keeps what was read of `key`: `place` when it was answered, else, asked
/// again after `retry` ms and longer on each failure, the place known before,
/// so views do not switch away and back. Whether the place changed.
function settle(key: string, place: WorkPlace | null, retry: number | null) {
  const before = known.get(key);
  const now = Date.now();

  known.delete(key);

  if (retry === null) {
    keep(key, { place, at: now, failures: 0, retryAt: 0 });
    return JSON.stringify(before?.place ?? null) !== JSON.stringify(place);
  }

  const failures = Math.min((before?.failures ?? 0) + 1, 5);
  const wait = Math.min(300_000, retry * 2 ** (failures - 1));
  // Due again at `retryAt`, however old what is kept.
  keep(key, { place: before?.place ?? null, at: 0, failures, retryAt: now + wait });
  return false;
}

/// Keeps `got` as the most recently read, dropping the least recently read
/// past `KEPT`. A dropped place is asked for again only when a view reads it.
function keep(key: string, got: KnownPlace) {
  known.set(key, got);

  for (const oldest of known.keys()) {
    if (known.size <= KEPT) break;
    known.delete(oldest);
  }
}

/// Takes the places a `files` update pushes: each view answers for the sessions it was read with.
export function takePlaces(data: unknown) {
  let moved = false;

  for (const { args, pushed } of pushedEach<PlacesAnswer>(data, "agent_places")) {
    const { machineId, asks } = args;
    if (typeof machineId !== "string" || !Array.isArray(asks) || !("value" in pushed) || pushed.value.kind !== "found") continue;
    const { places } = pushed.value;

    asks.forEach((ask: unknown, i) => {
      if (typeof ask !== "object" || ask === null || !("agent" in ask) || !("session" in ask)) return;
      const key = placeKey({ machine_id: machineId, agent: String(ask.agent), id: String(ask.session) });
      if (known.has(key) && !asking.has(key)) moved = settle(key, places[i] ?? null, null) || moved;
    });
  }

  if (moved) changed();
}

/// Redraws the views once, after a burst of answers.
function changed() {
  clearTimeout(timer);
  timer = setTimeout(() => { for (const f of listeners) f(); }, 150);
}

/// A Running now row's title and sub line: the project and machine, or in a
/// linked worktree the repository, the worktree and the machine.
export function runningLineAt(r: RunningAgent, projects: Parameters<typeof runningLine>[1]) {
  const line = runningLine(r, projects);
  const placed = placeSub(placeOf(r), r.machine_name);
  const sub = placed === null ? line.sub : [placed, r.account].filter(Boolean).join(" · ");
  // A session on its way to another machine names both.
  const moving = moveLabel(r);
  return { title: line.title, sub: moving ? sub.replace(r.machine_name, moving.sub) : sub };
}
