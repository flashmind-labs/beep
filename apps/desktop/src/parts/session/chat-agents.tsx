// The Agents tab of an assistant chat's pane: the custom agents its folder defines, which the
// chat's agent may have written, as the project page lists a project's.
import type { AgentSession } from "../../stores/agents/agents-state.ts";
import { makeCustomAgents } from "../project/customagents.tsx";
import type { AgentsPlace } from "../../stores/project/customagents-state.ts";
import { redraw } from "../../stores/agentpane/pane-state.ts";
import { chatPane } from "./media-layout.ts";

const agents = makeCustomAgents({ redraw: () => redraw(), start: null, own: true });

/// How many agents the folder of `chat` defines, read the first time it is asked.
export function chatAgentsCount(chat: Pick<AgentSession, "machine_id" | "machine_name" | "cwd">) {
  return agents.count(place(chat));
}

/// Reads the agents of the pane's chat again, if it shows one, at most every `RELOAD_MS`:
/// the Files tree reads again on each change in the folder.
export function reloadChatAgents() {
  const chat = chatPane(), now = Date.now();
  if (!chat || now - (reloaded.get(chat.cwd) ?? 0) < RELOAD_MS) return;
  reloaded.set(chat.cwd, now);
  void agents.reload(place(chat));
}

const RELOAD_MS = 15_000;
/// When each chat folder's agents were read again last.
const reloaded = new Map<string, number>();

export function ChatAgentsPane({ hidden }: { hidden: boolean }) {
  const chat = chatPane();
  return <div className="agpview agagents" hidden={hidden}>{chat && agents.section(place(chat))}</div>;
}

function place(chat: Pick<AgentSession, "machine_id" | "machine_name" | "cwd">): AgentsPlace {
  return { id: chat.machine_id, name: chat.machine_name, folder: chat.cwd };
}
