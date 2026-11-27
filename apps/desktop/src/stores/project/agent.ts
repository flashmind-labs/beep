// Where ⌘N starts an agent, from what the window shows.
import { projectOf } from "../agents/running-state.ts";
import type { ViewName } from "../center/state.ts";

/// What the window shows, for `newChatTarget`.
interface ChatAt {
  shown: ViewName | null;
  picked?: string | null;
  view?: string | null;
  /// The coding agent session the pane shows, when it does.
  /// `main` is its repository's main checkout, when it works in a linked worktree.
  agent?: { machine_id: string; cwd?: string | null; main?: string | null } | null;
  /// This machine's id.
  mac?: string | null;
}

/// Where ⌘N starts: a project, on the machine its start form opens on.
interface NewChatTarget {
  project: string;
  machine: string | null;
}

type FolderOf = { name: string; machine_id: string; folder: string };

/// "widget" from "acme/widget": the name a header shows.
export function shortName(name: string | null | undefined) {
  const n = String(name || "");
  return n.slice(n.lastIndexOf("/") + 1) || n;
}

/// Where ⌘N starts an agent: the project whose page shows, or whose coding
/// agent session the pane shows. The machine is the shown session's, else
/// this one when the project has a checkout here, else the first with a
/// checkout. Null opens the start form.
export function newChatTarget(at: ChatAt, projects: readonly FolderOf[] | null = []): NewChatTarget | null {
  let project: string | null = null;
  if (at.shown === "projects") project = at.picked || null;
  else if (at.shown === "pane" && at.view === "agent" && at.agent) project = projectOf({ machine_id: at.agent.machine_id, cwd: at.agent.cwd ?? "", main: at.agent.main }, projects ?? [])?.name ?? null;
  if (!project) return null;
  const on = (projects ?? []).filter(p => p.name === project).map(p => p.machine_id);
  const machine = [at.agent?.machine_id, at.mac].find(m => m && on.includes(m)) ?? on[0] ?? null;
  return { project, machine };
}
