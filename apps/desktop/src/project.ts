// A project's agent on its page: the Tasks list, the Workers tree and the
// cards its chat waits on. The chat itself opens in the pane from the
// page's prompt bar. center.ts owns the page and its refreshes; this file
// draws these sections and holds the pure pieces the tests check.
import { agentOf } from "./home.ts";
import type { ChatApproval } from "./home.ts";
import { agentLabel, button, el } from "./projects-state.ts";
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
  machine_name: string;
  agent: string;
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

/// What the prompt bar under project `name` asks for.
export function barPlaceholder(name: string) {
  return `Ask the project agent about ${name}`;
}

/// Whether the project chat has anything to open: something said by the
/// user or the agent.
export function chatStarted<T extends { role: string }>(turns: readonly T[] | null | undefined) {
  return (turns || []).some(t => t.role === "user" || t.role === "assistant");
}

// ---------- Drawing ----------

/// A `dbtn` button with `cls`.
function dbtn(label: string, cls: string, on: () => void) {
  return button(label, `dbtn${cls ? " " + cls : ""}`, on);
}

function section(id: string, title: string, sub: string, body: Node) {
  const box = el("div");
  box.id = id;
  box.append(el("div", "cc-sec", el("h4", "", title), sub ? el("small", "", sub) : null), body);
  return box;
}

function chip(state: string) {
  return el("span", `pj-st ${state}`, state === "running" ? el("span", "dspin") : null, state);
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
  return el("div", "cc-panel cc-empty", el("span", "", text));
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
    return el("div", "pj-task", title, chip(t.state), meta, acts(b, err));
  });
  return section("sec-tasks", "Tasks", taskSummary(tasks), el("div", "cc-panel", ...rows));
}

/// The Workers tree: the project agent, then each worker under it. Worker
/// rows open nothing yet; a live one offers Stop.
export function workersSection(view: ProjectView, act: RowActs, chatIcon: string) {
  const titles = new Map(view.tasks.map(t => [t.id, t.title]));
  const now = Date.now();
  const lead = (ag: string, kid: Kid) => el("span", `cc-ag ${ag}`, kid);
  const icon = el("span");
  icon.style.display = "inline-flex";
  icon.innerHTML = chatIcon; // fixed markup from center.ts, no server text
  const nodes = [el("div", "pj-node", lead("beep", icon.firstChild), el("span", "t", el("b", "", "Project agent"), el("span", "", "Plans tasks, files issues, places workers")))];
  for (const w of view.workers) {
    const ag = agentOf(w.agent);
    const name = titles.get(w.task_id) || workerFolder(w.folder);
    const sub = el("span", "", workerLine(w, now));
    sub.title = w.folder;
    const node = el("div", "pj-node child", lead(ag.cls, ag.ini), el("span", "t", el("b", "", name), sub));
    if (w.live) {
      const busy = act.busy(w.id);
      const stop = dbtn(busy ? "Stopping…" : "Stop", "quiet small", () => act.stop(w));
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

/// One card the project chat waits on: what it asks, its fields, and
/// Approve, Allow for this session (the same action, until the chat ends)
/// or Deny.
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

/// The pull requests the project's tasks opened, each linked, with its
/// task's state and branch.
export function prsSection(view: ProjectView, act: RowActs) {
  const tasks = view.tasks.filter((t): t is TaskRow & { pr_url: string } => !!t.pr_url && t.state !== "dropped");
  if (!tasks.length) return section("sec-prs", "Pull requests", "", empty("No pull requests yet. A task's worker opens one when its work is ready."));
  const rows = tasks.map(t => {
    const title = el("span", "t", link(prLabel(t.pr_url), t.pr_url, act.openUrl), " ", t.title);
    title.title = t.title;
    const review = t.branch && act.review && t.state !== "done" ? el("span", "acts", dbtn("Review", t.state === "review" ? "pri" : "", () => act.review!(t.branch!))) : null;
    return el("div", "pj-task", title, chip(t.state), el("span", "m", t.branch ? el("code", "", t.branch) : null), review);
  });
  const open = tasks.filter(t => t.state !== "done").length;
  return section("sec-prs", "Pull requests", open ? `${open} open` : "", el("div", "cc-panel", ...rows));
}
