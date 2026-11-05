// The place of each session the rows show, asked of its machine a few at a
// time and kept a while. Rows read `placeOf` while they draw; what it does
// not know yet is asked for, and the views that registered redraw once the
// answers are in.

import { moveLabel } from "./move-store.ts";
import { type PlaceAsk, type WorkPlace, placeKey, placeSub } from "./place-state.ts";
import { type RunningAgent, runningLine } from "./running-state.ts";
import { invoke } from "../ui/tauri.ts";

/// How long an answer holds before the machine is asked again: the agent may
/// have moved.
const STALE_MS = 30_000;
/// The most asked at once.
const IN_FLIGHT = 4;
/// The most answers kept.
const KEPT = 500;

const known = new Map<string, { place: WorkPlace | null; at: number }>();
const waiting = new Map<string, PlaceAsk>();
const asking = new Set<string>();
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setTimeout> | undefined;

/// `redraw` runs when new places arrive.
export function onPlaces(redraw: () => void) {
  listeners.add(redraw);
}

/// The place of `r` as last answered, asking again when it is unknown or
/// older than `maxAge` milliseconds.
export function placeOf(r: PlaceAsk, maxAge = STALE_MS) {
  const key = placeKey(r);
  const got = known.get(key);
  if (!got || Date.now() - got.at > maxAge) {
    if (!asking.has(key) && !waiting.has(key)) { waiting.set(key, r); pump(); }
  }
  return got?.place ?? null;
}

function pump() {
  for (const [key, ask] of waiting) {
    if (asking.size >= IN_FLIGHT) return;
    waiting.delete(key);
    asking.add(key);
    invoke<WorkPlace | null>("agent_place", { machineId: ask.machine_id, agent: ask.agent, session: ask.id, external: !!ask.external })
      .catch(() => null)
      .then(place => {
        asking.delete(key);
        if (known.size >= KEPT) known.clear();
        const before = known.get(key);
        known.set(key, { place, at: Date.now() });
        if (JSON.stringify(before?.place ?? null) !== JSON.stringify(place)) changed();
        pump();
      });
  }
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
  const sub = placeSub(placeOf(r), r.machine_name) ?? line.sub;
  // A session on its way to another machine names both.
  const moving = moveLabel(r);
  return { title: line.title, sub: moving ? sub.replace(r.machine_name, moving.sub) : sub };
}
