// The Home assistant: which sessions are it, and which harness and machine it starts on.
import type { AgentHost, AgentSession } from "./agents-state.ts";
import { AGENT_ORDER, harnessFor, keepEffort } from "./agentpick.ts";
import type { AssistantPick } from "../settings/assistant.ts";

/// What a start composer says: its field to a screen reader, its heading (a new one drawn by
/// lot each time when null), the line under it and the field's placeholder.
export interface ComposerWords { label: string; title: string | null; line: string; placeholder: string }

/// A coding agent's start composer.
export const TASK_WORDS = { label: "Task for a coding agent", title: null, line: "A fresh idea. A tricky bug. A place to start.", placeholder: "Describe the task." } as const satisfies ComposerWords;
/// The assistant's start composer, on Home and from Cmd+N.
export const ASSISTANT_WORDS = { label: "Message for the assistant", title: "What can I help with?", line: "Ask about your machines and projects.", placeholder: "Ask the assistant." } as const satisfies ComposerWords;

/// A session is the assistant when started as one (Cmd+N on Home), as its node keeps it. A
/// reviewer never is.
export function isAssistant(session: Pick<AgentSession, "assistant" | "reviewer">): boolean {
  return session.assistant && !session.reviewer;
}

/// The assistant's open and ended chats, newest created first within each group.
export function assistantRows(sessions: readonly AgentSession[]): { running: AgentSession[]; ended: AgentSession[] } {
  const listed = sessions.filter(isAssistant).sort((a, b) =>
    b.started_at - a.started_at || a.id.localeCompare(b.id) || a.machine_id.localeCompare(b.machine_id));

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
  return { agent, model: agent === pick.agent ? pick.model : null, effort: keepEffort(agent, pick.effort), fast: agent === "codex" ? pick.fast ?? null : null };
}

/// The machine to start `agent` on, or list its models from, among those that run the
/// assistant: `own` when it offers the harness, else the first that does.
export function assistantHost<H extends Pick<AgentHost, "machine_id" | "agents" | "assistant">>(hosts: readonly H[], own: string | null, agent: string): H | null {
  const offering = hosts.filter(host => host.assistant && host.agents.includes(agent));
  return offering.find(host => host.machine_id === own) ?? offering[0] ?? null;
}
