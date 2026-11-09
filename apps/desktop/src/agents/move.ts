import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { MoveStatus, type MoveView } from "./move-view.tsx";
import { subscribeChanges as subscribeTopics, topic } from "../ui/mesh-events.ts";
// Moving the open session to another machine: the machine name in the head is
// a menu, a turn that runs waits (or is stopped), steps tick above the
// composer, and the lists say "laptop to studio". The records are in the
// mesh; the target says how far it is (`move-state.ts` holds the logic).

import { type AgentHost, type AgentRow, AG } from "./agents.ts";
import { type MoveRecord, type MoveTarget, canMove, going, moveRows, targetNote } from "./move-state.ts";
import { MV, moved, nameOf, stateOf } from "./move-store.ts";
import { loadSessions } from "./sessions.ts";
import { $, S, windowHooks } from "../bar/core.ts";
import { placeOf } from "./place.ts";
import { esc } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { paneShown } from "../ui/active.ts";
import type { ProjectMachineRow } from "../project/projects-state.ts";

const CHEV = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';

const UI = { targets: [] as MoveTarget[], open: false, loading: false, error: "", errorSession: "" };
let warningTimer: ReturnType<typeof setTimeout> | null = null;

function warn(s: AgentRow, error: unknown) {
  if (warningTimer !== null) clearTimeout(warningTimer);
  UI.error = String(error);
  UI.errorSession = s.id;
  warningTimer = setTimeout(() => { UI.error = ""; warningTimer = null; drawMove(); }, 5000);
}

/// How long a machine may take to say whether it takes the move.
const PROBE_MS = 3000;

/// `ask`, or `fallback` when it fails or takes longer than `PROBE_MS`.
function probe<T>(ask: Promise<T>, fallback: T): Promise<T> {
  return Promise.race([ask.catch(() => fallback), new Promise<T>(done => setTimeout(() => done(fallback), PROBE_MS))]);
}

const here = (s: AgentRow) => MV.records.get(s.id);

/// Draws the machine menu in the head and the move above the composer.
export function drawMove() {
  const s = AG.page?.session;
  if (!s) return;
  drawHead(s);
  drawBox(s);
}

subscribeTopics([topic("moves"), topic("agents")], () => poll(), null, () => !!AG.open && S.view === "agent" && paneShown());

async function poll() {
  try { await loadSessions(); } catch (e) { console.warn("sessions not read after a move", e); }
  drawMove();
}

function drawHead(s: AgentRow) {
  const el = $("#aghere");
  if (!el) return;
  const mid = going(stateOf(s.id)) && here(s)?.from === s.machine_id;
  if (!canMove(s) || s.external || !windowHooks?.sessionProject?.(s)?.name) return;
  el.innerHTML = `<button class="mvb" id="agmoveb" ${mid ? "disabled" : ""} aria-haspopup="menu" aria-expanded="${UI.open}" title="${mid ? "Moving" : "Move this session to another machine"}">${esc(s.machine_name)}${mid ? "" : CHEV}</button>`;
  $("#agmoveb")?.addEventListener("click", e => { e.stopPropagation(); void toggle(s); });
  el.querySelector(".mvm")?.remove();
  if (UI.open) el.append(menu(s));
}

async function toggle(s: AgentRow) {
  UI.open = !UI.open;
  if (!UI.open) return drawMove();
  // The menu opens at once and fills in when the machines answered.
  UI.targets = [];
  UI.loading = true;
  document.addEventListener("click", () => { UI.open = false; drawMove(); }, { once: true });
  drawMove();
  const found = await targets(s);
  UI.loading = false;
  if (!UI.open) return;
  UI.targets = found;
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
  const others = (rows ?? []).filter(m => m.id !== s.machine_id);
  // A node from before moves would ignore the move, so it is not offered. One
  // that cannot be asked (offline) is offered: it answers when it returns.
  // An offline one is not asked: the dial would only time out.
  const takes = await Promise.all(others.map(m => m.online ? probe(invoke<boolean>("session_move_probe", { machineId: m.id }), true) : Promise.resolve(true)));
  // A session in a worktree also needs a node that makes the worktree, and a
  // Syncthing that does not leave it out.
  const tree = placeOf(s);
  const trees = await Promise.all(others.map(m => m.online && tree?.linked && tree.branch
    ? probe(invoke<"takes" | "ignored" | "update">("session_move_worktree_probe", { machineId: m.id, project, branch: tree.branch }), "takes" as const)
    : Promise.resolve("takes" as const)));
  return others.map((m, i) => ({
    id: m.id,
    name: m.name,
    online: m.online,
    harnesses: hosts.find(h => h.machine_id === m.id)?.agents ?? [],
    checkout: !!m.folder,
    old: !takes[i] || trees[i] === "update",
    ignored: trees[i] === "ignored",
  }));
}

function menu(s: AgentRow) {
  const box = document.createElement("div");
  box.className = "agwtm mvm";
  box.setAttribute("role", "menu");
  box.innerHTML = `<div class="mvh">Move to</div>` + (UI.loading ? `<div class="mvh">Checking machines…</div>` : "") + UI.targets.map((t, i) => {
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
    const tree = placeOf(s);
    const r = await invoke<MoveRecord>("session_move", {
      machineId: s.machine_id, to: to.id, agent: s.agent, session: s.id, project, cwd: s.cwd,
      worktreeFolder: tree?.linked ? tree.folder : null, worktreeBranch: tree?.linked ? tree.branch ?? "" : null,
      mode: null, stop,
    });
    MV.records.set(r.session, r);
    MV.progress.set(r.session, { answer: "known", phase: "waiting", copied: 0, total: 0, files_left: 0, source_away: !to.online, reason: null });
  } catch (e) {
    warn(s, e);
  }

  moved();
  drawMove();
}

async function cancel(s: AgentRow) {
  try { await invoke("session_move_cancel", { session: s.id }); } catch (e) { warn(s, e); }
  MV.records.delete(s.id);
  MV.progress.delete(s.id);
  moved();
  drawMove();
}

let moveRoot: { host: HTMLElement; root: Root; observer: MutationObserver } | null = null;
function disposeMove() {
  const current = moveRoot;
  moveRoot = null;
  if (current) { current.observer.disconnect(); current.root.unmount(); }
}
function drawBox(s: AgentRow) {
  const box = $("#agmovebox");
  if (!box) return;
  if (moveRoot?.host !== box) {
    disposeMove();
    const root = createRoot(box);
    const observer = new MutationObserver(() => { if (moveRoot?.host === box && !box.isConnected) disposeMove(); });
    observer.observe(document.body, { childList: true, subtree: true });
    moveRoot = { host: box, root, observer };
  }
  const r = here(s), p = stateOf(s.id);
  let view: MoveView = { kind: "none" };
  if (UI.error && UI.errorSession === s.id) view = { kind: "warning", text: UI.error };
  else if (!r || r.from !== s.machine_id || !p || !going(p)) {
    if (p?.answer === "known" && p.phase === "failed" && r?.from === s.machine_id) view = { kind: "failed", text: `Move failed: ${p.reason ?? "the move did not finish"}. Still on ${s.machine_name}.` };
  } else {
    const to = nameOf(r.to);
    if (p.phase === "waiting" && !p.source_away && (s.status === "working" || s.status === "asking")) view = { kind: "waiting", to };
    else view = { kind: "progress", to, cancelable: p.phase !== "resuming", rows: moveRows(p, s.machine_name, to, UI.targets.find(t => t.id === r.to)?.online ?? true) };
  }
  const root = moveRoot.root;
  flushSync(() => root.render(createElement(MoveStatus, {
    view, cancel: () => { void cancel(s); },
    stop: () => { if (r) void start(s, UI.targets.find(t => t.id === r.to) ?? { id: r.to, name: nameOf(r.to), online: true, harnesses: [], checkout: true }, true); },
  })));
}
