// What Home's rows say and how its sections are laid out, apart from the DOM
// so it can be tested.
import { agentName, type AgentQuestion, type ChatApproval, type RunningRow } from "../agents/running-state.ts";
import type { MachineCard } from "../machines/machine-state.ts";
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
