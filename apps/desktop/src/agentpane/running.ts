import { createElement } from "react";
import { removeSessionPortal, setSessionPortal } from "../agents/session-frame.tsx";
import { RunningView, type RunningActions } from "./running-view.tsx";
// Commands the session left running, following changes while its pane shows.
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import type { RunningProcess } from "./running-state.ts";
import { draw, paneOf, paneSession, shell } from "./media.ts";
import { DOCKS } from "./media-state.ts";
import { AMBIENT_MS, onShownChange, paneShown } from "../ui/active.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";
import { AGENT_READ_WAIT_MS } from "../agents/agents.ts";
import { readError } from "./read.ts";

interface ProcessesView { processes: RunningProcess[]; local: boolean }
let key = "";
let unfollow: (() => void) | undefined;
let list: RunningProcess[] = [];
let local = false;
let el: HTMLElement | null = null;
let error = "";
let readVersion = 0;
const stopping = new Set<number>();
/// Rows whose whole command shows, by pid.
const whole = new Set<number>();

function syncSession() {
  const s = paneSession();
  const next = s ? `${s.machine_id}\n${s.id}` : "";
  if (key === next) return;
  key = next;
  readVersion++;
  unfollow?.();
  unfollow = s?.id && !s.external ? subscribeChanges(topic("processes", s.id), readRunning, s.machine_id, visible, AMBIENT_MS) : undefined;
  list = [];
  local = false;
  error = "";
  if (el) removeSessionPortal(el);
  stopping.clear();
  el?.remove();
  el = null;
  if (s?.id && !s.external) queueMicrotask(() => void readRunning());
}

export function runningCount() { syncSession(); return list.length; }

const visible = () => paneShown() && !!shell()?.clientWidth && DOCKS.some(d => paneOf(d)?.hidden === false);

window.addEventListener("agent-pane-shown", () => void readRunning());
onShownChange(shown => { if (shown) void readRunning(); }, true);

export async function readRunning() {
  syncSession();
  const s = paneSession(), asked = key, count = list.length;
  if (!s?.id || s.external || !visible()) return;
  const version = ++readVersion;

  try {
    const view = await invoke<ProcessesView>("agent_processes", { machineId: s.machine_id, session: s.id }, AGENT_READ_WAIT_MS);
    syncSession();
    if (key !== asked || version !== readVersion || !visible()) return;
    list = view.processes;
    local = view.local;
    error = "";
  } catch (e) {
    if (key !== asked || version !== readVersion || !visible()) return;
    error = readError(e);
  }

  render();
  if (list.length !== count) draw();
}

/// The Running view, made once per session. Its host outlives a rebuilt
/// session frame, so each call renders it again.
export function runningView() {
  syncSession();
  if (!list.length) { el?.remove(); return null; }
  el ??= h('<div class="agpview running"></div>');
  render();
  return el;
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
  if (!el || !session || key !== `${session.machine_id}\n${session.id}`) return;
  setSessionPortal(el, createElement(RunningView, { model: { processes: list, local, stopping, whole, machine: session.machine_name, error }, actions }));
}

async function stop(pids: number[]) {
  const s = paneSession(), asked = key;
  if (!s) return;
  readVersion++;
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
  readVersion++;
  error = failed.join(" ");
  render();
  draw();
}
