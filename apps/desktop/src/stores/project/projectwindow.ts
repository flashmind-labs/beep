// A project in a window of its own.
import type { MouseEventHandler } from "react";
import type { PaneTarget } from "../../shell/bar/pane.ts";
import { windowPreferenceKey } from "../../ui/window-layout.ts";
import type { Location } from "../center/state.ts";

/// What a click needs to say, as a `MouseEvent` has it.
interface ClickLike {
  button: number;
  metaKey: boolean;
}

/// The handlers a project's row or card takes to open it in its own window.
export interface ProjectWindowProps {
  clicks: { onClickCapture: MouseEventHandler; onAuxClick: MouseEventHandler; onMouseEnter: MouseEventHandler; onMouseLeave: MouseEventHandler };
  menu: { onContextMenu: MouseEventHandler };
}

/// The menu line that opens a project in its own window.
export const NEW_WINDOW = "Open in New Window";

/// Whether a click opens the project in a window of its own: ⌘ with the
/// main button, or the middle button.
export function opensNewWindow(e: ClickLike): boolean {
  return e.button === 1 || (e.button === 0 && e.metaKey);
}

/// What a window apart from the main one shows alone (`Alone` in window.rs).
export type Alone = { kind: "project"; project: string } | { kind: "chat"; machine_id: string; session: string };

/// The session or thread showing inside a detached window when the app exited.
const LAST_SESSION = "beep.windowSession";

export function keepWindowSession(page: Location) {
  const target = page.view === "pane" ? page.arg : null;
  const key = windowPreferenceKey(LAST_SESSION);
  try {
    if (target && typeof target === "object" && "agent" in target) {
      localStorage.setItem(key, JSON.stringify([target.agent.machine_id, target.agent.id]));
    } else {
      localStorage.removeItem(key);
    }
  } catch { /* storage can be unavailable */ }
}

export function savedWindowSession(): { machine_id: string; id: string } | null {
  if (!window.__BEEP_RESTORING_WINDOW__) return null;
  try {
    const pair: unknown = JSON.parse(localStorage.getItem(windowPreferenceKey(LAST_SESSION)) ?? "null");
    if (Array.isArray(pair) && typeof pair[0] === "string" && typeof pair[1] === "string") {
      return { machine_id: pair[0], id: pair[1] };
    }
  } catch { /* the window opens on its project or chat */ }
  return null;
}

/// The window's title: its project's name, the Assistant for a chat's, Beep for the main window.
export function windowTitle(alone: Alone | null): string {
  return !alone ? "Beep" : alone.kind === "project" ? alone.project : "Assistant";
}

/// What a project or chat window knows to tell its own targets from the rest.
export type WindowScope =
  /// The project's folder on each machine; empty until they are known.
  | { kind: "project"; project: string; folders: readonly { id: string; folder: string }[] }
  /// The chat and its threads.
  | { kind: "chat"; sessions: readonly { machine_id: string; id: string }[] };

/// Where a target opens: in this window, or in the main one.
type Route = "here" | "main";

/// A coding agent session, as far as routing and project scoping need it.
export interface AgentLike {
  machine_id: string;
  machine_name?: string;
  id?: string;
  agent?: string;
  title?: string | null;
  cwd?: string | null;
  last_activity?: number;
  place?: { main?: string | null } | null;
}

function withinFolder(path: string | null | undefined, folder: string): boolean {
  if (!path) return false;
  const root = folder.replace(/\/+$/, "");
  return !!root && (path === root || path.startsWith(root + "/"));
}

/// Whether `agent` runs in one of the project's folders, a worktree under it
/// or a linked worktree of it included.
export function agentInProject(agent: AgentLike, folders: Extract<WindowScope, { kind: "project" }>["folders"]): boolean {
  if (!folders.length) return true;
  return folders.some(f => f.id === agent.machine_id && (withinFolder(agent.cwd, f.folder) || withinFolder(agent.place?.main, f.folder)));
}

/// Where `go(view, arg)` opens.
export function route(scope: WindowScope | null, page: Location): Route {
  if (!scope) return "here";
  if (scope.kind === "chat") return chatRoute(scope.sessions, page);
  if (page.view === "projects") return page.arg?.project === scope.project ? "here" : "main";
  if (page.view !== "pane") return "main";
  const target = page.arg;
  if (!target || typeof target !== "object") return "main";
  if ("agentnew" in target) return "here";
  if ("assistant" in target) return "main";
  if ("draft" in target) return target.draft.project === scope.project ? "here" : "main";
  return agentInProject(target.agent, scope.folders) ? "here" : "main";
}

/// A chat's window keeps the chat and its threads, and the start of a thread; the rest goes to
/// the main window.
function chatRoute(sessions: readonly { machine_id: string; id: string }[], page: Location): Route {
  const target = page.view === "pane" ? page.arg : null;
  if (!target || typeof target !== "object") return "main";
  if ("agentnew" in target) return "here";
  if (!("agent" in target)) return "main";
  const agent = target.agent;
  return sessions.some(row => row.machine_id === agent.machine_id && row.id === agent.id) ? "here" : "main";
}

/// What the main window is asked to show (`CenterTarget` in center.rs).
export type CenterTarget = "home" | "settings" | "instructions" | "providers" | "terminals" | "machines" | "report"
  | { agent: AgentTarget } | { projects: { project: string | null; run?: { id: number; host: string } | null } } | { notice: { session: string | null } };

/// A coding agent session for the main window, every field filled as
/// `AgentTarget` in center.rs reads it.
interface AgentTarget {
  kind: string;
  machine_id: string;
  machine_name: string;
  id: string;
  agent: string;
  title: string | null;
  cwd: string;
  status: string;
  doing: string | null;
  started_at: number;
  last_activity: number;
  external: boolean;
}

/// The main window's wire target for a typed location. Internal-only pages
/// fall back to Home, as the native window protocol cannot represent them.
export function mainTarget(page: Location): CenterTarget {
  switch (page.view) {
    case "home": case "instructions": case "providers": case "terminals": return page.view;
    case "projects": return { projects: { project: page.arg?.project ?? null, run: page.arg?.run ?? null } };
    case "pane": return paneTarget(page.arg);
  }
}

function paneTarget(target: PaneTarget): CenterTarget {
  if (target === "settings") return target;
  if (!target || typeof target === "string") return "home";

  if ("agent" in target) {
    const agent = target.agent;
    return { agent: {
      kind: "agent", machine_id: agent.machine_id, machine_name: agent.machine_name ?? "",
      id: agent.id, agent: agent.agent ?? "", title: agent.title ?? null, cwd: agent.cwd ?? "",
      status: agent.status ?? "idle", doing: agent.doing ?? null, started_at: agent.started_at ?? 0,
      last_activity: agent.last_activity ?? 0, external: agent.external ?? false,
    } };
  }

  return "home";
}
