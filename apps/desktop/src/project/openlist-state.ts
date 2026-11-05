// What the Issues and Pull requests lists share: when an item opened, its
// order, and which issues Start N agents can take, as plain data.

import { barPlace, type BarHost } from "./projectbar.ts";
import { harnessFor } from "../agents/agentpick.ts";

/// When an item opened, as a filter.
export type OpenedIn = "any" | "today" | "yesterday" | "week";

export type Order = "newest" | "oldest";

export const OPENED_IN: readonly { id: OpenedIn; label: string }[] = [
  { id: "any", label: "Any time" },
  { id: "today", label: "Today" },
  { id: "yesterday", label: "Yesterday" },
  { id: "week", label: "This week" },
];

/// Midnight starting the local day `back` days before `now`.
function dayStart(now: number, back = 0) {
  const d = new Date(now);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() - back).getTime();
}

/// Whether an item opened at `opened` (ms since the epoch; none or 0 when
/// unknown) is in `window`, by the local calendar: today, only yesterday, or
/// since Monday. An item with no time shows under Any time alone.
export function openedIn(opened: number | null | undefined, window: OpenedIn, now: number) {
  if (window === "any") return true;
  if (!opened) return false;
  const today = dayStart(now);
  if (window === "today") return opened >= today && opened < dayStart(now, -1);
  if (window === "yesterday") return opened >= dayStart(now, 1) && opened < today;
  const sinceMonday = (new Date(now).getDay() + 6) % 7;
  return opened >= dayStart(now, sinceMonday) && opened < dayStart(now, -1);
}

/// `items` in `window`, by open time in `order`; those with no time last in
/// either order, and ties as they were.
export function arrange<T>(items: readonly T[], opened: (item: T) => number | null | undefined, window: OpenedIn, order: Order, now: number): T[] {
  const sign = order === "newest" ? -1 : 1;
  return items
    .filter(i => openedIn(opened(i), window, now))
    .map((item, at) => ({ item, at, time: opened(item) || 0 }))
    .sort((a, b) => (a.time && b.time ? sign * (a.time - b.time) : Number(!a.time) - Number(!b.time)) || a.at - b.at)
    .map(x => x.item);
}

/// A list's choices: when it opened, the order, and the issues picked.
export interface ListChoice {
  window: OpenedIn;
  order: Order;
  selecting: boolean;
  /// Whether the start confirmation shows.
  confirming: boolean;
  picked: Set<string>;
}

export const newChoice = (): ListChoice => ({ window: "any", order: "newest", selecting: false, confirming: false, picked: new Set() });

/// The picked items that are still listed, in the order shown.
export function pickedInOrder<T extends { key: string }>(shown: readonly T[], picked: ReadonlySet<string>) {
  return shown.filter(i => picked.has(i.key));
}

/// "Start 3 agents".
export function startLabel(n: number) {
  return `Start ${n} ${n === 1 ? "agent" : "agents"}`;
}

/// An issue Start N agents takes: its project's name (none when it is not
/// imported).
export interface StartEntry {
  key: string;
  project: string | null;
  number: number;
  title: string;
  url: string;
}

/// Where one picked issue starts, or why it is left out.
export type StartRow =
  | { entry: StartEntry; skip: string }
  | { entry: StartEntry; skip: null; machine_id: string; machine_name: string; cwd: string; agent: string };

/// Each picked issue's machine, checkout and harness: the bar's machine for
/// its own project and Auto for any other, the bar's harness where that
/// machine has it, else the first it offers. An issue no machine has a
/// checkout for is left out.
export function planStarts(
  entries: readonly StartEntry[],
  places: (project: string) => readonly { machine: string; cwd: string }[],
  hosts: readonly BarHost[],
  machineFor: (project: string) => string,
  harness: string | null,
  order: readonly string[],
): StartRow[] {
  return entries.map((entry): StartRow => {
    if (!entry.project) return { entry, skip: "not imported" };
    const here = places(entry.project);
    const place = barPlace(here, hosts, machineFor(entry.project)) ?? barPlace(here, hosts, "auto");
    if (!place) return { entry, skip: "no machine has a checkout" };
    const offered = hosts.find(h => h.machine_id === place.machine_id)?.agents ?? [];
    const agent = harnessFor(harness, offered, order);
    if (!agent) return { entry, skip: `no coding agent on ${place.machine_name}` };
    return { entry, skip: null, machine_id: place.machine_id, machine_name: place.machine_name, cwd: place.cwd, agent };
  });
}
