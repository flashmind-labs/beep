// What Home and the Projects page say about coding agents running or waiting:
// their rows, lines and keys, apart from the DOM so it can be tested.
import { base, formatAgo } from "../ui/format.ts";
import type { ProjectRow } from "../project/projects-state.ts";

/// A coding agent's session running now: a Running now row (RunningAgent).
export interface RunningAgent {
  kind: "agent";
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
  /// Its subagents that still run.
  subagents?: number;
}

/// What a coding agent asks (AgentQuestion).
export interface AgentQuestion {
  id: number;
  text: string;
  detail: string | null;
  options: { id: string; name: string; allow: boolean }[];
}

const AGENTS: Record<string, string> = { claude: "Claude Code", codex: "Codex", opencode: "OpenCode", hermes: "Hermes" };

export function agentName(name: string | null | undefined) {
  return AGENTS[name ?? ""] || name || "Agent";
}

/// A Running now row: a title and one muted line.
export function runningLine(
  r: Pick<RunningAgent, "agent" | "title" | "cwd" | "machine_id" | "machine_name">,
  projects: readonly ProjectFolder[] = [],
) {
  const project = repoOf(r, projects);
  return { title: r.title || project || agentName(r.agent), sub: [project, r.machine_name].filter(Boolean).join(" · ") };
}

/// When a Running now row last moved: now for an agent in a turn, else its
/// last step or reply (its end, once it stopped).
const runningAt = (r: Pick<RunningAgent, "started_at" | "last_activity"> & { status?: string }) =>
  r.status === "working" ? Date.now() : r.last_activity || r.started_at;

/// Idle sessions show how long ago they were last active.
export function runningWhen(r: Parameters<typeof runningAt>[0]) {
  const time = formatAgo(runningAt(r), "short");
  if (r.status !== "idle" || time === "now") return time;
  return `${time.replace(" min", "m").replace(" h", "h").replace(" d", "d")} ago`;
}

/// Running now rows, the one that moved last first.
export function byRecent<T extends Parameters<typeof runningAt>[0]>(rows: readonly T[]) {
  return [...rows].sort((a, b) => runningAt(b) - runningAt(a));
}

/// What names a Running now row across redraws.
export const runningKey = (r: Pick<RunningAgent, "machine_id" | "id">) => `agent:${r.machine_id}:${r.id}`;

/// An imported project's folder on one machine.
type ProjectFolder = Pick<ProjectRow, "name" | "machine_id" | "folder"> & { url?: ProjectRow["url"]; display_name?: string | null };

/// The repo's path on its host from a remote, groups and all, without `.git`:
/// `org/repo`, or `group/sub/repo` on GitLab.
export function remotePath(url: string | null | undefined) {
  const u = String(url || "").trim();
  const m = /^[a-z][a-z0-9+.-]*:\/\/[^/]+\/(.+)$/i.exec(u) ?? /^[^/\s:@]+@[^/\s:]+:(.+)$/.exec(u);
  const path = m?.[1].replace(/\/+$/, "").replace(/\.git$/i, "").replace(/^\/+/, "");
  return path && path.includes("/") ? path : null;
}

/// The repo a session runs in: the imported project whose folder on its
/// machine holds it, by its remote's full path ("group/sub/repo"), else by
/// its short name ("repo" of "org/repo"); else its folder's name.
export function repoOf(r: Pick<RunningAgent, "machine_id" | "cwd">, projects: readonly ProjectFolder[] = []) {
  const p = projectOf(r, projects);
  if (!p) return base(r.cwd);
  return p.display_name ?? remotePath(p.url) ?? p.name.split("/").pop();
}

/// `path` without trailing slashes: a moved session's folder can end in one.
const trimSlash = (path: string) => path.replace(/(.)\/+$/, "$1");

/// The project whose folder holds a session: the deepest one. `main` is the
/// repository's main checkout when the session works in a linked worktree,
/// which may be outside the project's folder.
export function projectOf(r: Pick<RunningAgent, "machine_id" | "cwd"> & { main?: string | null }, projects: readonly ProjectFolder[] = []) {
  const paths = [r.cwd, r.main].filter((p): p is string => !!p).map(trimSlash);
  const holds = (folder: string) => paths.some(p => p === folder || p.startsWith(folder + "/"));
  const inside = (p: ProjectFolder) => p.machine_id === r.machine_id && holds(trimSlash(p.folder));
  return projects.filter(inside).sort((a, b) => b.folder.length - a.folder.length)[0];
}

/// Whether `open` is session `s`, on the same machine and agent.
export function sameSession(
  open: { machine_id: string; agent: string; session: string } | null,
  s: { machine_id: string; agent: string; id: string },
) {
  return !!open && open.machine_id === s.machine_id && open.agent === s.agent && open.session === s.id;
}
