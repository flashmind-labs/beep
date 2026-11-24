// The commands the open session's agent runs in the background: one line
// above the field with their count, which opens the Running tab, where each
// one shows with its Stop button.
import { Spinner } from "../ui/kit.tsx";
import { showAgentControl } from "./composer-ui.tsx";
import { type AgentTask, type OpenAgent, AG, openTasks, AGENT_READ_WAIT_MS, where } from "./agents.ts";
import { steerAgent } from "./messages.ts";
import { invoke } from "../ui/tauri.ts";
import { plural } from "../ui/format.ts";
import { backgroundTasks, paneTasks, sessionKey } from "../agentpane/running.ts";
import { showRunning } from "../agentpane/media.ts";
import { draw } from "../agentpane/media-draw.ts";
import { isStarting } from "./starting-state.ts";

/// The tasks asked to stop, until they end.
const stopping = new Set<string>();
/// What the Running tab was last given.
let shown = "";

/// The open session's tasks; the last ones when a read fails.
export async function readTasks(o: OpenAgent) {
  const s = AG.page?.session;
  if (!s || s.external || isStarting(s.id) || s.status === "ended") return [];

  try {
    return await invoke<AgentTask[]>("agent_tasks", where(o), AGENT_READ_WAIT_MS);
  } catch (e) {
    console.warn("agent tasks not read", e);
    return openTasks();
  }
}

/// A task that has not ended.
export const runs = (t: AgentTask) => t.state === "running" || t.state === "paused";

/// Whether the log entry `at` is a call whose command still runs in the
/// background, though the call itself is done.
export const backgroundedEntry = (at: number) => openTasks().some(t => t.entry === at && runs(t));

export function drawTasks() {
  const live = openTasks().filter(runs);
  for (const id of stopping) if (!live.some(t => t.id === id)) stopping.delete(id);

  const session = sessionKey(AG.page?.session), signature = JSON.stringify([session, live, [...stopping]]);
  if (signature !== shown) {
    shown = signature;
    const before = paneTasks().length;
    backgroundTasks.setState({ session, tasks: live, stopping: new Set(stopping), stop: id => void stopTask(id) }, true);
    // The Running tab shows while there are some, and its count follows at once.
    if (paneTasks().length !== before) draw();
  }

  // No tasks draws nothing, and an unchanged line does not draw again.
  showAgentControl("tasks", live.length === 0 ? null : <TasksLine count={live.length} />);
}

/// "3 commands running", which opens the Running tab.
function TasksLine({ count }: { count: number }) {
  return (
    <button type="button" className="ag-tasks" title="Show them in the Running tab" onClick={showRunning}>
      <Spinner dot />
      <span>{plural(count, "command", "commands")} running</span>
    </button>
  );
}

/// Stop was clicked on task `id` in the Running tab.
async function stopTask(id: string) {
  if (stopping.has(id)) return;
  stopping.add(id);
  drawTasks();
  // A refusal says why, and the next read shows the task as it is.
  if (!await steerAgent({ stop_task: { task: id } })) { stopping.delete(id); drawTasks(); }
}
