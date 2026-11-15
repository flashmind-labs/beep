// The Home assistant: which sessions are it, and which harness and machine it starts on.
import type { AgentHost, AgentSession } from "./agents.ts";
import { AGENT_ORDER, harnessFor, keepEffort } from "./agentpick.ts";
import type { AssistantPick } from "../settings/assistant.ts";
import type { MachineEnv } from "../machines/machine-state.ts";

/// A session is the assistant when started as one, or, off Windows, when one
/// found running outside Beep works in the machine's home folder. Beep's own
/// starts say what they are.
export function isAssistant(session: Pick<AgentSession, "assistant" | "cwd" | "parent" | "external">, home: string | null, windows: boolean): boolean {
  if (session.assistant) return true;
  return session.external && !windows && !session.parent && home !== null && session.cwd === home;
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
