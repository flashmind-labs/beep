// What an agent's question asks of the user, answered from Home.
import { conflicted, invoke } from "../../ui/tauri.ts";
import type { NeedsYou } from "../../stores/center/home-state.ts";
import { state } from "../../stores/center/state.ts";
import { go } from "../../parts/center/nav.ts";

/// Opens the session an agent's question came from, where it is answered.
export function openAsking(n: NeedsYou) {
  const r = state.home?.running?.find(r => r.machine_id === n.machine_id && r.id === n.session);
  go("pane", { agent: r || { kind: "agent", machine_id: n.machine_id, machine_name: n.machine_name, id: n.session, agent: n.agent, title: n.title, cwd: "", status: "asking", external: false } });
}

/// Answers an agent's question from Home. A conflict means another device
/// answered it first, which is fine; any other failure is the caller's.
export async function answerNeed(n: NeedsYou, reply: string) {
  try {
    await invoke("center_answer_agent", { machineId: n.machine_id, agent: n.agent, session: n.session, question: n.question.id, reply });
  } catch (e) {
    if (!conflicted(e)) throw e;
  }
}
