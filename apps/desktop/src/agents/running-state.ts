// What Home and the Projects page say about coding agents running or waiting:
// their rows, lines and keys, apart from the DOM so it can be tested.
import type { ApprovalField } from "../chat/chat.ts";
import { ago, base } from "../ui/format.ts";
import type { ProjectRow } from "../project/projects-state.ts";

/// A coding agent's session running now.
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
}

/// A research running now.
export interface RunningResearch {
  kind: "research";
  session: string;
  topic: string;
  status: string;
  round: number;
  sources: number;
  started_at: number;
}

/// A Running now row (RunningRow).
export type RunningRow = RunningAgent | RunningResearch;

/// An approval a chat waits on (PendingApproval).
export interface ChatApproval {
  run_id: string;
  query_id: string;
  approval: { app: string; action: string; fields: ApprovalField[] };
}

/// What a coding agent asks (AgentQuestion).
export interface AgentQuestion {
  id: number;
  text: string;
  detail: string | null;
  options: { id: string; name: string; allow: boolean }[];
}

const AGENTS: Record<string, string> = { claude: "Claude Code", codex: "Codex", opencode: "OpenCode" };

export function agentName(name: string | null | undefined) {
  return AGENTS[name ?? ""] || name || "Agent";
}

/// A Running now row: a title and one muted line.
export function runningLine(
  r: Pick<RunningResearch, "kind" | "topic" | "round"> | Pick<RunningAgent, "kind" | "agent" | "title" | "cwd" | "machine_id" | "machine_name">,
  projects: readonly ProjectFolder[] = [],
) {
  if (r.kind === "research") return { title: r.topic, sub: `Research · round ${r.round}` };
  const project = repoOf(r, projects);
  return { title: r.title || project || agentName(r.agent), sub: [project, r.machine_name].filter(Boolean).join(" · ") };
}

/// When a Running now row last moved: now for an agent in a turn, else its
/// last step or reply (its end, once it stopped), a research's start.
export const runningAt = (r: (Pick<RunningAgent, "kind" | "started_at" | "last_activity"> & { status?: string }) | Pick<RunningResearch, "kind" | "started_at">) =>
  r.kind !== "agent" ? r.started_at : r.status === "working" ? Date.now() : r.last_activity || r.started_at;

/// What a Running now row says for its time. A session Beep started reports
/// only its start, so one between turns says idle, not how long ago it began.
export const runningWhen = (r: Parameters<typeof runningAt>[0]) =>
  r.kind === "agent" && r.status === "idle" ? "idle" : ago(runningAt(r));

/// Running now rows, the one that moved last first.
export function byRecent<T extends Parameters<typeof runningAt>[0]>(rows: readonly T[]) {
  return [...rows].sort((a, b) => runningAt(b) - runningAt(a));
}

/// What names a Running now row across redraws.
export const runningKey = (r: Pick<RunningAgent, "kind" | "machine_id" | "id"> | Pick<RunningResearch, "kind" | "session">) =>
  r.kind === "agent" ? `agent:${r.machine_id}:${r.id}` : `research:${r.session}`;

/// An imported project's folder on one machine.
type ProjectFolder = Pick<ProjectRow, "name" | "machine_id" | "folder"> & { url?: ProjectRow["url"] };

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
  return remotePath(p.url) ?? p.name.split("/").pop();
}

/// The project whose folder holds a session: the deepest one.
export function projectOf(r: Pick<RunningAgent, "machine_id" | "cwd">, projects: readonly ProjectFolder[] = []) {
  const inside = (p: ProjectFolder) => p.machine_id === r.machine_id && (r.cwd === p.folder || r.cwd?.startsWith(p.folder + "/"));
  return projects.filter(inside).sort((a, b) => b.folder.length - a.folder.length)[0];
}

/// Whether `open` is session `s`, on the same machine and agent.
export function sameSession(
  open: { machine_id: string; agent: string; session: string } | null,
  s: { machine_id: string; agent: string; id: string },
) {
  return !!open && open.machine_id === s.machine_id && open.agent === s.agent && open.session === s.id;
}
