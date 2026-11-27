// What Home's rows say and how its sections are laid out, apart from the DOM
// so it can be tested.
import { agentName, type AgentQuestion, type RunningAgent } from "../agents/running-state.ts";
import type { AgentSession } from "../agents/agents-state.ts";
import type { MachineTraffic } from "../machines/traffic-state.ts";
import type { CardStats, MachineCard } from "../../parts/machines/machine-state.ts";
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

/// `home` with each machine card named as the machine list names it, and without a machine this
/// window removed. A card the list does not have yet stays: Home's own push says it exists. The
/// cards keep what only Home reads. `home` itself when nothing differs.
export function withListedMachines(home: HomeView, listed: readonly { id: string; name: string }[] | null, removed: ReadonlySet<string>): HomeView {
  if (!home.machines) return home;

  const names = new Map((listed ?? []).map(machine => [machine.id, machine.name]));
  const cards = home.machines.filter(card => !removed.has(card.id)).map(card => {
    const name = names.get(card.id);
    return name === undefined || name === card.name ? card : { ...card, name };
  });
  const same = cards.length === home.machines.length && cards.every((card, i) => card === home.machines?.[i]);

  return same ? home : { ...home, machines: cards };
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

/// A Needs you row: the question as its title, and one muted line.
export function needsLine(n: Pick<NeedsYou, "agent" | "title" | "machine_name"> & { question: Pick<AgentQuestion, "text" | "detail"> }) {
  return { title: n.question.text, sub: [n.question.detail, agentName(n.agent), n.title || n.machine_name].filter(Boolean).join(" · ") };
}

/// Replaces the start placeholder `start` in `home.running` with `session`, or
/// drops `start` when `session` already arrived or is not a top-level live row.
export function startedInHome(home: HomeView, start: string, session: AgentSession): HomeView {
  if (!home.running) return home;
  const live = session.status !== "ended" && !session.reviewer && !session.parent;
  const named: RunningAgent = { ...session, kind: "agent" };
  const hadStart = home.running.some(r => r.id === start);
  const hadSession = home.running.some(r => r.machine_id === session.machine_id && r.id === session.id);
  if (!hadStart && (!live || hadSession)) return home;

  const running: RunningAgent[] = [];
  let placed = false;

  for (const row of home.running) {
    if (row.id === start) {
      if (live && !hadSession && !placed) { running.push(named); placed = true; }
      continue;
    }
    if (row.machine_id === session.machine_id && row.id === session.id) {
      if (live && !placed) { running.push(named); placed = true; }
      continue;
    }
    running.push(row);
  }

  if (live && !placed) running.unshift(named);
  return { ...home, running };
}

