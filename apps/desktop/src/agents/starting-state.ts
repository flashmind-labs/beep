import type { AgentSession } from "./agents-state.ts";

/// A session this device keeps until its machine names it: the desktop answers for it, not the machine.
export const isStarting = (id: string) => id.startsWith("start:");

/// What `agent-started` says once the machine named the session of start `start`.
export interface AgentStarted {
  start: string;
  session: AgentSession;
  first_message: string;
}
