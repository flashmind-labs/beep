import { createElement, Fragment, type ReactElement } from "react";
import { createPortal, flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { MoveMenu, MoveStatus, type MoveView } from "./move-view.tsx";
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
import { invoke } from "../ui/tauri.ts";
import { paneShown } from "../ui/active.ts";
import type { ProjectMachineRow } from "../project/projects-state.ts";

const UI = { targets: [] as MoveTarget[], open: false, loading: false, error: "", errorSession: "" };
let warningTimer: ReturnType<typeof setTimeout> | null = null;
let menuRequest = 0, menuSession = "";

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
  const key = `${s.machine_id}\n${s.agent}\n${s.id}`;
  if (menuSession !== key) { menuSession = key; menuRequest++; UI.open = false; UI.loading = false; UI.targets = []; closeOutside(); }
  const head = $("#aghere"), box = $("#agmovebox");
  const eligible = !!head && canMove(s) && !s.external && !!windowHooks?.sessionProject?.(s)?.name;
  if (!eligible && !box && !moveRoot) return;
  const owner = ownerOf(head, box);
  if (eligible && head) {
    if (!owner.headView) head.replaceChildren();
    const mid = going(stateOf(s.id)) && here(s)?.from === s.machine_id;
    owner.headView = createElement(MoveMenu, {
      machine: s.machine_name, moving: mid, open: UI.open, loading: UI.loading,
      targets: UI.targets.map(target => ({ id: target.id, name: target.name, ...targetNote(target, s.agent) })),
      toggle: () => { void toggle(s); }, pick: (id: string) => { const target = UI.targets.find(t => t.id === id); if (target) void start(s, target, false); },
    });
  } else if (owner.headView) {
    owner.headView = null;
    drawRoot(owner);
    if (head) head.textContent = s.machine_name;
  }
  if (box) owner.boxView = moveStatus(s);
  drawRoot(owner);
}

subscribeTopics([topic("moves"), topic("agents")], () => poll(), null, () => !!AG.open && S.view === "agent" && paneShown());
async function poll() {
  try { await loadSessions(); } catch (e) { console.warn("sessions not read after a move", e); }
  drawMove();
}
const outside = () => { UI.open = false; menuRequest++; closeOutside(); drawMove(); };
function closeOutside() { document.removeEventListener("click", outside); }
async function toggle(s: AgentRow) {
  UI.open = !UI.open;
  const request = ++menuRequest;
  closeOutside();
  if (!UI.open) return drawMove();
  UI.targets = []; UI.loading = true;
  document.addEventListener("click", outside);
  drawMove();
  const found = await targets(s);
  if (request !== menuRequest || !UI.open || AG.page?.session.id !== s.id || AG.page.session.machine_id !== s.machine_id) return;
  UI.loading = false; UI.targets = found;
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

/// Asks the machine to take the session over.
async function start(s: AgentRow, to: MoveTarget, stop: boolean) {
  UI.open = false; menuRequest++; closeOutside();
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

interface MoveOwner { head: HTMLElement | null; box: HTMLElement | null; root: DocumentView; headView: ReactElement | null; boxView: ReactElement | null; observer: MutationObserver }
let moveRoot: MoveOwner | null = null;
function disposeMove() {
  const current = moveRoot;
  moveRoot = null; closeOutside(); menuRequest++; UI.open = false;
  if (current) { current.observer.disconnect(); current.root.unmount(); }
}
function ownerOf(head: HTMLElement | null, box: HTMLElement | null): MoveOwner {
  if (moveRoot && moveRoot.head === head && moveRoot.box === box) return moveRoot;
  disposeMove();
  const root = createDocumentView(document.createElement("div"));
  const observer = new MutationObserver(() => { if (moveRoot && moveRoot.head === head && moveRoot.box === box && !(head?.isConnected || box?.isConnected)) disposeMove(); });
  observer.observe(document.body, { childList: true, subtree: true });
  moveRoot = { head, box, root, headView: null, boxView: null, observer };
  return moveRoot;
}
function drawRoot(owner: MoveOwner) {
  flushSync(() => owner.root.render(createElement(Fragment, null, owner.head && owner.headView && createPortal(owner.headView, owner.head), owner.box && owner.boxView && createPortal(owner.boxView, owner.box))));
}
function moveStatus(s: AgentRow) {
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
  return createElement(MoveStatus, {
    view, cancel: () => { void cancel(s); },
    stop: () => { if (r) void start(s, UI.targets.find(t => t.id === r.to) ?? { id: r.to, name: nameOf(r.to), online: true, harnesses: [], checkout: true }, true); },
  });
}
