// Commands the session left running, refreshed with its visible-session poll.
import { type RunningProcess, runningRow } from "./running-state.ts";
import { draw, paneOf, paneSession, shell } from "./media.ts";
import { DOCKS } from "./media-state.ts";
import { paneShown } from "../ui/active.ts";
import { h } from "../ui/dom.ts";
import { esc } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { readError } from "./read.ts";

interface ProcessesView { processes: RunningProcess[]; local: boolean }
let key = "";
let list: RunningProcess[] = [];
let local = false;
let el: HTMLElement | null = null;
let drawn = "";
let error = "";
const stopping = new Set<number>();

function syncSession() {
  const s = paneSession();
  const next = s ? `${s.machine_id}\n${s.id}` : "";
  if (key === next) return;
  key = next;
  list = [];
  local = false;
  error = drawn = "";
  stopping.clear();
  el?.remove();
  el = null;
}

export function runningCount() { syncSession(); return list.length; }

const visible = () => paneShown() && !!shell()?.clientWidth && DOCKS.some(d => paneOf(d)?.hidden === false);

export async function readRunning() {
  syncSession();
  const s = paneSession(), asked = key, count = list.length;
  if (!s || s.external || !visible()) return;
  try {
    const view = await invoke<ProcessesView>("agent_processes", { machineId: s.machine_id, session: s.id });
    syncSession();
    if (key !== asked || !visible()) return;
    list = view.processes;
    local = view.local;
    error = "";
  } catch (e) {
    if (key !== asked) return;
    error = readError(e);
  }
  render();
  if (list.length !== count) draw();
}

export function runningView() {
  syncSession();
  if (!list.length) { el?.remove(); return null; }
  if (!el) {
    el = h('<div class="agpview running"><div class="agrunninglist"></div><div class="agpacts"><span class="meta"></span><span class="sp"></span><button class="dbtn warn" data-stop-all>Stop all</button></div><div class="derr agrunningerror" role="status" hidden></div></div>');
    el.addEventListener("click", e => {
      const target = e.target as HTMLElement;
      const link = target.closest<HTMLAnchorElement>("a[data-port]");
      if (link) {
        e.preventDefault();
        e.stopPropagation();
        invoke("open_url", { url: link.href }).catch(e => { error = readError(e); render(); });
        return;
      }
      const button = target.closest<HTMLButtonElement>("button[data-pid], button[data-stop-all]");
      if (button && !button.disabled) void stop(button.hasAttribute("data-stop-all") ? list.map(p => p.pid) : [+button.dataset.pid!]);
    });
  }
  render();
  return el;
}

function render() {
  if (!el) return;
  const sig = JSON.stringify([list, local, [...stopping]]);
  if (sig !== drawn) {
    drawn = sig;
    el.querySelector(".agrunninglist")!.innerHTML = list.map(p => {
      const row = runningRow(p, local);
      const ports = row.ports.map(port => port.url
        ? `<a data-port href="${port.url}">${port.label}</a>` : `<span>${port.label}</span>`).join("");
      return `<div class="agart agrunningrow"><span class="t"><b title="${esc(row.command)}">${esc(row.command)}</b><small>${esc(row.detail)}</small></span><span class="r">${ports}<button class="dbtn" data-pid="${p.pid}"${stopping.has(p.pid) ? " disabled" : ""}>${stopping.has(p.pid) ? "Stopping…" : "Stop"}</button></span></div>`;
    }).join("");
  }
  el.querySelector(".meta")!.textContent = `${paneSession()?.machine_name ?? "The machine"} · closing the session stops these`;
  el.querySelector<HTMLButtonElement>("[data-stop-all]")!.disabled = stopping.size > 0;
  const note = el.querySelector<HTMLElement>("[role=status]")!;
  note.hidden = !error;
  note.textContent = error;
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
        if (key === asked) list = list.filter(p => p.pid !== pid);
      } catch (e) { failed.push(readError(e)); }
      finally { if (key === asked) stopping.delete(pid); }
    }));
  }
  if (key !== asked) return;
  error = failed.join(" ");
  render();
  draw();
}
