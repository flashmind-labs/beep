// Commands the session left running, as the node pushes them while its pane shows.
// `RunningPane` in running-view.tsx draws `runningModel`.
import { createStore } from "zustand";
import type { RunningActions, RunningModel } from "./running-view.tsx";
import { topic } from "../ui/mesh-events.ts";
import { TopicValues } from "../ui/topic-values-state.ts";
import { parseProcesses, runningTrees, type ProcessesValue, type RunningContainer, type RunningProcess } from "./running-state.ts";
import { paneSession } from "./media.ts";
import { draw } from "./media-draw.ts";
import { shellSize, shownDocks } from "./media-layout.ts";
import { viewKey } from "./view-memory.ts";
import { paneShown } from "../ui/active.ts";
import { invoke } from "../ui/tauri.ts";
import type { AgentTask } from "../agents/agents-state.ts";
import { runs, transcriptKey, transcripts } from "../agents/transcript-cache-state.ts";
import { readError } from "./read-file.ts";

let key = "";
let unfollow: (() => void) | undefined;
/// The lists of the sessions seen lately, so one shows at once when its conversation opens again.
const lists = new TopicValues<ProcessesValue>(parseProcesses, 32);
let list: RunningProcess[] = [];
let containers: RunningContainer[] = [];
let error = "";
/// Set while `syncSession` takes the kept list, which its caller draws.
let syncing = false;

/// The Stop clicks of the background commands the session's agent runs, which the Running tab lists
/// above its processes. The commands are the open session's `transcripts` extras; the log shows only
/// their count.
export interface BackgroundStops { stopping: ReadonlySet<string>; stop(id: string): void }
export const backgroundStops = createStore<BackgroundStops>(() => ({ stopping: new Set(), stop() {} }));

/// The tasks asked to stop, as the open log draws them; the one place that changes `backgroundStops`.
export function publishBackgroundStops(next: BackgroundStops) {
  const before = backgroundStops.getState().stopping;
  if (next.stop === backgroundStops.getState().stop && next.stopping.size === before.size && [...next.stopping].every(id => before.has(id))) return;
  backgroundStops.setState(next, true);
}

/// The key of session `s` in the Running tab.
export const sessionKey = (s: { machine_id: string; id: string } | null | undefined) => s ? viewKey(s.machine_id, s.id) : "";

/// The key of the session the pane shows in `transcripts`, or null.
export const paneKey = () => {
  const s = paneSession();
  return s ? transcriptKey({ machine_id: s.machine_id, agent: s.agent, session: s.id }) : null;
};

/// The background commands of the session the pane shows, from what its follow last said.
export function paneTasks(tasks: readonly AgentTask[] = transcripts.tasks(paneKey() ?? "")) {
  return tasks.filter(runs);
}

/// The pane's count of commands when last drawn: the tab shows while there are some, and its count
/// follows at once.
let drawnTasks = 0;
transcripts.subscribe(() => { if (paneTasks().length !== drawnTasks) draw(); });

/// What the view draws, for the session the pane shows.
export const runningModel = createStore<{ model: RunningModel; actions: RunningActions } | null>(() => null);
const stopping = new Set<number>();
/// The containers asked to stop, by Docker id.
const stoppingContainers = new Set<string>();
/// Rows whose whole command shows, by pid.
const whole = new Set<number>();

function syncSession() {
  const s = paneSession();
  const next = sessionKey(s);
  if (key === next) return;
  key = next;
  unfollow?.();
  unfollow = undefined;
  list = [];
  containers = [];
  error = "";
  runningModel.setState(null, true);
  stopping.clear();
  stoppingContainers.clear();
  if (!s?.id || s.external) return;

  // The kept list is applied before this returns, with no draw: the caller draws.
  syncing = true;
  unfollow = lists.watch(s.machine_id, topic("processes", s.id), update, visible);
  syncing = false;
  if (list.length || containers.length || error) queueMicrotask(() => { if (key === next) { render(); draw(); } });
}

/// Takes a pushed list. A node that could not list keeps the last one on screen with the reason.
function update(value: ProcessesValue) {
  const count = list.length + containers.length;

  if (value.kind === "list") {
    list = value.processes;
    containers = value.containers;
    error = "";
  } else {
    error = value.message;
  }

  if (syncing || !visible()) return;
  render();
  if (list.length + containers.length !== count) draw();
}

export function runningCount() {
  syncSession();
  drawnTasks = paneTasks().length;
  return runningTrees(list).length + containers.length + drawnTasks;
}

const visible = () => paneShown() && shellSize().width > 0 && shownDocks().length > 0;

// The list changed while the pane was hidden: it draws now.
window.addEventListener("agent-pane-shown", () => { syncSession(); render(); });

const actions: RunningActions = {
  open: port => void openPort(port),
  toggle: pid => {
    if (!whole.delete(pid)) whole.add(pid);
    render();
  },
  stop: (pids, ids) => void stopAll(pids, ids),
};

function render() {
  const session = paneSession();
  if (!session || key !== sessionKey(session)) return;
  runningModel.setState({ model: { processes: list, containers, stopping: new Set(stopping), stoppingContainers: new Set(stoppingContainers), whole: new Set(whole), machine: session.machine_name, error }, actions }, true);
}

/// Another machine's port opens through a forward to this computer, on the same port when it is free.
async function openPort(port: number) {
  const s = paneSession(), asked = key;
  if (!s) return;

  try {
    const local = await invoke<number>("machine_forward_port", { machineId: s.machine_id, port });
    await invoke("open_url", { url: `http://localhost:${local}/` });
  } catch (e) {
    if (key !== asked) return;
    error = readError(e);
    render();
  }
}

/// Stops processes `pids` and containers `ids` together, and shows every failure.
async function stopAll(pids: number[], ids: string[]) {
  const asked = key;
  const failed = (await Promise.all([pids.length ? stop(pids) : [], ids.length ? stopContainers(ids) : []])).flat();
  if (key !== asked) return;
  error = failed.join(" ");
  render();
  draw();
}

async function stop(pids: number[]) {
  const s = paneSession(), asked = key;
  if (!s) return [];
  for (const pid of pids) stopping.add(pid);
  error = "";
  render();
  const failed: string[] = [];

  // Keep a large list from opening an unbounded number of mesh streams.
  for (let i = 0; i < pids.length; i += 4) {
    if (key !== asked) return failed;
    await Promise.all(pids.slice(i, i + 4).map(async pid => {
      try {
        await invoke("agent_stop_process", { machineId: s.machine_id, session: s.id, pid });
        if (key === asked) {
          list = list.filter(p => p.pid !== pid);
          lists.put(s.machine_id, topic("processes", s.id), { kind: "list", processes: list, containers });
        }
      } catch (e) { failed.push(readError(e)); }
      finally { if (key === asked) stopping.delete(pid); }
    }));
  }

  return failed;
}

/// Docker gives each container ten seconds to end, so they stop together.
async function stopContainers(ids: string[]) {
  const s = paneSession(), asked = key;
  if (!s) return [];
  for (const id of ids) stoppingContainers.add(id);
  error = "";
  render();
  const failed: string[] = [];

  await Promise.all(ids.map(async id => {
    try {
      await invoke("agent_stop_container", { machineId: s.machine_id, session: s.id, id });
      if (key === asked) {
        containers = containers.filter(c => c.id !== id);
        lists.put(s.machine_id, topic("processes", s.id), { kind: "list", processes: list, containers });
      }
    } catch (e) { failed.push(readError(e)); }
    finally { if (key === asked) stoppingContainers.delete(id); }
  }));

  return failed;
}
