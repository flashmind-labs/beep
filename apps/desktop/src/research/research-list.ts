// The command center's Research page and sidebar group: which researches
// show, in what order, and how each row reads. Drawing is in page.ts.
import { plural } from "../ui/format.ts";

/// Where a research is, as `list_researches` says it (ResearchStatus).
export type ResearchStatus = "planning" | "researching" | "reviewing" | "writing" | "done" | "failed" | "stopped" | "interrupted";

/// A research on the account, as `list_researches` lists it (ResearchItem).
export interface ResearchRow {
  session: string;
  topic: string;
  status: ResearchStatus;
  round: number;
  sources: number;
  started_at: number;
  finished_at: number | null;
}

/// What a row's status chip says: the team works, waits for the user's go,
/// finished, failed or was stopped (stopped by the user or cut off).
export type ResearchState = "running" | "waiting" | "done" | "failed" | "stopped";

export function researchState(status: ResearchStatus): ResearchState {
  switch (status) {
    case "researching": case "reviewing": case "writing": return "running";
    case "planning": return "waiting";
    case "done": return "done";
    case "failed": return "failed";
    case "stopped": case "interrupted": return "stopped";
  }
}

export const STATE_LABEL: Record<ResearchState, string> = {
  running: "Running", waiting: "Waiting for your go", done: "Done", failed: "Failed", stopped: "Stopped",
};

/// A status in words, where the exact phase matters.
export const STATUS_LABEL: Record<ResearchStatus, string> = {
  planning: "Waiting for your go", researching: "Researching", reviewing: "Reviewing",
  writing: "Writing", done: "Done", failed: "Failed", stopped: "Stopped", interrupted: "Interrupted",
};

/// The class of a research's status chip.
export function chipClass(status: ResearchStatus) {
  const state = researchState(status);
  return state === "running" ? "run" : state === "waiting" ? "wait" : state === "failed" ? "bad" : "";
}

/// A status chip as HTML, for the views drawn from strings; `label` is trusted.
export function chipHtml(status: ResearchStatus, label: string) {
  const spin = researchState(status) === "running" ? '<span class="dspin"></span>' : "";
  return `<span class="chip ${chipClass(status)}">${spin}${label}</span>`;
}

/// A running round's steps, in order.
export const PHASES = ["researching", "reviewing", "writing"] as const;

/// The step a running research is on, from 1 to `PHASES.length`; 0 when it is not running.
export function phaseStep(status: ResearchStatus) {
  return PHASES.indexOf(status as (typeof PHASES)[number]) + 1;
}

/// Every research, newest first; ties keep the server's order.
export function newestFirst(list: readonly ResearchRow[]): ResearchRow[] {
  return [...list].sort((a, b) => b.started_at - a.started_at);
}

/// The Progress cell: the round and step while it runs, how long it took
/// once done, the round it stopped in otherwise.
export function progressLine(r: ResearchRow) {
  const round = r.round > 1 ? `Round ${r.round} · ` : "";
  const state = researchState(r.status);
  if (state === "running") return round + STATUS_LABEL[r.status];
  if (state === "waiting") return r.round > 1 ? `Round ${r.round} planned` : "Plan ready";
  if (state === "done") {
    const rounds = r.round > 1 ? `${r.round} rounds · ` : "";
    return r.finished_at != null ? `${rounds}took ${tookFor(r.finished_at - r.started_at)}` : rounds + "Done";
  }
  const how = state === "failed" ? "Failed" : r.status === "interrupted" ? "Cut off" : "Stopped";
  return `${how} in round ${r.round}`;
}

/// How long a research took: "14 min", "2 h".
function tookFor(ms: number) {
  const min = Math.max(1, Math.round(ms / 60000));
  return min < 60 ? `${min} min` : `${Math.round(min / 60)} h`;
}

/// The page's line under its title: what runs and waits, and how many in all.
export function researchSummary(list: readonly ResearchRow[]) {
  if (!list.length) return "Nothing researched yet";
  const running = list.filter(r => researchState(r.status) === "running").length;
  const waiting = list.filter(r => researchState(r.status) === "waiting").length;
  const now = [running && `${running} running`, waiting && `${waiting} waiting`].filter(Boolean).join(", ");
  const all = plural(list.length, "research", "researches");
  return now ? `${now} · ${all}` : all;
}

/// Chats (and groups) without the researches, which list under Research.
export function withoutResearch<T extends { id: string }>(rows: readonly T[], researches: ReadonlySet<string>): T[] {
  return rows.filter(c => !researches.has(c.id));
}
