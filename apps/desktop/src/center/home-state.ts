// What Home's rows say and how its sections are laid out, apart from the DOM
// so it can be tested.
import { agentName, type AgentQuestion, type RunningAgent } from "../agents/running-state.ts";
import type { MachineTraffic } from "../machines/traffic-state.ts";
import type { CardStats, MachineCard } from "../machines/machine-state.ts";
import type { ProjectRow } from "../project/projects-state.ts";

/// A Needs you row: a coding agent's question.
export interface NeedsYou { kind: "agent"; machine_id: string; machine_name: string; agent: string; session: string; title: string | null; question: AgentQuestion }

/// What `center_home` gives. A section is null when it cannot be listed.
export interface HomeView {
  machines: MachineCard[] | null;
  running: RunningAgent[] | null;
  needs_you: NeedsYou[];
  projects: ProjectRow[] | null;
}

/// Current native samples, pushed without another machine read.
export interface Telemetry {
  stats: Array<CardStats & { id: string }>;
  traffic: MachineTraffic[];
}

export function applyTelemetry(home: HomeView, telemetry: Telemetry): HomeView {
  const stats = new Map(telemetry.stats.map(sample => [sample.id, sample]));
  return { ...home, machines: home.machines?.map(machine => {
    const sample = stats.get(machine.id);
    if (!sample || (machine.stats && machine.stats.sampled_at > sample.sampled_at)) return machine;
    return { ...machine, stats: sample };
  }) ?? null };
}

/// The window opens before the mesh has answered. Keep this out of the
/// stored state so a cached or live Home can still replace it.
export function homeForDisplay(home: HomeView | null): HomeView {
  return home ?? { machines: [], running: [], needs_you: [], projects: [] };
}

const SECTIONS = ["machines", "running", "needs_you", "projects"] as const;

/// `after` with each section equal to `before`'s kept as `before`'s object,
/// so views that compare by reference skip it; null when nothing changed.
export function keepUnchanged(before: HomeView | null, after: HomeView): HomeView | null {
  if (!before) return after;
  const kept = { ...after };
  let changed = false;

  for (const section of SECTIONS) {
    if (JSON.stringify(before[section]) === JSON.stringify(after[section])) Object.assign(kept, { [section]: before[section] });
    else changed = true;
  }

  return changed ? kept : null;
}

/// A Needs you row: a title, one muted line and its one button, which opens the session.
export function needsLine(n: Pick<NeedsYou, "agent" | "title" | "machine_name"> & { question: Pick<AgentQuestion, "text"> }) {
  return { title: n.question.text, sub: [agentName(n.agent), n.title || n.machine_name].filter(Boolean).join(" · "), button: "Answer" };
}
