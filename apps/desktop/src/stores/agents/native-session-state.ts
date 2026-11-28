import type { AgentRow, AgentSession } from "./agents-state.ts";

export const isNativeSession = (id: string) => /^subagent:(?:(?:codex|agy):[\w-]+|claude:[\w-]+:[\w-]+)$/.test(id);

/// A native thread shares its parent's machine and harness, with its own id and state.
export function nativeSession(parent: AgentRow, id: string, title: string, status: string): AgentSession | null {
  if (!isNativeSession(id) || !id.startsWith(`subagent:${parent.agent}:`)) return null;
  return {
    machine_id: parent.machine_id, machine_name: parent.machine_name, agent: parent.agent,
    id, title, cwd: parent.cwd, external: true, status: status === "pending" || status === "in_progress" ? "working" : "idle",
    doing: null, question: null, started_at: 0, last_activity: 0, assistant: false, persona: null,
    parent: { machine_id: parent.machine_id, id: parent.id, key: "" },
  };
}

/// The node describes native threads as external sessions. Keep their ancestry
/// across streamed updates without borrowing the parent's status or usage.
export function nativeUpdate(previous: AgentRow, update: AgentRow): AgentRow {
  if (!isNativeSession(update.id) || previous.id !== update.id || previous.machine_id !== update.machine_id) return update;
  return { ...update, parent: previous.parent, title: update.title || previous.title, status: update.status === "ended" ? "idle" : update.status };
}
