// A project in a window of its own.

/// What a click needs to say, as a `MouseEvent` has it.
export interface ClickLike {
  button: number;
  metaKey: boolean;
}

/// The menu line that opens a project in its own window.
export const NEW_WINDOW = "Open in New Window";

/// Whether a click opens the project in a window of its own: ⌘ with the
/// main button, or the middle button.
export function opensNewWindow(e: ClickLike): boolean {
  return e.button === 1 || (e.button === 0 && e.metaKey);
}

/// The view a window starts on: its project when it was opened on one,
/// else what the bar or tray left waiting (null).
export function startView(project: string | null): { view: "projects"; arg: { project: string } } | null {
  return project ? { view: "projects", arg: { project } } : null;
}

/// The window's title: its project's name, Beep for the main window.
export function windowTitle(project: string | null): string {
  return project || "Beep";
}

/// What a project window knows to tell its own targets from the rest.
export interface ProjectScope {
  project: string;
  /// The project chat `session` belongs to, its project chat or a PR or
  /// issue thread there; null for any other chat.
  chatProject(session: string): string | null;
  /// The project's folder on each machine; empty until they are known.
  folders: readonly { id: string; folder: string }[];
}

/// Where a target opens: in this window, or in the main one.
export type Route = "here" | "main";

/// A coding agent session, as far as routing needs it.
interface AgentLike {
  machine_id: string;
  cwd?: string | null;
}

/// Whether `agent` runs in one of the project's folders, a worktree under it
/// included.
export function agentInProject(agent: AgentLike, folders: ProjectScope["folders"]): boolean {
  if (!folders.length) return true;
  const cwd = agent.cwd || "";
  if (!cwd) return false;
  return folders.some(f => f.id === agent.machine_id && (cwd === f.folder || cwd.startsWith(f.folder.replace(/\/+$/, "") + "/")));
}

/// Where `go(view, arg)` opens.
export function route(scope: ProjectScope | null, view: string, arg?: unknown): Route {
  if (!scope) return "here";
  if (view === "projects") {
    const project = (arg as { project?: string | null } | undefined)?.project;
    return project === scope.project ? "here" : "main";
  }
  if (view !== "pane" || !arg || typeof arg !== "object") return "main";
  if ("agentnew" in arg) return "here";
  if ("agent" in arg) return agentInProject((arg as { agent: AgentLike }).agent, scope.folders) ? "here" : "main";
  if ("chat" in arg) {
    const session = (arg as { chat: { session: string | null } }).chat.session;
    return session && scope.chatProject(session) === scope.project ? "here" : "main";
  }
  return "main";
}

/// What the main window is asked to show (`CenterTarget` in center.rs).
export type CenterTarget = "home" | "chats" | "memories" | "routines" | "skills" | "apps" | "settings" | "instructions" | "providers" | "terminals" | "research"
  | { chat: { session: string | null } } | { group: { id: string } } | { agent: AgentTarget }
  | { projects: { project: string | null } } | { ask: { prompt: string } };

/// A coding agent session for the main window, every field filled as
/// `AgentTarget` in center.rs reads it.
export interface AgentTarget {
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

/// Pane pages the main window opens by name.
const PANE_PAGES = new Set(["chats", "memories", "routines", "skills", "apps", "settings"]);
/// Window views the main window opens by name.
const VIEWS = new Set(["home", "instructions", "providers", "terminals", "research"]);

/// The main window's target for `go(view, arg)`; Home for anything it
/// cannot be told.
export function mainTarget(view: string, arg?: unknown): CenterTarget {
  if (VIEWS.has(view)) return view as CenterTarget;
  if (view === "projects") return { projects: { project: (arg as { project?: string | null } | undefined)?.project ?? null } };
  if (view !== "pane") return "home";
  if (typeof arg === "string") return PANE_PAGES.has(arg) ? arg as CenterTarget : "home";
  if (!arg || typeof arg !== "object") return "home";
  if ("chat" in arg) return { chat: { session: (arg as { chat: { session: string | null } }).chat.session ?? null } };
  if ("group" in arg) return { group: { id: (arg as { group: { id: string } }).group.id } };
  if ("agent" in arg) {
    const a = (arg as { agent: Partial<AgentTarget> & { machine_id: string; id: string } }).agent;
    return {
      agent: {
        kind: "agent",
        machine_id: a.machine_id,
        machine_name: a.machine_name ?? "",
        id: a.id,
        agent: a.agent ?? "",
        title: a.title ?? null,
        cwd: a.cwd ?? "",
        status: a.status ?? "idle",
        doing: a.doing ?? null,
        started_at: a.started_at ?? 0,
        last_activity: a.last_activity ?? 0,
        external: a.external ?? false,
      },
    };
  }
  return "home";
}

/// A search hit, as far as ordering needs it.
interface HitLike {
  kind: string;
  id: string;
  title: string;
  detail: string;
  top: boolean;
}

/// Whether hit `h` is about `project`: the project itself, one of its
/// branches, or a hit that names it.
function aboutProject(h: HitLike, project: string): boolean {
  if (h.kind === "project") return h.id === project;
  if (h.kind === "branch") return h.id.startsWith(project + " ");
  const text = `${h.title} ${h.detail}`.toLowerCase();
  const name = project.toLowerCase(), short = name.slice(name.lastIndexOf("/") + 1);
  return text.includes(name) || (short.length >= 3 && text.includes(short));
}

/// The palette's hits with `project`'s first within each group, the
/// groups and the top hit where they were. Without a project, unchanged.
export function projectFirst<H extends HitLike>(hits: readonly H[], project: string | null | undefined): H[] {
  if (!project) return [...hits];
  const out: H[] = [];
  let run: H[] = [];
  const flush = () => {
    out.push(...run.filter(h => aboutProject(h, project)), ...run.filter(h => !aboutProject(h, project)));
    run = [];
  };
  hits.forEach((h, i) => {
    const prev = hits[i - 1];
    if (prev && (h.top || prev.top || h.kind !== prev.kind)) flush();
    run.push(h);
  });
  flush();
  return out;
}
