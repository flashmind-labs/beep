// The Home this window read and the one it shows, and the local edits to both.
import type { AgentSession } from "../agents/agents-state.ts";
import { startedInHome, type HomeView } from "./home-state.ts";
import { heldHome, homeDataChanged, listsChanged, pages, projectLoaded, state } from "./state.ts";

/// The last answer, before telemetry, and the Home it made: a Home put in
/// place elsewhere is compared whole.
export const read: { last: HomeView | null; shown: HomeView | null } = { last: null, shown: null };

/// Every write to the window's Home, pins, groups, `where` and Mac comes
/// through here. A local edit goes to `read.last` too, so the next read still
/// compares against what Home shows and an unchanged Home draws nothing.
function editHome(edit: (home: HomeView) => HomeView): boolean {
  const held = heldHome();
  if (!held) return false;
  const kept = held === read.shown;
  const next = edit(held);
  if (next === held) return false;
  state.home = next;

  if (kept) {
    read.shown = state.home;
    if (read.last) read.last = edit(read.last);
  }
  return true;
}

/// Applies `agent-started` to the window's Home at once so `start:<uuid>` does
/// not linger behind Home's ambient throttle while `session-list` shows the real id.
export function takeStartedSession(start: string, session: AgentSession) {
  if (!editHome(home => startedInHome(home, start, session))) return;
  homeDataChanged.notify();
  projectLoaded.notify();
  listsChanged.notify();
}

/// A project the user removed: the topics push the new lists.
export function forgetProject(name: string) {
  state.top = state.top.filter(n => n !== name);
  editHome(home => home.projects ? { ...home, projects: home.projects.filter(p => p.name !== name) } : home);
  for (const section of ["where", "sessions", "gitsync"] as const) pages.drop(section, name);
}
