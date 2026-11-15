// The Home assistant: which sessions are it, and which harness and machine it starts on.
import type { AgentHost, AgentSession } from "./agents.ts";
import { AGENT_ORDER, harnessFor, keepEffort } from "./agentpick.ts";
import type { AssistantPick } from "../settings/assistant.ts";
import type { MachineEnv } from "../machines/machine-state.ts";
import { isNewer } from "../ui/format.ts";

/// What a start composer's field is to a screen reader: a task, or a message for the assistant.
export const TASK_LABEL = "Task for a coding agent";
export const ASSISTANT_LABEL = "Message for the assistant";

/// The last released beep-node, which does not mark every assistant session itself
/// (`BEFORE_ASSISTANT` in src-tauri/src/agents/steer.rs).
const BEFORE_ASSISTANT = "0.7.12";

/// What the assistant list needs of a session's machine.
export interface AssistantMachine {
  home: string | null;
  windows: boolean;
  /// Its node marks every assistant session itself, so its flag is the answer.
  marks: boolean;
}

/// Whether a node of `version` marks every assistant session itself.
function marksAssistant(version: string): boolean {
  return /^\d/.test(version) && isNewer(version, BEFORE_ASSISTANT);
}

/// A session is the assistant when its node says so. A reviewer never is. For an older node,
/// off Windows, so is one that no session started in the machine's home folder.
export function isAssistant(session: Pick<AgentSession, "assistant" | "cwd" | "parent" | "reviewer">, machine: AssistantMachine): boolean {
  if (session.reviewer) return false;
  if (session.assistant) return true;
  if (machine.marks) return false;
  return !machine.windows && !session.parent && machine.home !== null && session.cwd === machine.home;
}

/// The assistant sessions Home lists, the latest first: those that run, and those that ended.
export function assistantRows(sessions: readonly AgentSession[], machines: readonly { id: string; version: string; env: Pick<MachineEnv, "os" | "home"> | null }[]): { running: AgentSession[]; ended: AgentSession[] } {
  const byId = new Map(machines.map(m => [m.id, m]));
  const listed = sessions
    .filter(s => {
      const m = byId.get(s.machine_id) ?? null;
      return isAssistant(s, { home: m?.env?.home || null, windows: m?.env?.os === "windows", marks: !!m && marksAssistant(m.version) });
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
