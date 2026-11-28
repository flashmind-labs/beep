// Which tile of a project's page shows, and the shell that keeps the page's navigation and Overview
// mounted between tiles.
import { type ReactNode } from "react";
import { createPortal } from "react-dom";
import { invoke } from "../../ui/tauri.ts";
import { removeWindowPart, showWindowParts } from "../../parts/center/window-root.tsx";
import { retainedHost } from "./page-view.tsx";
import { makeGitHistory } from "../review/history.tsx";
import type { BranchSessionRow } from "../../stores/review/history-state.ts";
import { sessions } from "../../stores/agents/sessions.ts";
import { placeOf } from "../../stores/agents/place.ts";
import { sessionVisible, shownStatus } from "../../stores/agents/closing.ts";
import { showChanges } from "../../parts/session/changes.ts";
import type { SessionRow } from "../../stores/project/projects-state.ts";
import { sessionsOf, projectLoaded } from "../../stores/center/state.ts";
import { boxOf, go } from "../../parts/center/nav.ts";
import { terms } from "../../parts/project/stores.ts";
import { runs } from "../../parts/project/sections.ts";
import { pa, projectChanged } from "../../stores/project/barstate.ts";
import { rememberGitRow } from "./loads.tsx";

/// The project's navigation and Overview stay mounted in their own elements while
/// other tiles show, so their rows, focus and scroll survive a visit to Git or Running.
interface ProjectShell {
  project: string;
  navigation: HTMLElement;
  overview: HTMLElement;
  navView: ReactNode;
  overviewView: ReactNode;
  overviewScroll: Map<string, number>;
}
let shell: ProjectShell | null = null;

export function renderProjectShell(current: ProjectShell) {
  showWindowParts([{ part: "project-shell", host: null, identity: current.project, content: <>
    {createPortal(current.navView, current.navigation, "navigation")}{createPortal(current.overviewView, current.overview, "overview")}</> }]);
}
export function disposeProjectShell() {
  if (!shell) return;
  removeWindowPart("project-shell");
  shell = null;
}
export function projectShell(project: string) {
  if (shell?.project === project) return shell;
  disposeProjectShell();
  shell = { project, navigation: retainedHost("pj-list"), overview: retainedHost("pj-ov"), navView: null, overviewView: null, overviewScroll: new Map() };
  return shell;
}

/// Picks item `key` of the split's list, or the first one with null, and
/// stops the polling of every other item. With `draw` false the caller
/// draws.
export function openTile(key: string | null, draw = true) {
  const current = shell;
  const scrolls = ["#sec-sessions .pj-scroll", ".pj-ov-issues"];
  if (draw && current?.overview.isConnected) for (const selector of scrolls) {
    const element = current.overview.querySelector<HTMLElement>(selector);
    if (element) current.overviewScroll.set(selector, element.scrollTop);
  }
  if (!draw && key === null) {
    disposeProjectShell();
  }
  pa.tile = key === "sync" || key === "terminals" ? "machines" : key;
  if (key !== "actions") runs.stop();
  if (pa.tile !== "machines") terms.stop();
  if (key !== "branches") gitHistory.stop();
  if (!draw) return;
  projectChanged.notify();
  boxOf("projects")?.querySelector(":scope > .cc-view .pj-det")?.scrollTo({ top: 0 });
  if ((key === "overview" || key === null) && current) for (const selector of scrolls) {
    const element = current.overview.querySelector<HTMLElement>(selector), top = current.overviewScroll.get(selector);
    if (element && top !== undefined) element.scrollTop = top;
  }
}

/// The project's sessions with their latest status and reported worktree.
function projectSessions(project: string): BranchSessionRow[] | undefined {
  const listed = sessionsOf(project);
  if (!listed) return undefined;
  const key = (s: SessionRow) => `${s.machine_id}:${s.agent}:${s.id}`;
  const latest = new Map(sessions.map(s => [key(s), s]));
  return listed.filter(sessionVisible).map(s => {
    const live = latest.get(key(s));
    const session = live && live.last_activity >= s.last_activity ? live : s;
    return { ...session, status: shownStatus(session), worktree: placeOf(session)?.folder };
  });
}

function openBranchSession(session: SessionRow, changes = false) {
  go("pane", { agent: session });
  if (changes) showChanges();
}

export const gitHistory = makeGitHistory({
  invoke,
  back: () => openTile("overview"),
  sessions: projectSessions,
  openSession: openBranchSession,
  changedCheckout: (project, row) => { rememberGitRow(project, row); projectLoaded.notify(); },
});
