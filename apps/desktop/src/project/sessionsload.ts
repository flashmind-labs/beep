// Reading a project's sessions: kept per project, pushed by the machines, and listed for the rail.
import { retitled, type Retitled } from "../agents/conversation-state.ts";
import { moveStore } from "../agents/move-store.ts";
import { sessionVisible } from "../agents/closing.ts";
import { runningKey } from "../agents/running-state.ts";
import { Changes } from "../ui/pagecache.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { projectRunning, projectPastSessions, nextSessions, type SessionList, type SessionRow } from "./projects-state.ts";
import { projectRows } from "./projectrows.ts";
import { state, pages, keep, sessionsOf, peekSessions, sessionsDue } from "../center/state.ts";
import { currentLocation } from "../center/nav.ts";
import { railProject, shell } from "../center/window-shell.tsx";
import { redraw } from "../center/redraw.ts";

/// Whether the sessions of each project changed since they were read.
export const sessionChanges = new Changes();
// The lists themselves come as `session-list` events; a reconnect says they may be out of step.
listen("reconnected", () => sessionChanges.changed());

/// Project `name`'s running sessions in the order the rail lists them, newest started first,
/// which activity never reorders, then its past ones.
export function railSessions(name: string) {
  const p = projectRows()?.find(row => row.name === name);
  if (!p) return null;
  const sessions = sessionsOf(name), moves = [...moveStore.getState().records.values()];
  if ((sessions === undefined || sessionsDue.has(name)) && p.imported) loadSessions(name);
  const running = projectRunning(p, state.home?.running, sessions, moves).filter(sessionVisible);
  const past = projectPastSessions(sessions, running, moves).filter(sessionVisible);

  return { project: p, running: running.toSorted(railOrder), past };
}

/// The rail's order for running sessions: newest start first, so activity never reshuffles them.
const railOrder = (a: RailPlace, b: RailPlace) => b.started_at - a.started_at || runningKey(a).localeCompare(runningKey(b));
type RailPlace = { machine_id: string; id: string; started_at: number };

/// The running session after `s` in the rail's order, round to the first; null when none other runs.
export function nextRunning(name: string, s: { machine_id: string; id: string; started_at?: number }) {
  const running = railSessions(name)?.running.filter(row => runningKey(row) !== runningKey(s)) ?? [];
  const at = { machine_id: s.machine_id, id: s.id, started_at: s.started_at ?? 0 };
  return running.find(row => railOrder(at, row) < 0) ?? running[0] ?? null;
}

/// The projects whose sessions are being read, and those to read again on the
/// next draw.
export const readingSessions = new Set<string>();
const sessionReads = new Map<string, Promise<void>>();
export async function loadSessions(name: string): Promise<void> {
  const pending = sessionReads.get(name);
  if (pending) { await pending; if (sessionsDue.has(name)) return loadSessions(name); return; }
  const read = readSessions(name).finally(() => sessionReads.delete(name));
  sessionReads.set(name, read);
  return read;
}
/// The `session-list` events seen for each project, so a read that was out when one came is dropped.
const sessionPushes = new Map<string, number>();

/// The projects whose list native holds and pushes for this window.
const held = new Set<string>();
/// The forgets sent and not yet done, so a read after one cannot overtake it.
const forgetting = new Map<string, Promise<unknown>>();

/// Lets native drop the list of every project that neither the page nor the rail shows. It sends
/// no more pushes for it, so the project reads again when shown. An open page is never let go,
/// however long it stays open. A project whose read is out is let go when it ends: native takes
/// the two calls on different threads, so a forget sent now could be heard before the read.
export function releaseSessions(page: string | null) {
  const rail = railProject(shell.getState().side.rail);

  for (const name of [...held]) {
    if (name === page || name === rail || readingSessions.has(name)) continue;

    held.delete(name);
    sessionsDue.add(name);
    const sent = invoke("project_sessions_forget", { project: name }).catch(e => console.warn("project sessions forget:", e))
      .finally(() => { if (forgetting.get(name) === sent) forgetting.delete(name); });
    forgetting.set(name, sent);
  }
}
const shownPage = () => currentLocation()?.view === "projects" ? state.picked : null;
shell.subscribe(() => releaseSessions(shownPage()));

async function readSessions(name: string) {
  const pushes = sessionPushes.get(name) ?? 0;
  readingSessions.add(name);
  sessionsDue.delete(name);
  sessionChanges.read(name);
  let got: SessionRow[] | null = null, failed = false;

  try {
    await forgetting.get(name);
    held.add(name);
    // Null until every machine answered once: the list comes as a `session-list` event.
    got = await invoke<SessionRow[] | null>("project_sessions", { project: name });
  } catch (e) {
    failed = true;
    console.warn("project sessions:", e);
    sessionChanges.changed();
  } finally {
    readingSessions.delete(name);
    releaseSessions(shownPage());
  }

  // A list pushed while the read was out is newer than its answer.
  if ((sessionPushes.get(name) ?? 0) !== pushes) return;

  if (got || failed) showSessions(name, nextSessions(sessionsOf(name), got).list, got !== null);
  // The draw drops the Updating note. With no list yet it reads Reading sessions, and a draw
  // would ask again at once.
  else if (sessionsOf(name) !== undefined) drawSessions(name);
}

listen<SessionList>("session-list", ({ payload }) => {
  if (payload.list === "home") return;
  sessionPushes.set(payload.list.project, (sessionPushes.get(payload.list.project) ?? 0) + 1);
  showSessions(payload.list.project, payload.sessions, true);
});

/// Project `name`'s sessions are `list` now; `read` when the machines gave it, so it is kept.
function showSessions(name: string, list: SessionRow[], read: boolean) {
  if (read) keep("sessions", name, list);
  else pages.hold("sessions", name, list);
  drawSessions(name);
}

function drawSessions(name: string) {
  if (railProject(shell.getState().side.rail) === name) redraw("rail");
  if (currentLocation()?.view !== "projects" || state.picked !== name) return;
  redraw("projectsBody");
}

// The lists themselves come as `session-list` events, also for agents and moves.
// A title comes with its value, a cleared one with the agent's own: every kept list takes it.
listen<Retitled>("agent-title-changed", ({ payload }) => {
  for (const name of pages.ids("sessions")) {
    const next = retitled(peekSessions(name) ?? [], payload);
    if (next) showSessions(name, next, true);
  }
});
