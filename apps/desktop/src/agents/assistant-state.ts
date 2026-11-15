// The Home assistant: which sessions are it, and which harness and machine it starts on.
import type { AgentHost, AgentSession } from "./agents.ts";
import { AGENT_ORDER, harnessFor, keepEffort } from "./agentpick.ts";
import type { AssistantPick } from "../settings/assistant.ts";
import type { MachineEnv } from "../machines/machine-state.ts";

/// A session is the assistant when started as one, or, off Windows, when an
/// agent no other started works in the machine's home folder, as one found
/// running there does.
export function isAssistant(session: Pick<AgentSession, "assistant" | "cwd" | "parent">, home: string | null, windows: boolean): boolean {
  if (session.assistant) return true;
  return !windows && !session.parent && home !== null && session.cwd === home;
}

/// The assistant sessions Home lists, the latest first: those that run, and those that ended.
export function assistantRows(sessions: readonly AgentSession[], machines: readonly { id: string; env: Pick<MachineEnv, "os" | "home"> | null }[]): { running: AgentSession[]; ended: AgentSession[] } {
  const envs = new Map(machines.map(m => [m.id, m.env]));
  const listed = sessions
    .filter(s => {
      const env = envs.get(s.machine_id) ?? null;
      return isAssistant(s, env?.home || null, env?.os === "windows");
    })
    .sort((a, b) => b.last_activity - a.last_activity);

  return { running: listed.filter(s => s.status !== "ended"), ended: listed.filter(s => s.status === "ended") };
}

/// The harness the assistant starts with: the one picked when a machine offers it, else the
/// first offered; null while none is.
export function assistantHarness(agent: string | null, hosts: readonly Pick<AgentHost, "agents">[]): string | null {
  return harnessFor(agent, [...new Set(hosts.flatMap(host => host.agents))], AGENT_ORDER);
}

/// The assistant setting as a start uses it: its harness resolved to one a machine offers,
/// the model kept only with the harness it was picked for, and a thinking level that harness takes.
export function resolveAssistant(pick: AssistantPick, hosts: readonly Pick<AgentHost, "agents">[]): AssistantPick {
  const agent = assistantHarness(pick.agent, hosts);
  return { agent, model: agent === pick.agent ? pick.model : null, effort: keepEffort(agent, pick.effort) };
}

/// The machine to start `agent` on, or list its models from: `own` when it offers the harness,
/// else the first that does.
export function assistantHost<H extends Pick<AgentHost, "machine_id" | "agents">>(hosts: readonly H[], own: string | null, agent: string): H | null {
  const offering = hosts.filter(host => host.agents.includes(agent));
  return offering.find(host => host.machine_id === own) ?? offering[0] ?? null;
}
