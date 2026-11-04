// A project's agent on its page: the Tasks list, the Workers tree and the
// cards its chat waits on.
import { harnessEl } from "../agents/harnessicon.ts";
import type { ChatApproval } from "../center/home-state.ts";
import { agentLabel, el } from "./projects-state.ts";
import { dbtn, emptyPanel, stateChip, svgEl } from "../ui/parts.ts";
import type { Kid } from "./projects-state.ts";

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
export interface CardOutcome {
  said?: string;
  error?: string;
}

/// A task row's change: Drop or Reopen.
export type TaskChange = "drop" | "reopen";

/// How a card is answered.
export type CardDecision = "allow" | "session" | "deny";

/// What the sections do, as the page hands it to them. `busy` and `error`
/// are by task or worker id, `outcome` by a card's query id.
export interface ProjectActs {
  busy(id: string): boolean;
  error(id: string): string | null | undefined;
  outcome(id: string): CardOutcome | null | undefined;
  openUrl(url: string): unknown;
  /// Opens a branch's review; absent where there is none.
  review?: (branch: string) => void;
  /// Shows what changed on a branch; absent where nothing can.
  diff?: (branch: string) => void;
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

/// What the window shows, for `newChatProject`.
export interface ChatAt {
  shown: string | null;
  picked?: string | null;
  view?: string | null;
  session?: string | null;
}

/// A piece under a task's title.
export interface TaskMeta {
  text: string;
  url?: string;
  code?: boolean;
}

type Named = Pick<ProjectAgent, "name" | "chat_id">;

/// The project agent for project `name`, or null.
export function agentFor<T extends Named>(agents: readonly T[] | null | undefined, name: string | null | undefined) {
  return (agents || []).find(a => a.name === name) || null;
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

/// The project ⌘N goes to: the one whose page shows, or whose chat the
/// pane shows; null starts a general chat.
export function newChatProject(at: ChatAt, agents: readonly Named[] | null | undefined) {
  if (at.shown === "projects") return at.picked || null;
  if (at.shown === "pane" && at.view === "chat") return chatProject(agents, at.session)?.name ?? null;
  return null;
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
export function prLabel(url: string | null | undefined) {
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

function section(id: string, title: string, sub: string, body: Node) {
  const box = el("div");
  box.id = id;
  box.append(el("div", "cc-sec", el("h4", "", title), sub ? el("small", "", sub) : null), body);
  return box;
}

function link(text: string, url: string, open: (url: string) => unknown) {
  const a = el("a", "", text);
  a.href = url;
  a.addEventListener("click", e => { e.preventDefault(); open(url); });
  return a;
}

/// A row's actions: its button and why the last try failed.
function acts(b: HTMLButtonElement, err: string | null | undefined) {
  return el("span", "acts", b, err ? el("span", "cc-err", err) : null);
}

/// An empty section's note.
function empty(text: string) {
  return emptyPanel(el("span", "", text));
}

/// The Tasks section. `act.change(task, change)` drops or reopens one.
export function tasksSection(view: ProjectView, act: RowActs) {
  const tasks = view.tasks;
  if (!tasks.length) return section("sec-tasks", "Tasks", "", empty("No tasks yet. Ask the project agent below and it plans them."));
  const rows = tasks.map(t => {
    const meta = el("span", "m", ...taskMeta(t, tasks, view.workers).map(m =>
      m.url ? link(m.text, m.url, act.openUrl) : m.code ? el("code", "", m.text) : el("span", "", m.text)));
    const { change, label } = taskAction(t.state);
    const busy = act.busy(t.id);
    const b = dbtn(busy ? "…" : label, "quiet small", () => act.change(t, change));
    b.disabled = !!busy;
    const err = act.error(t.id);
    const title = el("span", "t", t.title);
    title.title = t.title;
    const row = el("div", "pj-task", title, stateChip(t.state, t.state, t.state === "running"), meta, acts(b, err));
    row.dataset.task = t.id;
    const review = reviewActs(t, act);
    if (t.branch && act.diff && t.state !== "dropped") {
      const branch = t.branch;
      review.push(dbtn("View diff", "small", () => act.diff!(branch)));
    }
    if (review.length) row.querySelector(".acts")!.prepend(...review);
    return row;
  });
  return section("sec-tasks", "Tasks", taskSummary(tasks), el("div", "cc-panel", ...rows));
}

/// A task in review: Review its branch, Merge… (which asks first) and
/// Open PR, as far as it has a branch and a PR.
function reviewActs(t: TaskRow, act: RowActs) {
  if (t.state !== "review") return [];
  const out: HTMLButtonElement[] = [];
  if (t.branch && act.review) out.push(dbtn("Review", "pri small", () => act.review!(t.branch!)));
  if (t.branch && t.pr_url && act.merge) {
    const m = dbtn(act.busy(t.id) ? "Asking…" : "Merge…", "small", () => act.merge!(t));
    m.disabled = !!act.busy(t.id);
    m.title = "Merge always asks first";
    out.push(m);
  }
  if (t.pr_url) out.push(dbtn("Open PR", "small", () => act.openUrl(t.pr_url!)));
  return out;
}

/// The Workers tree: the project agent, then each worker under it. A
/// worker whose session has started opens it; a live one offers Stop.
export function workersSection(view: ProjectView, act: RowActs, chatIcon: string) {
  const titles = new Map(view.tasks.map(t => [t.id, t.title]));
  const now = Date.now();
  const lead = (ag: string, kid: Kid) => el("span", `cc-ag ${ag}`, kid);
  const nodes = [el("div", "pj-node", lead("beep", svgEl(chatIcon)), el("span", "t", el("b", "", "Project agent"), el("span", "", "Plans tasks, files issues, places workers")))];
  for (const w of view.workers) {
    const name = titles.get(w.task_id) || workerFolder(w.folder);
    const sub = el("span", "", workerLine(w, now));
    sub.title = w.folder;
    const node = el("div", "pj-node child", harnessEl(w.agent), el("span", "t", el("b", "", name), sub));
    if (w.session && act.openWorker) {
      node.classList.add("link");
      node.title = "Open its session";
      node.addEventListener("click", () => act.openWorker!(w, titles.get(w.task_id) ?? null));
    }
    if (w.live) {
      const busy = act.busy(w.id);
      const stop = dbtn(busy ? "Stopping…" : "Stop", "quiet small", e => { e.stopPropagation(); act.stop(w); });
      stop.disabled = !!busy;
      node.append(acts(stop, act.error(w.id)));
    }
    if (w.last_output) {
      const out = el("span", "out", w.last_output);
      out.title = w.last_output;
      node.append(out);
    }
    if (!w.live) node.classList.add("over");
    nodes.push(node);
  }
  const live = view.workers.filter(w => w.live).length;
  return section("sec-workers", "Workers", live ? `${live} running` : "none running", el("div", "cc-panel pj-tree", ...nodes));
}

/// One card the project chat waits on: what it asks, its fields, and Approve,
/// Allow for this session (the same action, until the chat ends) or Deny.
function card(c: ChatApproval, act: ProjectActs) {
  const out = act.outcome(c.query_id);
  const a = c.approval;
  const items = a.fields.map(f => el("li", "", f.label, " ", el("small", "", f.value)));
  const row = el("div", "acts");
  if (out?.said) row.append(el("span", "ok", out.said));
  else {
    row.append(
      dbtn("Approve", "pri small", () => act.answer(c, "allow", "Approved")),
      dbtn("Allow for this session", "small", () => act.answer(c, "session", "Allowed for the rest of this chat")),
      dbtn("Deny", "warn small", () => act.answer(c, "deny", "Denied")));
    if (out?.error) row.append(el("span", "cc-err", out.error));
  }
  const box = el("div", "pj-plan", el("b", "", a.action), a.app ? el("small", "", a.app) : null, items.length ? el("ol", "", ...items) : null, row);
  if (out?.said) box.style.opacity = ".55";
  return box;
}

/// The cards the project chat waits on, above the board; null without any.
export function cardsSection(view: ProjectView, act: ProjectActs) {
  if (!view.cards.length) return null;
  return section("sec-project-needs", "Needs you", "the project chat waits on these", el("div", "pj-needs", ...view.cards.map(c => card(c, act))));
}
