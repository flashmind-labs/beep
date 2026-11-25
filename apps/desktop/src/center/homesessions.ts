import { retitled, type Retitled } from "../agents/conversation-state.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { ambient } from "../ui/active.ts";
import type { SessionList, SessionRow } from "../project/projects-state.ts";
import { currentLocation } from "./nav.ts";
import { redraw } from "./redraw.ts";

// The Done today lane: the sessions of every project, ended ones included, which the
// running list drops; and every session, for the Assistant list. Read when they changed,
// while Home shows, seldom: they are not live.
const DONE_REREAD_MS = 5000;
let projectSessions: SessionRow[] = [];
let sessionsStale = true;
export const homeSessions = () => projectSessions;
const readSessions = ambient(() => {
  sessionsStale = false;
  // Null until every machine answered once: the list comes as a `session-list` event.
  const pushes = listsPushed;
  // A list pushed while the read is out is newer than its answer.
  return invoke<SessionRow[] | null>("projects_sessions").then(rows => { if (rows && pushes === listsPushed) showSessions(rows); }, e => { sessionsStale = true; console.warn("sessions:", e); });
}, DONE_REREAD_MS, () => currentLocation()?.view === "home");
let listsPushed = 0;
listen<SessionList>("session-list", ({ payload }) => { if (payload.list === "home") { listsPushed++; showSessions(payload.sessions); } });
function showSessions(rows: SessionRow[]) { projectSessions = rows; redraw("home"); }
// The list itself follows as `session-list` events; a reconnect says it may be out of step.
listen("reconnected", () => { sessionsStale = true; readSessions(); });
listen<Retitled>("agent-title-changed", ({ payload }) => {
  const next = retitled(projectSessions, payload);
  if (next) showSessions(next);
});

/// Reads them again when they went stale: a read failed, or a reconnect came since.
export function readHomeSessionsIfStale() {
  if (sessionsStale) readSessions(true);
}
