// The Home assistant: which sessions are it, and which harness and machine it starts on.
import type { AgentHost, AgentSession } from "./agents.ts";
import { AGENT_ORDER, harnessFor, keepEffort } from "./agentpick.ts";
import type { AssistantPick, AssistantSetting, HermesConnection } from "../settings/assistant.ts";

/// The agent a node offers for the Home assistant on Hermes. It is no coding agent.
export const HERMES_AGENT = "hermes-gateway";
/// The agents nodes offer that no harness list shows.
export const HIDDEN_AGENTS: readonly string[] = [HERMES_AGENT];

/// The agents `host` offers that harness lists show.
export function visibleAgents(host: Pick<AgentHost, "agents">): string[] {
  return host.agents.filter(agent => !HIDDEN_AGENTS.includes(agent));
}

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

/// The assistant sessions Home lists, the latest first: those that run, and those that ended.
export function assistantRows(sessions: readonly AgentSession[]): { running: AgentSession[]; ended: AgentSession[] } {
  const listed = sessions.filter(isAssistant).sort((a, b) => b.last_activity - a.last_activity);

  return { running: listed.filter(s => s.status !== "ended"), ended: listed.filter(s => s.status === "ended") };
}

/// The harnesses the machines whose node runs the assistant offer.
export function assistantAgents(hosts: readonly Pick<AgentHost, "agents" | "assistant">[]): string[] {
  return [...new Set(hosts.filter(host => host.assistant).flatMap(host => host.agents))];
}

/// What the assistant starts on: Hermes on its machine while connected, else a harness.
export type AssistantStart = { kind: "hermes"; hermes: HermesConnection } | { kind: "harness"; pick: AssistantPick };

/// The assistant setting as a start uses it: a Hermes connection stands in for the harness pick.
export function resolveAssistant(setting: AssistantSetting, hosts: readonly Pick<AgentHost, "agents" | "assistant">[]): AssistantStart {
  if (setting.hermes) return { kind: "hermes", hermes: setting.hermes };
  const { agent, model, effort, fast } = setting;

  return { kind: "harness", pick: resolveHarness({ agent, model, effort, fast }, hosts) };
}

/// Hermes's machine is offline or does not offer it.
export class HermesUnreachable extends Error {
  constructor(hermes: HermesConnection) {
    super(`Hermes is not reachable on ${hermes.machine_name}.`);
  }
}

/// The machine Hermes starts on: its own, while online and offering it. Hosts are the online
/// machines, so a missing one is offline.
export function hermesMachine(hermes: HermesConnection, hosts: readonly Pick<AgentHost, "machine_id" | "hermes">[]): string {
  if (!hosts.some(host => host.machine_id === hermes.machine && host.hermes)) throw new HermesUnreachable(hermes);

  return hermes.machine;
}

/// The harness pick as a start uses it: its harness resolved to one a machine that runs
/// the assistant offers, else the first offered (null while none is), the model kept only with
/// the harness it was picked for, and a thinking level that harness takes.
export function resolveHarness(pick: AssistantPick, hosts: readonly Pick<AgentHost, "agents" | "assistant">[]): AssistantPick {
  const agent = harnessFor(pick.agent, assistantAgents(hosts), AGENT_ORDER);
  return { agent, model: agent === pick.agent ? pick.model : null, effort: keepEffort(agent, pick.effort), fast: agent === "codex" ? pick.fast ?? null : null };
}

/// The machine to start `agent` on, or list its models from, among those that run the
/// assistant: `own` when it offers the harness, else the first that does.
export function assistantHost<H extends Pick<AgentHost, "machine_id" | "agents" | "assistant">>(hosts: readonly H[], own: string | null, agent: string): H | null {
  const offering = hosts.filter(host => host.assistant && host.agents.includes(agent));
  return offering.find(host => host.machine_id === own) ?? offering[0] ?? null;
}
