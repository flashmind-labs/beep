// Commands the session left running, as the node pushes them while its pane shows.
// `RunningPane` in running-view.tsx draws `runningModel`.
import { createStore } from "zustand";
import type { RunningActions, RunningModel } from "./running-view.tsx";
import { topic } from "../ui/mesh-events.ts";
import { TopicValues } from "../ui/topic-values-state.ts";
import { parseProcesses, runningTrees, type ProcessesValue, type RunningProcess } from "./running-state.ts";
import { paneSession } from "./media.ts";
import { draw } from "./media-draw.ts";
import { viewKey } from "./view-memory.ts";
import { shellSize, shownDocks } from "./media-layout.ts";
import { paneShown } from "../ui/active.ts";
import { invoke } from "../ui/tauri.ts";
import type { AgentTask } from "../agents/agents.ts";
import { readError } from "./read.ts";

let key = "";
let unfollow: (() => void) | undefined;
/// The lists of the sessions seen lately, so one shows at once when its conversation opens again.
const lists = new TopicValues<ProcessesValue>(parseProcesses, 32);
let list: RunningProcess[] = [];
let error = "";
/// Set while `syncSession` takes the kept list, which its caller draws.
let syncing = false;

/// The background commands the session's agent runs, listed above its
/// processes with their Stop; the log shows only their count.
export interface BackgroundTasks { session: string; tasks: readonly AgentTask[]; stopping: ReadonlySet<string>; stop(id: string): void }
export const backgroundTasks = createStore<BackgroundTasks>(() => ({ session: "", tasks: [], stopping: new Set(), stop() {} }));

/// The key of session `s` in `BackgroundTasks.session`.
export const sessionKey = (s: { machine_id: string; id: string } | null | undefined) => s ? viewKey(s.machine_id, s.id) : "";

/// The background commands of the session the pane shows.
export function paneTasks(state = backgroundTasks.getState()) {
  return state.session && state.session === sessionKey(paneSession()) ? state.tasks : [];
}

/// What the view draws, for the session the pane shows.
export const runningModel = createStore<{ model: RunningModel; actions: RunningActions } | null>(() => null);
const stopping = new Set<number>();
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
  error = "";
  runningModel.setState(null, true);
  stopping.clear();
  if (!s?.id || s.external) return;

  // The kept list is applied before this returns, with no draw: the caller draws.
  syncing = true;
  unfollow = lists.watch(s.machine_id, topic("processes", s.id), update, visible);
  syncing = false;
  if (list.length || error) queueMicrotask(() => { if (key === next) { render(); draw(); } });
}

/// Takes a pushed list. A node that could not list keeps the last one on screen with the reason.
function update(value: ProcessesValue) {
  const count = list.length;

  if (value.kind === "list") {
    list = value.processes;
    error = "";
  } else {
    error = value.message;
  }

  if (syncing || !visible()) return;
  render();
  if (list.length !== count) draw();
}

export function runningCount() { syncSession(); return runningTrees(list).length + paneTasks().length; }

const visible = () => paneShown() && shellSize().width > 0 && shownDocks().length > 0;

// The list changed while the pane was hidden: it draws now.
window.addEventListener("agent-pane-shown", () => { syncSession(); render(); });

const actions: RunningActions = {
  open: port => void openPort(port),
  toggle: pid => {
    if (!whole.delete(pid)) whole.add(pid);
    render();
  },
  stop: pids => void stop(pids),
};

function render() {
  const session = paneSession();
  if (!session || key !== sessionKey(session)) return;
  runningModel.setState({ model: { processes: list, stopping: new Set(stopping), whole: new Set(whole), machine: session.machine_name, error }, actions }, true);
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

async function stop(pids: number[]) {
  const s = paneSession(), asked = key;
  if (!s) return;
  for (const pid of pids) stopping.add(pid);
  error = "";
  render();
  const failed: string[] = [];

  // Keep a large list from opening an unbounded number of mesh streams.
  for (let i = 0; i < pids.length; i += 4) {
    if (key !== asked) return;
    await Promise.all(pids.slice(i, i + 4).map(async pid => {
      try {
        await invoke("agent_stop_process", { machineId: s.machine_id, session: s.id, pid });
        if (key === asked) {
          list = list.filter(p => p.pid !== pid);
          lists.put(s.machine_id, topic("processes", s.id), { kind: "list", processes: list });
        }
      } catch (e) { failed.push(readError(e)); }
      finally { if (key === asked) stopping.delete(pid); }
    }));
  }

  if (key !== asked) return;
  error = failed.join(" ");
  render();
  draw();
}
