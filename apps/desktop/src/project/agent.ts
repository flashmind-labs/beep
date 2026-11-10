// A project's agent on its page: the Tasks list, the Workers tree and the
// cards its chat waits on.
import { agentLabel } from "./projects-state.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { localProjectId } from "./localid.ts";
import { projectOf, type ChatApproval } from "../agents/running-state.ts";
import type { ViewName } from "../center/state.ts";

/// A project agent with its chat (ProjectAgent).
export interface ProjectAgent {
  project_id: string;
  /// `org/repo`.
  name: string;
  chat_id: string;
  open_tasks: number;
  open_workers: number;
}

/// One task of a project agent (TaskRow).
export interface TaskRow {
  id: string;
  title: string;
  /// `planned`, `running`, `blocked`, `review`, `done` or `dropped`.
  state: string;
  /// The task that must finish first.
  depends_on: string | null;
  issue_number: number | null;
  issue_url: string | null;
  branch: string | null;
  pr_url: string | null;
  /// The worker on it now, if one is.
  worker: string | null;
  /// In milliseconds since the Unix epoch.
  updated_at: number;
}

/// One worker of a project agent (WorkerRow).
export interface WorkerRow {
  id: string;
  task_id: string;
  /// The machine it runs on.
  machine_id: string;
  machine_name: string;
  agent: string;
  /// The node's id for its coding-agent session, once it has started.
  session: string | null;
  folder: string;
  state: string;
  /// Not ended or failed, so it can be stopped.
  live: boolean;
  last_output: string | null;
  persona: string | null;
  /// In milliseconds since the Unix epoch.
  started_at: number;
  ended_at: number | null;
}

/// What `center_project` gives (ProjectView): the cards are approvals the
/// project chat waits on.
export interface ProjectView {
  tasks: TaskRow[];
  workers: WorkerRow[];
  turns: { role: string }[];
  cards: ChatApproval[];
}

/// What happened to a card once answered: the word it shows, or why it failed.
interface CardOutcome {
  said?: string;
  error?: string;
}

/// A task row's change: Drop or Reopen.
type TaskChange = "drop" | "reopen";

/// How a card is answered.
type CardDecision = "allow" | "session" | "deny";

/// What the sections do, as the page hands it to them. `busy` and `error`
/// are by task or worker id, `outcome` by a card's query id.
export interface ProjectActs {
  busy(id: string): boolean;
  error(id: string): string | null | undefined;
  outcome(id: string): CardOutcome | null | undefined;
  openUrl(url: string): unknown;
  /// Opens a branch's review; absent where there is none.
  review?: (branch: string) => void;
  /// What opens the session working on a branch at its changes; null when none works on it.
  diff?: (branch: string) => (() => void) | null;
  /// Asks the project agent to merge a task's branch; absent where there is none.
  merge?: (task: TaskRow) => unknown;
  /// Opens a worker's coding-agent session; absent where there is none.
  openWorker?: (worker: WorkerRow, title: string | null) => void;
  change(task: TaskRow, change: TaskChange): unknown;
  stop(worker: WorkerRow): unknown;
  answer(card: ChatApproval, decision: CardDecision, said: string): unknown;
}
/// The actions task, worker and pull request rows use: all but answering cards.
export type RowActs = Omit<ProjectActs, "answer">;

/// What the window shows, for `newChatTarget`.
interface ChatAt {
  shown: ViewName | null;
  picked?: string | null;
  view?: string | null;
  session?: string | null;
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

/// A piece under a task's title.
interface TaskMeta {
  text: string;
  url?: string;
  code?: boolean;
}

type Named = Pick<ProjectAgent, "name" | "chat_id">;
type FolderOf = { name: string; machine_id: string; folder: string };

/// The project agent for project `name`, or null.
export function agentFor<T extends Named>(agents: readonly T[] | null | undefined, name: string | null | undefined) {
  return (agents || []).find(a => a.name === name) || null;
}

/// What a project's page draws its lists with: the project agent, or with no subscription a
/// stand-in that only carries the project's id, from which the mesh reads its runs and pull
/// requests. It has no chat, tasks or plan.
export function pageAgent(agents: readonly ProjectAgent[] | null | undefined, name: string, subscribed: boolean): ProjectAgent | null {
  return agentFor(agents, name) || (subscribed ? null : { project_id: localProjectId(name), name, chat_id: "", open_tasks: 0, open_workers: 0 });
}

/// The project agent whose chat is `session`, or null for a general chat.
export function chatProject<T extends Named>(agents: readonly T[] | null | undefined, session: string | null | undefined) {
  if (!session) return null;
  return (agents || []).find(a => a.chat_id === session) || null;
}

/// "widget" from "acme/widget": the name a chat's header shows.
export function shortName(name: string | null | undefined) {
  const n = String(name || "");
  return n.slice(n.lastIndexOf("/") + 1) || n;
}

/// Where ⌘N starts an agent: the project whose page shows, or whose session
/// (a coding agent's, or its project chat) the pane shows. The machine is the
/// shown session's, else this one when the project has a checkout here, else
/// the first with a checkout. Null opens the start form.
export function newChatTarget(at: ChatAt, agents: readonly Named[] | null | undefined, projects: readonly FolderOf[] | null = []): NewChatTarget | null {
  let project: string | null = null;
  if (at.shown === "projects") project = at.picked || null;
  else if (at.shown === "pane" && at.view === "chat") project = chatProject(agents, at.session)?.name ?? null;
  else if (at.shown === "pane" && at.view === "agent" && at.agent) project = projectOf({ machine_id: at.agent.machine_id, cwd: at.agent.cwd ?? "", main: at.agent.main }, projects ?? [])?.name ?? null;
  if (!project) return null;
  const on = (projects ?? []).filter(p => p.name === project).map(p => p.machine_id);
  const machine = [at.agent?.machine_id, at.mac].find(m => m && on.includes(m)) ?? on[0] ?? null;
  return { project, machine };
}

/// "18 min", "2 h 5 min", "3 d": how long `from` to `to` took.
export function duration(from: number, to: number) {
  const min = Math.max(0, Math.floor((to - from) / 60000));
  if (min < 1) return "under a minute";
  if (min < 60) return `${min} min`;
  const hours = Math.floor(min / 60);
  if (hours < 24) return min % 60 ? `${hours} h ${min % 60} min` : `${hours} h`;
  return `${Math.floor(hours / 24)} d`;
}

/// What a task row offers: Drop while it may still happen, Reopen once it
/// is over.
export function taskAction(state: string): { change: TaskChange; label: string } {
  if (state === "dropped" || state === "done") return { change: "reopen", label: "Reopen" };
  return { change: "drop", label: "Drop" };
}

const STATE_WORDS: Record<string, string> = { running: "running", review: "in review", blocked: "blocked", planned: "planned", done: "done", dropped: "dropped" };

/// The section's count line: "2 running · 1 in review · 3 planned".
export function taskSummary(tasks: readonly { state: string }[] | null | undefined) {
  const counts = new Map<string, number>();
  for (const t of tasks || []) counts.set(t.state, (counts.get(t.state) || 0) + 1);
  return Object.keys(STATE_WORDS).filter(k => counts.get(k)).map(k => `${counts.get(k)} ${STATE_WORDS[k]}`).join(" · ");
}

/// "PR #41" from a pull request's address, "PR" when it has no number.
function prLabel(url: string | null | undefined) {
  const n = String(url || "").match(/\/(?:pull|pulls)\/(\d+)/);
  return n ? `PR #${n[1]}` : "PR";
}

/// The pieces under a task's title: its issue and PR links, who works on
/// it, its branch, and the task it waits for while that is not done.
export function taskMeta(
  task: Partial<Pick<TaskRow, "issue_url" | "issue_number" | "pr_url" | "worker" | "branch" | "depends_on">>,
  tasks: readonly Pick<TaskRow, "id" | "state" | "title">[] | null | undefined,
  workers: readonly Pick<WorkerRow, "id" | "agent" | "machine_name">[] | null | undefined,
) {
  const out: TaskMeta[] = [];
  if (task.issue_url) out.push({ text: task.issue_number ? `#${task.issue_number}` : "Issue", url: task.issue_url });
  if (task.pr_url) out.push({ text: prLabel(task.pr_url), url: task.pr_url });
  const w = task.worker && (workers || []).find(x => x.id === task.worker);
  if (w) out.push({ text: `${agentLabel(w.agent)} on ${w.machine_name}` });
  if (task.branch) out.push({ text: task.branch, code: true });
  const before = task.depends_on && (tasks || []).find(t => t.id === task.depends_on);
  if (before && before.state !== "done") out.push({ text: `after ${before.title}` });
  return out;
}

/// The folder a worker runs in, from `.worktrees/` on when it is a
/// worktree, else its last two parts.
export function workerFolder(folder: string | null | undefined) {
  const f = String(folder || "");
  const at = f.indexOf(".worktrees/");
  if (at >= 0) return f.slice(at);
  return f.split(/[\\/]/).filter(Boolean).slice(-2).join("/");
}

/// A worker's line: agent, machine, folder, state and how long it ran.
export function workerLine(w: Pick<WorkerRow, "agent" | "machine_name" | "folder" | "state" | "started_at" | "ended_at">, now: number) {
  const took = duration(w.started_at, w.ended_at ?? now);
  return [agentLabel(w.agent), w.machine_name, workerFolder(w.folder), w.state, took].filter(Boolean).join(" · ");
}

/// The session a worker row opens, as the agent view takes it, or null
/// before its session has started. `title` is its task's title.
export function workerSession(w: Pick<WorkerRow, "machine_id" | "machine_name" | "session" | "agent" | "folder" | "state">, title: string | null) {
  if (!w.session || !w.machine_id) return null;
  return {
    kind: "agent",
    machine_id: w.machine_id,
    machine_name: w.machine_name,
    id: w.session,
    agent: w.agent,
    title,
    cwd: w.folder,
    status: WORKER_STATUS[w.state] || "ended",
    external: false,
  };
}

/// A worker's state as the agent view words a session's.
const WORKER_STATUS: Record<string, string> = { starting: "starting", running: "working", waiting: "idle" };

// ---------- Drawing ----------

/// A task row's right-click menu: what its buttons do. Drop and Merge ask first.
export function taskMenu(e: MouseEvent, t: TaskRow, act: RowActs, diff: (() => void) | null | undefined) {
  const items: MenuItem[] = [];

  if (t.state === "review") {
    if (t.branch && act.review) items.push({ label: "Review", on: () => act.review!(t.branch!) });
    if (t.branch && t.pr_url && act.merge) items.push({
      label: "Merge…",
      on: () => confirmSheet({
        title: `Merge ${t.title}?`,
        text: "The branch merges once its review allows it; otherwise its review opens.",
        action: "Merge",
        run: async () => { await act.merge!(t); },
      }),
    });
    if (t.pr_url) items.push({ label: "Open PR", on: () => act.openUrl(t.pr_url!) });
  }

  if (diff) items.push({ label: "View diff", on: diff });
  const { change, label } = taskAction(t.state);
  if (change === "reopen") items.push({ label, on: () => void act.change(t, change) });
  else items.push({
    label: `${label}…`,
    warn: true,
    on: () => confirmSheet({
      title: `Drop ${t.title}?`,
      text: "The task is not worked on. Reopen it later to bring it back.",
      action: label,
      run: async () => { await act.change(t, change); },
    }),
  });
  openMenu(e, t.title, items);
}

export function workerMenu(e: MouseEvent, w: WorkerRow, name: string, title: string | null, act: RowActs) {
  const items: MenuItem[] = [];
  if (w.session && act.openWorker) items.push({ label: "Open session", on: () => act.openWorker?.(w, title) });
  if (w.live) items.push({ label: "Stop…", warn: true, on: () => confirmSheet({
    title: `Stop ${name}?`, text: "The worker stops and its session ends.", action: "Stop",
    run: async () => { await act.stop(w); },
  }) });
  if (items.length) openMenu(e, name, items);
}
