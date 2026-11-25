import { moveViews, type MoveStatus, type MoveView } from "./move-view.tsx";
// Moving the open session to another machine: the machine name in the head is
// a menu, a turn that runs waits (or is stopped), steps tick above the
// composer, and the lists say "laptop to studio". The records are in the
// mesh; the target says how far it is (`move-state.ts` holds the logic).

import { type AgentHost, type AgentRow, AG, ensureAgentHosts } from "./agents.ts";
import { agentKey } from "./conversation-state.ts";
import { type MoveProgress, type MoveRecord, type MoveTarget, canMove, chatSessions, going, movePct, moveRows, targetNote } from "./move-state.ts";
import { isAssistant } from "./assistant-state.ts";
import { isStarting } from "./starting-state.ts";
import { moveStore, nameOf, onMoves, refreshMoves, stateOf } from "./move-store.ts";
import { sessions, sessionsRead } from "./sessions.ts";
import { S, windowHooks } from "../bar/core.ts";
import { placeOf } from "./place.ts";
import { invoke } from "../ui/tauri.ts";
import { ambient, latest, onShownChange, paneShown, SLOW_MS, STALE } from "../ui/active.ts";
import type { ProjectMachineRow } from "../project/projects-state.ts";

/// `agents`: the harnesses the session uses, with a chat's threads.
const UI = { targets: [] as MoveTarget[], agents: [] as string[], open: false, loading: false, error: "", errorSession: "" };
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

/// An assistant chat, which moves with its threads and its folder.
const isChat = (s: AgentRow) => isAssistant({ assistant: s.assistant ?? false, reviewer: s.reviewer });

/// The sessions that move with chat `s`, as the lists have them.
function chatOf(s: AgentRow) {
  const row = sessions.find(r => r.machine_id === s.machine_id && r.id === s.id);
  return row ? chatSessions(sessions, row) : { refused: "the chat is not listed yet" };
}

/// When the window first saw each session's files coming, for their speed.
const filesSince = new Map<string, number>();

/// A move's first answer, before the target says anything.
const waiting = (sourceAway: boolean): MoveProgress => ({
  answer: "known", phase: "waiting", copied: 0, total: 0, source_away: sourceAway, reason: null, worktree: null, worktree_at: 0, worktrees: 0, edits_left: false, files_copied: 0, files_total: 0,
});

/// Draws the machine menu in the head and the move above the composer.
export function drawMove() {
  const s = AG.page?.session;
  if (!s) return;
  const key = agentKey(s);
  if (menuSession !== key) { menuSession = key; menuRead.cancel(); UI.open = false; UI.loading = false; UI.targets = []; closeOutside(); }
  // A chat on a node from before chat moves is refused when asked, in words.
  const placed = isChat(s) || !!windowHooks?.sessionProject?.(s)?.name;
  const eligible = canMove(s) && !s.external && !isStarting(s.id) && placed;
  const mid = going(stateOf(s.id)) && here(s)?.from === s.machine_id;
  moveViews.setState({
    key,
    menu: eligible ? {
      machine: s.machine_name, moving: mid, open: UI.open, loading: UI.loading,
      targets: UI.targets.map(target => ({ id: target.id, name: target.name, ...targetNote(target, UI.agents) })),
      toggle: () => { void toggle(s); }, pick: (id: string) => { const target = UI.targets.find(t => t.id === id); if (target) void start(s, target, false); },
    } : null,
    status: moveStatus(s),
  });
}

// The open session follows its move once its new machine lists it. Only while it has one
// do the session lists matter: any machine's session changes would read every machine.
const viewing = () => !!AG.open && S.view === "agent" && paneShown();
const following = () => {
  const s = AG.page?.session, phase = s ? stateOf(s.id)?.phase : null;
  return !!s && here(s)?.from === s.machine_id && phase !== "failed" && phase !== "cancelled";
};
// How far a move is, the target says, not the mesh: asked again while one shows.
const readProgress = ambient(async () => { await refreshMoves(); drawMove(); }, SLOW_MS, () => viewing());
// The lists arrive by the home push: a move or a session change draws the move again while one shows.
sessionsRead.add(() => { if (viewing() && following()) drawMove(); });
onMoves(() => { if (viewing()) drawMove(); });
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

  const chat = isChat(s) ? chatOf(s) : null;

  if (chat && "refused" in chat) {
    UI.open = false;
    warn(s, `This chat cannot move: ${chat.refused}.`);
    return drawMove();
  }

  UI.targets = []; UI.agents = chat?.agents ?? [s.agent]; UI.loading = true;
  document.addEventListener("click", outside);
  drawMove();
  const found = await menuRead.run(chat ? chatTargets(s) : targets(s));
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
    ensureAgentHosts().catch(() => [] as AgentHost[]),
  ]);
  const others = (rows ?? []).filter(m => m.id !== s.machine_id);
  // A node from before moves would ignore the move, so it is not offered. One
  // that cannot be asked (offline) is offered: it answers when it returns.
  // An offline one is not asked: the dial would only time out.
  const takes = await Promise.all(others.map(m => m.online ? probe(invoke<boolean>("session_move_probe", { machineId: m.id }), true) : Promise.resolve(true)));
  return others.map((m, i) => ({
    id: m.id,
    name: m.name,
    online: m.online,
    harnesses: hosts.find(h => h.machine_id === m.id)?.agents ?? [],
    checkout: !!m.folder,
    old: !takes[i],
  }));
}

/// The machines a chat could go to: those online whose node moves chats.
async function chatTargets(s: AgentRow): Promise<MoveTarget[]> {
  const hosts = await ensureAgentHosts().catch(() => [] as AgentHost[]);
  return hosts.filter(h => h.machine_id !== s.machine_id).map(h => ({
    id: h.machine_id, name: h.machine_name, online: true, harnesses: h.agents, checkout: true, old: !h.moves_chats,
  }));
}

/// Asks the machine to take the session over, or the chat with its threads.
async function start(s: AgentRow, to: MoveTarget, stop: boolean) {
  UI.open = false; menuRead.cancel(); closeOutside();
  UI.error = "";

  try {
    const moved = isChat(s) ? await moveChat(s, to, stop) : [await moveSession(s, to, stop)];
    const records = new Map(moveStore.getState().records), progress = new Map(moveStore.getState().progress);
    for (const r of moved) { records.set(r.session, r); progress.set(r.session, waiting(!to.online)); }
    moveStore.setState({ records, progress });
  } catch (e) {
    warn(s, e);
  }

  drawMove();
}

function moveSession(s: AgentRow, to: MoveTarget, stop: boolean) {
  const project = windowHooks?.sessionProject?.(s)?.name ?? "";
  // A session that worked in a worktree resumes in the project folder.
  const tree = placeOf(s);
  return invoke<MoveRecord>("session_move", {
    machineId: s.machine_id, to: to.id, agent: s.agent, session: s.id, project, cwd: tree?.linked ? tree.main : s.cwd,
    mode: null, model: s.model ?? null, stop,
  });
}

function moveChat(s: AgentRow, to: MoveTarget, stop: boolean) {
  const chat = chatOf(s);
  if ("refused" in chat) throw new Error(`This chat cannot move: ${chat.refused}.`);
  const asked = chat.sessions.map(r => ({ agent: r.agent, session: r.id, cwd: r.cwd, model: r.model ?? null, ended: r.status === "ended" }));
  return invoke<MoveRecord[]>("chat_move", { machineId: s.machine_id, to: to.id, sessions: asked, stop });
}

/// Takes the move back: a chat's with its threads', which were asked at once.
async function cancel(s: AgentRow) {
  const r = here(s), { records, progress } = moveStore.getState();
  const ids = r ? [...records.values()].filter(o => o.at === r.at && o.from === r.from).map(o => o.session) : [s.id];
  const results = await Promise.allSettled(ids.map(session => invoke("session_move_cancel", { session })));
  const failed = results.find(result => result.status === "rejected");
  if (failed) warn(s, failed.reason);
  const left = new Map(records), told = new Map(progress);
  for (const id of ids) { left.delete(id); told.delete(id); }
  moveStore.setState({ records: left, progress: told });
  drawMove();
}

function moveStatus(s: AgentRow): Parameters<typeof MoveStatus>[0] {
  const r = here(s), p = stateOf(s.id);
  let view: MoveView = { kind: "none" };
  if (UI.error && UI.errorSession === s.id) view = { kind: "warning", text: UI.error };
  else if (!r || r.from !== s.machine_id || !p || !going(p)) {
    if (p?.answer === "known" && p.phase === "failed" && r?.from === s.machine_id) view = { kind: "failed", text: `Move failed: ${p.reason ?? "the move did not finish"}. Still on ${s.machine_name}.` };
  } else {
    const to = nameOf(r.to), now = Date.now();
    if (p.phase === "files") filesSince.set(s.id, filesSince.get(s.id) ?? now); else filesSince.delete(s.id);
    const since = filesSince.get(s.id);
    readProgress();
    if (p.phase === "waiting" && !p.source_away && (s.status === "working" || s.status === "asking")) view = { kind: "waiting", to };
    else view = { kind: "progress", to, cancelable: p.phase !== "resuming", rows: moveRows(p, s.machine_name, to, UI.targets.find(t => t.id === r.to)?.online ?? true, isChat(s) ? "chat" : "session", since === undefined ? null : { since, now }), pct: movePct(p) };
  }
  return {
    view, cancel: () => { void cancel(s); },
    stop: () => { if (r) void start(s, UI.targets.find(t => t.id === r.to) ?? { id: r.to, name: nameOf(r.to), online: true, harnesses: [], checkout: true }, true); },
  };
}
