import { moveViews, type MoveStatus, type MoveView } from "./move-view.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
// Moving the open session to another machine: the machine name in the head is
// a menu, a turn that runs waits (or is stopped), steps tick above the
// composer, and the lists say "laptop to studio". The records are in the
// mesh; the target says how far it is (`move-state.ts` holds the logic).

import { type AgentHost, type AgentRow, AG } from "./agents.ts";
import { type MoveRecord, type MoveTarget, canMove, going, moveRows, targetNote } from "./move-state.ts";
import { machineNames, moveStore, nameOf, stateOf } from "./move-store.ts";
import { loadSessions } from "./sessions.ts";
import { S, windowHooks } from "../bar/core.ts";
import { placeOf } from "./place.ts";
import { invoke } from "../ui/tauri.ts";
import { latest, onShownChange, paneShown, STALE } from "../ui/active.ts";
import type { ProjectMachineRow } from "../project/projects-state.ts";

const UI = { targets: [] as MoveTarget[], open: false, loading: false, error: "", errorSession: "" };
let warningTimer: ReturnType<typeof setTimeout> | null = null;
/// The menu's read of the machines; closing the menu drops it.
const menuRead = latest();
let menuSession = "";

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

const here = (s: AgentRow) => moveStore.getState().records.get(s.id);

/// The session a move view belongs to.
export const moveKey = (s: AgentRow) => `${s.machine_id}\n${s.agent}\n${s.id}`;

/// Draws the machine menu in the head and the move above the composer.
export function drawMove() {
  const s = AG.page?.session;
  if (!s) return;
  const key = moveKey(s);
  if (menuSession !== key) { menuSession = key; menuRead.cancel(); UI.open = false; UI.loading = false; UI.targets = []; closeOutside(); }
  const eligible = canMove(s) && !s.external && !!windowHooks?.sessionProject?.(s)?.name;
  const mid = going(stateOf(s.id)) && here(s)?.from === s.machine_id;
  moveViews.setState({
    key,
    menu: eligible ? {
      machine: s.machine_name, moving: mid, open: UI.open, loading: UI.loading,
      targets: UI.targets.map(target => ({ id: target.id, name: target.name, ...targetNote(target, s.agent) })),
      toggle: () => { void toggle(s); }, pick: (id: string) => { const target = UI.targets.find(t => t.id === id); if (target) void start(s, target, false); },
    } : null,
    status: moveStatus(s),
  });
}

subscribeChanges([topic("moves"), topic("agents")], { deliver: "latest", receive: () => poll() }, { when: () => !!AG.open && S.view === "agent" && paneShown() });
async function poll() {
  try { await loadSessions(); } catch (e) { console.warn("sessions not read after a move", e); }
  drawMove();
}
const outside = () => { UI.open = false; menuRead.cancel(); closeOutside(); drawMove(); };

/// Closes the machine menu; false when it was not open.
export function closeMoveMenu() {
  if (!UI.open) return false;
  outside();
  return true;
}
// The center keeps a session's page while another view shows: hiding it closes the menu.
onShownChange(shown => { if (!shown) closeMoveMenu(); }, true);

/// Closes the menu and forgets the views, as when the session leaves the page.
export function closeMove() {
  // The click listener is there only while the menu is open.
  if (UI.open) closeOutside();
  UI.open = false; UI.loading = false; UI.targets = [];
  menuRead.cancel();
  menuSession = "";
  moveViews.setState({ key: "", menu: null, status: null }, true);
}

function closeOutside() { document.removeEventListener("click", outside); }
async function toggle(s: AgentRow) {
  UI.open = !UI.open;
  closeOutside();

  if (!UI.open) {
    menuRead.cancel();
    return drawMove();
  }

  UI.targets = []; UI.loading = true;
  document.addEventListener("click", outside);
  drawMove();
  const found = await menuRead.run(targets(s));
  if (found === STALE || !UI.open || AG.page?.session.id !== s.id || AG.page.session.machine_id !== s.machine_id) return;
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
  for (const m of rows ?? []) machineNames.set(m.id, m.name);
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
  UI.open = false; menuRead.cancel(); closeOutside();
  UI.error = "";

  try {
    const project = windowHooks?.sessionProject?.(s)?.name ?? "";
    const tree = placeOf(s);
    const r = await invoke<MoveRecord>("session_move", {
      machineId: s.machine_id, to: to.id, agent: s.agent, session: s.id, project, cwd: s.cwd,
      worktreeFolder: tree?.linked ? tree.folder : null, worktreeBranch: tree?.linked ? tree.branch ?? "" : null,
      mode: null, stop,
    });
    const { records, progress } = moveStore.getState();
    moveStore.setState({
      records: new Map(records).set(r.session, r),
      progress: new Map(progress).set(r.session, { answer: "known", phase: "waiting", copied: 0, total: 0, files_left: 0, source_away: !to.online, reason: null }),
    });
  } catch (e) {
    warn(s, e);
  }

  drawMove();
}

async function cancel(s: AgentRow) {
  try { await invoke("session_move_cancel", { session: s.id }); } catch (e) { warn(s, e); }
  const records = new Map(moveStore.getState().records), progress = new Map(moveStore.getState().progress);
  records.delete(s.id);
  progress.delete(s.id);
  moveStore.setState({ records, progress });
  drawMove();
}

function moveStatus(s: AgentRow): Parameters<typeof MoveStatus>[0] {
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
  return {
    view, cancel: () => { void cancel(s); },
    stop: () => { if (r) void start(s, UI.targets.find(t => t.id === r.to) ?? { id: r.to, name: nameOf(r.to), online: true, harnesses: [], checkout: true }, true); },
  };
}
