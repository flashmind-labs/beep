// The Home assistant: which sessions are it, and which harness and machine it starts on.
import type { AgentHost, AgentSession } from "./agents.ts";
import { AGENT_ORDER, harnessFor, keepEffort } from "./agentpick.ts";
import type { AssistantPick } from "../settings/assistant.ts";

/// What a start composer's field is to a screen reader: a task, or a message for the assistant.
export const TASK_LABEL = "Task for a coding agent";
export const ASSISTANT_LABEL = "Message for the assistant";

/// A session is the assistant when started as one (Cmd+N on Home), as its node keeps it. A
/// reviewer never is.
export function isAssistant(session: Pick<AgentSession, "assistant" | "reviewer">): boolean {
  return session.assistant && !session.reviewer;
}

/// The assistant sessions Home lists, the latest first: those that run, and those that ended.
export function assistantRows(sessions: readonly AgentSession[]): { running: AgentSession[]; ended: AgentSession[] } {
  const listed = sessions.filter(isAssistant).sort((a, b) => b.last_activity - a.last_activity);

  return { running: listed.filter(s => s.status !== "ended"), ended: listed.filter(s => s.status === "ended") };
}

/// The harnesses the machines whose node runs the assistant offer.
export function assistantAgents(hosts: readonly Pick<AgentHost, "agents" | "assistant">[]): string[] {
  return [...new Set(hosts.filter(host => host.assistant).flatMap(host => host.agents))];
}

/// The assistant setting as a start uses it: its harness resolved to one a machine that runs
/// the assistant offers, else the first offered (null while none is), the model kept only with
/// the harness it was picked for, and a thinking level that harness takes.
export function resolveAssistant(pick: AssistantPick, hosts: readonly Pick<AgentHost, "agents" | "assistant">[]): AssistantPick {
  const agent = harnessFor(pick.agent, assistantAgents(hosts), AGENT_ORDER);
  return { agent, model: agent === pick.agent ? pick.model : null, effort: keepEffort(agent, pick.effort) };
}

/// The machine to start `agent` on, or list its models from, among those that run the
/// assistant: `own` when it offers the harness, else the first that does.
export function assistantHost<H extends Pick<AgentHost, "machine_id" | "agents" | "assistant">>(hosts: readonly H[], own: string | null, agent: string): H | null {
  const offering = hosts.filter(host => host.assistant && host.agents.includes(agent));
  return offering.find(host => host.machine_id === own) ?? offering[0] ?? null;
}
