// The Agents tab of an assistant chat's pane: the custom agents its folder defines, which the
// chat's agent may have written, as the project page lists a project's.
import type { AgentSession } from "../agents/agents.ts";
import { makeCustomAgents } from "../project/customagents.tsx";
import type { AgentsPlace } from "../project/customagents-state.ts";
import { draw } from "./media-draw.ts";
import { chatPane } from "./media-layout.ts";

const agents = makeCustomAgents({ redraw: () => draw(), start: null, own: true });

/// How many agents the folder of `chat` defines, read the first time it is asked.
export function chatAgentsCount(chat: Pick<AgentSession, "machine_id" | "machine_name" | "cwd">) {
  return agents.count(place(chat));
}

/// Reads the agents of the pane's chat again, if it shows one.
export function reloadChatAgents() {
  const chat = chatPane();
  if (chat) void agents.reload(place(chat));
}

export function ChatAgentsPane({ hidden }: { hidden: boolean }) {
  const chat = chatPane();
  return <div className="agpview agagents" hidden={hidden}>{chat && agents.section(place(chat))}</div>;
}

function place(chat: Pick<AgentSession, "machine_id" | "machine_name" | "cwd">): AgentsPlace {
  return { id: chat.machine_id, name: chat.machine_name, folder: chat.cwd };
}
