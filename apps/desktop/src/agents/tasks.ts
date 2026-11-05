// The commands the open session's agent runs in the background: a strip
// above the field with each one's name and a Stop button.
import { $ } from "../bar/core.ts";
import { type AgentTask, type OpenAgent, AG, where } from "./agents.ts";
import { steerAgent } from "./messages.ts";
import { esc } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";

/// The server did not know the request, so it is not asked again.
let oldServer = false;
/// The tasks asked to stop, until they end.
const stopping = new Set<string>();

/// The open session's tasks; the last ones when a read fails.
export async function readTasks(o: OpenAgent) {
  const s = AG.page?.session;
  if (oldServer || !s || s.external || s.status === "ended") return [];
  try {
    return await invoke<AgentTask[]>("agent_tasks", where(o));
  } catch (e) {
    console.warn("agent tasks not read", e);
    return AG.tasks;
  }
}

/// A task that has not ended.
const runs = (t: AgentTask) => t.state === "running" || t.state === "paused";

/// Whether the log entry `at` is a call whose command still runs in the
/// background, though the call itself is done.
export const backgroundedEntry = (at: number) => AG.tasks.some(t => t.entry === at && runs(t));

/// `el` showing `html`, left alone when it does already: a spinner keeps
/// turning and a click is not lost to a button written again.
function show(el: HTMLElement | null, html: string) {
  if (el && el.dataset.shown !== html) { el.dataset.shown = html; el.innerHTML = html; }
}

export function drawTasks() {
  const live = AG.tasks.filter(runs);
  for (const id of stopping) if (!live.some(t => t.id === id)) stopping.delete(id);
  show($("#agtasks"), live.map(t => {
    const going = stopping.has(t.id);
    const what = t.description || t.name;
    const button = t.can_stop ? `<button class="dbtn quiet small" data-agtask="${esc(t.id)}"${going ? " disabled" : ""}>${going ? "Stopping" : "Stop"}</button>` : "";
    return `<div class="ag-task"><span class="dspin"></span><span class="nm" title="${esc(what)}">${esc(t.name || what)}</span>${button}</div>`;
  }).join(""));
}

/// Stop was clicked on task `id`.
export async function stopTask(id: string) {
  if (stopping.has(id)) return;
  stopping.add(id);
  drawTasks();
  // A refusal says why, and the next read shows the task as it is.
  if (!await steerAgent({ stop_task: { task: id } })) { stopping.delete(id); drawTasks(); }
}
