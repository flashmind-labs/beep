// What Home's rows say and how its sections are laid out, apart from the DOM
// so it can be tested.
import { agentName, type AgentQuestion, type ChatApproval, type RunningRow } from "../agents/running-state.ts";
import type { MachineTraffic } from "../machines/traffic-state.ts";
import type { CardStats, MachineCard } from "../machines/machine-state.ts";
import type { ProjectRow } from "../project/projects-state.ts";

/// A Needs you row.
export type NeedsYou =
  | { kind: "chat"; session_id: string; session_title: string; pending: ChatApproval }
  | { kind: "agent"; machine_id: string; machine_name: string; agent: string; session: string; title: string | null; question: AgentQuestion };

/// A routine coming up (CronRow).
export interface CronRow {
  id: string;
  label: string;
  prompt: string;
  schedule: string;
  /// RFC 3339, in UTC.
  next_run_at: string;
}

/// Something the event triage agent found in one of the user's apps (UpdateRow).
export interface UpdateRow {
  id: number;
  summary: string;
  app: string;
  app_name: string;
  at: number;
}

/// What `center_home` gives. A section is null when the server cannot list
/// it for this account.
export interface HomeView {
  machines: MachineCard[] | null;
  running: RunningRow[] | null;
  needs_you: NeedsYou[];
  coming_up: CronRow[] | null;
  updates: UpdateRow[] | null;
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

/// A serverless window opens before the mesh has answered. Keep this out of
/// the stored state so a cached or live Home can still replace it.
export function homeForDisplay(home: HomeView | null, server: boolean): HomeView | null {
  if (home || server) return home;
  return { machines: [], running: [], needs_you: [], coming_up: null, updates: null, projects: [] };
}

/// A Needs you row: a title, one muted line and its one button. A chat
/// approval is allowed from the row; an agent's question opens its session.
export function needsLine(n:
  | { kind: "chat"; session_title: string; pending: { approval: Pick<ChatApproval["approval"], "app" | "action"> } }
  | { kind: "agent"; agent: string; title: string | null; machine_name: string; question: Pick<AgentQuestion, "text"> }) {
  if (n.kind === "chat") {
    const a = n.pending.approval;
    const action = a.action ? a.action[0].toUpperCase() + a.action.slice(1) : "Allow this";
    return { title: `${action}?`, sub: [a.app, n.session_title].filter(Boolean).join(" · "), button: "Allow" };
  }
  return { title: n.question.text, sub: [agentName(n.agent), n.title || n.machine_name].filter(Boolean).join(" · "), button: "Answer" };
}
