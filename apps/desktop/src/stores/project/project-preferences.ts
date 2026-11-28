import { parseProjectSort, type ProjectSort } from "./overview-state.ts";

const SORT_KEY = "beep.projects.sort";

export function readProjectSort(): ProjectSort {
  try { return parseProjectSort(localStorage.getItem(SORT_KEY)); } catch { return "default"; }
}

export function saveProjectSort(sort: ProjectSort) {
  try { localStorage.setItem(SORT_KEY, sort); } catch { /* kept for this launch only */ }
}

const VISITS_KEY = "beep.projects.visited";
let visits = new Map<string, number>();
try {
  const saved: unknown = JSON.parse(localStorage.getItem(VISITS_KEY) ?? "{}");
  if (saved && typeof saved === "object") visits = new Map(Object.entries(saved).filter((entry): entry is [string, number] => typeof entry[1] === "number" && Number.isFinite(entry[1])));
} catch { /* none kept */ }
let visiting: string | null = null;

export const lastProjectVisit = (name: string): number | null => visits.get(name) ?? null;
export const leaveProject = () => { visiting = null; };

export function noteProjectVisit(name: string) {
  if (visiting === name) return;
  visiting = name;
  visits.set(name, Date.now());
  try { localStorage.setItem(VISITS_KEY, JSON.stringify(Object.fromEntries(visits))); } catch { /* kept for this launch only */ }
}
