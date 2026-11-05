// Moving the open session to another machine: the machine name in the head is
// a menu, a turn that runs waits (or is stopped), steps tick above the
// composer, and the lists say "laptop to studio". The records are in the
// mesh; the target says how far it is (`move-state.ts` holds the logic).

import { type AgentHost, type AgentRow, AG } from "./agents.ts";
import { type MoveProgress, type MoveRecord, type MoveTarget, canMove, going, moveRows, targetNote } from "./move-state.ts";
import { MV, moved, nameOf, refreshMoves } from "./move-store.ts";
import { $, windowHooks } from "../bar/core.ts";
import { placeOf } from "./place.ts";
import { esc } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import type { ProjectMachineRow } from "../project/projects-state.ts";

/// How often the target is asked while a move goes.
const POLL_MS = 1500;

const CHEV = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
const OK = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5 9-9"/></svg>';

const UI = { targets: [] as MoveTarget[], open: false, error: "", timer: 0 };

const here = (s: AgentRow) => MV.records.get(s.id);

/// Draws the machine menu in the head and the move above the composer.
export function drawMove() {
  const s = AG.page?.session;
  if (!s) return;
  drawHead(s);
  drawBox(s);
  const r = here(s);
  if (r && going(MV.progress.get(s.id)) && !UI.timer) UI.timer = window.setTimeout(poll, POLL_MS);
}

async function poll() {
  UI.timer = 0;
  await refreshMoves();
  drawMove();
}

function drawHead(s: AgentRow) {
  const el = $("#aghere");
  if (!el) return;
  const mid = going(MV.progress.get(s.id)) && here(s)?.from === s.machine_id;
  if (!canMove(s) || s.external || !windowHooks?.sessionProject?.(s)?.name) return;
  el.innerHTML = `<button class="mvb" id="agmoveb" ${mid ? "disabled" : ""} aria-haspopup="menu" aria-expanded="${UI.open}" title="${mid ? "Moving" : "Move this session to another machine"}">${esc(s.machine_name)}${mid ? "" : CHEV}</button>`;
  $("#agmoveb")?.addEventListener("click", e => { e.stopPropagation(); void toggle(s); });
  el.querySelector(".mvm")?.remove();
  if (UI.open) el.append(menu(s));
}

async function toggle(s: AgentRow) {
  UI.open = !UI.open;
  if (UI.open) {
    UI.targets = await targets(s);
    document.addEventListener("click", () => { UI.open = false; drawMove(); }, { once: true });
  }
  drawMove();
}

/// The machines of the mesh the session could go to, with what each has.
async function targets(s: AgentRow): Promise<MoveTarget[]> {
  const project = windowHooks?.sessionProject?.(s)?.name;
  if (!project) return [];
  const [rows, hosts] = await Promise.all([
    invoke<ProjectMachineRow[] | null>("project_machines", { project }).catch(() => null),
    invoke<AgentHost[]>("agent_hosts").catch(() => [] as AgentHost[]),
  ]);
  AG.hosts ??= hosts;
  for (const m of rows ?? []) MV.names.set(m.id, m.name);
  return (rows ?? []).filter(m => m.id !== s.machine_id).map(m => ({
    id: m.id,
    name: m.name,
    online: m.online,
    harnesses: hosts.find(h => h.machine_id === m.id)?.agents ?? [],
    checkout: !!m.folder,
  }));
}

function menu(s: AgentRow) {
  const box = document.createElement("div");
  box.className = "agwtm mvm";
  box.setAttribute("role", "menu");
  box.innerHTML = `<div class="mvh">Move to</div>` + UI.targets.map((t, i) => {
    const n = targetNote(t, s.agent);
    return `<button role="menuitem" data-mv="${i}" ${n.enabled ? "" : "disabled"}><b>${esc(t.name)}</b><small>${esc(n.note)}</small></button>`;
  }).join("");
  box.addEventListener("click", e => {
    const b = (e.target as HTMLElement).closest<HTMLElement>("[data-mv]");
    if (b && !b.hasAttribute("disabled")) void start(s, UI.targets[Number(b.dataset.mv)], false);
  });
  return box;
}

/// Asks the machine to take the session over.
async function start(s: AgentRow, to: MoveTarget, stop: boolean) {
  UI.open = false;
  UI.error = "";
  try {
    const project = windowHooks?.sessionProject?.(s)?.name ?? "";
    const r = await invoke<MoveRecord>("session_move", {
      machineId: s.machine_id, to: to.id, agent: s.agent, session: s.id, project, cwd: s.cwd,
      linked: !!placeOf(s)?.linked, mode: null, stop,
    });
    MV.records.set(r.session, r);
    MV.progress.set(r.session, { answer: "known", phase: "waiting", copied: 0, total: 0, files_left: 0, source_away: !to.online, reason: null });
  } catch (e) {
    UI.error = String(e);
  }
  moved();
  drawMove();
}

async function cancel(s: AgentRow) {
  try { await invoke("session_move_cancel", { session: s.id }); } catch (e) { UI.error = String(e); }
  MV.records.delete(s.id);
  MV.progress.delete(s.id);
  moved();
  drawMove();
}

function drawBox(s: AgentRow) {
  const box = $("#agmovebox");
  if (!box) return;
  const r = here(s), p = MV.progress.get(s.id);
  if (UI.error) {
    box.innerHTML = `<div class="mvs" role="alert"><div class="h"><span class="t">${esc(UI.error)}</span></div></div>`;
    return;
  }
  if (!r || r.from !== s.machine_id || !p || !going(p)) {
    box.innerHTML = p?.answer === "known" && p.phase === "failed" && r?.from === s.machine_id
      ? `<div class="mvs" role="status"><div class="h"><span class="t">${esc(`Move failed: ${p.reason ?? "the move did not finish"}. Still on ${s.machine_name}.`)}</span></div></div>`
      : "";
    return;
  }
  const to = nameOf(r.to);
  const turn = p.phase === "waiting" && !p.source_away && (s.status === "working" || s.status === "asking");
  if (turn) {
    box.innerHTML = `<div class="mvs bar" role="status"><span>Moving to ${esc(to)} after this turn</span><span class="sp"></span><button class="dbtn small" data-mvact="stop">Stop and move</button><button class="dbtn quiet small" data-mvact="cancel">Cancel</button></div>`;
  } else {
    const rows = moveRows(p, s.machine_name, to).map(row => `<div class="r ${row.state}"><span class="mk">${row.state === "done" ? OK : row.state === "run" ? '<span class="dspin"></span>' : ""}</span><span>${esc(row.text)}</span></div>`).join("");
    const cancelable = p.phase !== "resuming";
    box.innerHTML = `<div class="mvs" role="status"><div class="h"><b>Moving to ${esc(to)}</b><span class="sp"></span>${cancelable ? '<button class="dbtn quiet small" data-mvact="cancel">Cancel move</button>' : ""}</div>${rows}</div>`;
  }
  box.onclick = e => {
    const act = (e.target as HTMLElement).closest<HTMLElement>("[data-mvact]")?.dataset.mvact;
    if (act === "cancel") void cancel(s);
    if (act === "stop") void start(s, UI.targets.find(t => t.id === r.to) ?? { id: r.to, name: to, online: true, harnesses: [], checkout: true }, true);
  };
}
