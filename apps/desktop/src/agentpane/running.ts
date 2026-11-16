// Commands the session left running, following changes while its pane shows.
// `RunningPane` in running-view.tsx draws `runningModel`.
import { createStore } from "zustand";
import type { RunningActions, RunningModel } from "./running-view.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { runningTrees, type RunningProcess } from "./running-state.ts";
import { paneSession } from "./media.ts";
import { draw } from "./media-draw.ts";
import { viewKey } from "./view-memory.ts";
import { shellSize, shownDocks } from "./media-layout.ts";
import { AMBIENT_MS, latest, onShownChange, paneShown, STALE } from "../ui/active.ts";
import { invoke } from "../ui/tauri.ts";
import { AGENT_READ_WAIT_MS, type AgentTask } from "../agents/agents.ts";
import { readError } from "./read.ts";
import { isStarting } from "../agents/starting-state.ts";

interface ProcessesView { processes: RunningProcess[]; local: boolean }
let key = "";
let unfollow: (() => void) | undefined;
let list: RunningProcess[] = [];
let local = false;
let error = "";

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
const reads = latest();
const stopping = new Set<number>();
/// Rows whose whole command shows, by pid.
const whole = new Set<number>();

function syncSession() {
  const s = paneSession();
  const next = sessionKey(s);
  if (key === next) return;
  key = next;
  reads.cancel();
  unfollow?.();
  unfollow = s?.id && !s.external ? subscribeChanges(topic("processes", s.id), { deliver: "latest", receive: readRunning }, { machine: s.machine_id, when: visible, wait: AMBIENT_MS }) : undefined;
  list = [];
  local = false;
  error = "";
  runningModel.setState(null, true);
  stopping.clear();
  if (s?.id && !s.external) queueMicrotask(() => void readRunning());
}

export function runningCount() { syncSession(); return runningTrees(list).length + paneTasks().length; }

const visible = () => paneShown() && shellSize().width > 0 && shownDocks().length > 0;

window.addEventListener("agent-pane-shown", () => void readRunning());
onShownChange(shown => { if (shown) void readRunning(); }, true);

export async function readRunning() {
  syncSession();
  const s = paneSession(), asked = key, count = list.length;
  if (!s?.id || s.external || isStarting(s.id) || !visible()) return;

  try {
    const view = await reads.run(invoke<ProcessesView>("agent_processes", { machineId: s.machine_id, session: s.id }, AGENT_READ_WAIT_MS));
    syncSession();
    if (view === STALE || key !== asked || !visible()) return;
    list = view.processes;
    local = view.local;
    error = "";
  } catch (e) {
    if (key !== asked || !visible()) return;
    error = readError(e);
  }

  render();
  if (list.length !== count) draw();
}

const actions: RunningActions = {
  open: url => { invoke("open_url", { url }).catch(e => { error = readError(e); render(); }); },
  toggle: pid => {
    if (!whole.delete(pid)) whole.add(pid);
    render();
  },
  stop: pids => void stop(pids),
};

function render() {
  const session = paneSession();
  if (!session || key !== sessionKey(session)) return;
  runningModel.setState({ model: { processes: list, local, stopping: new Set(stopping), whole: new Set(whole), machine: session.machine_name, error }, actions }, true);
}

async function stop(pids: number[]) {
  const s = paneSession(), asked = key;
  if (!s) return;
  reads.cancel();
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
        if (key === asked) list = list.filter(p => p.pid !== pid);
      } catch (e) { failed.push(readError(e)); }
      finally { if (key === asked) stopping.delete(pid); }
    }));
  }

  if (key !== asked) return;
  reads.cancel();
  error = failed.join(" ");
  render();
  draw();
}
