// A project's issues and pull requests, and the user's threads there. The
// board's Issues tile and the Pull requests tile's rows, and the project
// page's Pull requests and Issues sections: every open PR with Let the
// agent reply, Continue and Review; every open issue, an untracked one with
// Work on it, Plan, Discuss and Not now, a tracked one with its task; a
// thread of the user's gets Let the agent reply when someone wrote after
// them, and Discuss. center.ts draws the page; this file keeps the state.
import type { invoke as Invoke } from "./tauri.ts";
import { reason } from "./projects-state.ts";
import type { Ago, MakeButton, MakeEl } from "./projects-state.ts";
import { prLabel } from "./project.ts";
import type { ProjectAgent, TaskRow } from "./project.ts";
import type { TileData } from "./board.ts";
import { STATE_LABEL, reviewState } from "./review.ts";
import type { ReviewState, ReviewView } from "./review.ts";

/// An open issue no task tracks (IssueRow).
export interface IssueRow {
  number: number;
  title: string;
  labels: string[];
  author: string;
  /// In milliseconds since the Unix epoch.
  opened_at: number;
  comments: number;
  url: string;
}

/// One of the user's open pull requests or issues in a project (ThreadRow).
export interface ThreadRow {
  /// `pr` or `issue`.
  kind: string;
  repo: string;
  number: number;
  title: string;
  url: string;
  /// Who wrote last after the user, and what.
  last_other: string;
  needs_reply: boolean;
  /// In milliseconds since the Unix epoch.
  updated_at: number;
}

/// An open pull request of the project's, anyone's (PullRequestRow).
export interface PullRequestRow {
  number: number;
  title: string;
  url: string;
  author: string;
  draft: boolean;
  /// Opened by the account the host's CLI is signed in as.
  mine: boolean;
  /// In milliseconds since the Unix epoch.
  created_at: number;
  /// In milliseconds since the Unix epoch.
  updated_at: number;
}

/// A chat about one thread (ThreadChatRow).
export interface ThreadChatRow {
  chat_id: string;
  /// `pr` or `issue`.
  kind: string;
  repo: string;
  number: number;
  title: string;
}

/// The branch a pull request is made from (PullRequestHead).
export interface PullRequestHead {
  number: number;
  branch: string;
  /// The branch is in a fork, so `origin` lacks it.
  fork: boolean;
}

/// A task as the sections read it.
export type TaskLink = Pick<TaskRow, "id" | "title" | "state" | "pr_url" | "issue_number" | "issue_url" | "branch">;

/// A branch review as the pull request rows read it.
export type PrReview = Pick<ReviewView, "branch"> & Parameters<typeof reviewState>[0];

/// The rows a section shows before Show all.
export const SHOWN = 10;

/// "#52", "PR #41".
export function threadCode(t: Pick<ThreadRow, "kind" | "number">) {
  return t.kind === "pr" ? `PR #${t.number}` : `#${t.number}`;
}

/// The Issues tile from the open issues no task tracks; undefined while
/// they load, null from a server too old to list them. `error` is why the
/// host could not be read, shown in place of an empty list.
export function issueTile(list: readonly Pick<IssueRow, "number" | "title" | "url" | "labels" | "comments">[] | null | undefined, error = ""): TileData {
  if (list === undefined) return { count: "", rows: [], empty: "Loading…" };
  if (list === null) return { count: "", rows: [], empty: "The server lists no issues yet" };
  if (error && !list.length) return { count: "", rows: [], empty: error };
  return {
    count: list.length ? `${list.length} untracked` : "",
    rows: list.map(i => ({ code: `#${i.number}`, text: i.title, url: i.url, note: i.labels?.[0] || (i.comments ? `${i.comments} comments` : "") })),
    empty: "No untracked issues",
  };
}

/// An open PR as `threadTile` reads it.
type OpenPr = Pick<PullRequestRow, "number" | "title" | "url" | "author" | "draft" | "mine" | "created_at">;

/// The Pull requests tile. With the project's open PRs (`open`): the threads
/// waiting on the user's reply first, then every other open PR newest
/// first, someone else's noting its author. Without them (loading, or a
/// server too old to list them), the user's threads: their open PRs, those
/// waiting on a reply first; `fallback` (the tasks' PRs) while those load
/// or on a server too old to list them.
export function threadTile(
  list: readonly (Pick<ThreadRow, "kind" | "number" | "title" | "needs_reply"> & Partial<Pick<ThreadRow, "url">>)[] | null | undefined,
  fallback: TileData,
  open?: readonly OpenPr[] | null,
): TileData {
  if (Array.isArray(open)) {
    const threads = Array.isArray(list) ? list : [];
    const waiting = threads.filter(t => t.needs_reply);
    const waitingPrs = new Set(waiting.filter(t => t.kind === "pr").map(t => t.number));
    const mine = new Set(threads.filter(t => t.kind === "pr").map(t => t.number));
    const byNumber = new Map(open.map(p => [p.number, p]));
    const rest = open.filter(p => !waitingPrs.has(p.number)).sort((a, b) => b.created_at - a.created_at || b.number - a.number);
    /// The author of someone else's PR; a draft of the user's says so.
    const note = (p: OpenPr | undefined) => !p ? "" : p.mine || mine.has(p.number) ? (p.draft ? "draft" : "") : p.author;
    const count = rest.length + waitingPrs.size;
    return {
      count: count ? `${count} open` : "",
      rows: [
        ...waiting.map(t => ({
          code: threadCode(t).replace(/^PR /, ""),
          text: t.title,
          url: t.url,
          chip: { cls: "wait", text: "reply" },
          note: t.kind === "pr" ? note(byNumber.get(t.number)) : "",
        })),
        ...rest.map(p => ({ code: `#${p.number}`, text: p.title, url: p.url, chip: null, note: note(p) })),
      ],
      empty: "No open pull requests",
    };
  }
  if (!Array.isArray(list)) return fallback;
  const prs = list.filter(t => t.kind === "pr");
  const waiting = list.filter(t => t.needs_reply).length;
  return {
    count: waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} reply` : prs.length ? `${prs.length} open` : "",
    rows: [...list.filter(t => t.needs_reply), ...prs.filter(t => !t.needs_reply)].map(t => ({
      code: threadCode(t).replace(/^PR /, ""),
      text: t.title,
      url: t.url,
      chip: t.needs_reply ? { cls: "wait", text: "reply" } : null,
    })),
    empty: fallback?.rows?.length ? fallback.empty : "No open pull requests of yours",
  };
}

/// The line under an issue: who opened it, when, its labels and comments.
export function issueMeta(i: Partial<Pick<IssueRow, "author" | "opened_at" | "labels" | "comments">>, ago: Ago) {
  const out = [`opened by ${i.author || "someone"}`];
  if (i.opened_at) out.push(ago(i.opened_at));
  out.push(...(i.labels || []));
  if (i.comments) out.push(`${i.comments} ${i.comments === 1 ? "comment" : "comments"}`);
  return out.join(" · ");
}

/// The line under a thread: who wrote last, else that nothing waits.
export function threadMeta(t: Pick<ThreadRow, "last_other" | "updated_at" | "needs_reply">, ago: Ago) {
  const out = [];
  if (t.last_other) out.push(t.last_other);
  if (t.updated_at) out.push(ago(t.updated_at));
  if (!t.needs_reply) out.push("your last comment is the newest");
  return out.join(" · ");
}

/// The number in a pull request's address, or null.
export function prNumber(url: string | null | undefined) {
  const n = String(url || "").match(/\/(?:pull|pulls)\/(\d+)/);
  return n ? Number(n[1]) : null;
}

/// One row of the Pull requests section.
export interface PrLine {
  pr: PullRequestRow;
  /// The project task that opened it.
  task: TaskLink | null;
  /// Its branch, when a task or a review names it.
  branch: string | null;
  /// Beep's review of its branch.
  review: ReviewState | null;
  /// What that review read of its checks: `failing`, `pending` or `passing`.
  checks: string | null;
  /// Someone wrote after the user's last comment.
  needs_reply: boolean;
  /// The repository its thread is in, for Let the agent reply.
  repo: string | null;
}

/// Every open pull request with its task, review and whether it waits on
/// the user: those waiting first, then the newest.
export function prLines(
  prs: readonly PullRequestRow[],
  tasks: readonly TaskLink[] | null | undefined,
  threads: readonly Pick<ThreadRow, "kind" | "number" | "needs_reply" | "repo">[] | null | undefined,
  reviews: readonly PrReview[] | null | undefined,
): PrLine[] {
  const lines = prs.map(pr => {
    const task = (tasks || []).find(t => t.state !== "dropped" && (t.pr_url === pr.url || prNumber(t.pr_url) === pr.number)) || null;
    const r = (reviews || []).find(x => x.pr?.number === pr.number) || (task?.branch ? (reviews || []).find(x => x.branch === task.branch) : undefined);
    const thread = (threads || []).find(t => t.kind === "pr" && t.number === pr.number);
    const checks = r?.pr?.checks && r.pr.checks !== "none" ? r.pr.checks : null;
    return {
      pr,
      task,
      branch: task?.branch || r?.branch || null,
      review: r ? reviewState(r) : null,
      checks,
      needs_reply: !!thread?.needs_reply,
      repo: thread?.repo || null,
    };
  });
  return lines.sort((a, b) => Number(b.needs_reply) - Number(a.needs_reply) || b.pr.created_at - a.pr.created_at || b.pr.number - a.pr.number);
}

const CHECKS: Record<string, string> = { failing: "CI failing", pending: "CI running", passing: "CI passing" };

/// The pieces under a pull request: who opened it, when, its review and
/// checks, and its task.
export function prMeta(l: PrLine, ago: Ago) {
  const out = [l.pr.mine ? "you" : l.pr.author || "someone"];
  if (l.pr.created_at) out.push(ago(l.pr.created_at));
  const review = l.review ? STATE_LABEL[l.review].toLowerCase() : "";
  const checks = l.checks ? CHECKS[l.checks] || "" : "";
  if (review || checks) out.push([review, checks].filter(Boolean).join(" · "));
  if (l.task) out.push(`task: ${l.task.title}`);
  return out;
}

/// A pull request row's chip: a reply it waits on first, then its review,
/// then whether it is a draft.
export function prChip(l: PrLine): { cls: string; text: string } | null {
  if (l.needs_reply) return { cls: "blocked", text: "needs reply" };
  if (l.review === "running") return { cls: "running", text: "reviewing" };
  if (l.review === "blocked") return { cls: "blocked", text: "blocked" };
  if (l.review === "changes") return { cls: "blocked", text: "needs changes" };
  if (l.review === "ready") return { cls: "done", text: "ready" };
  if (l.pr.draft) return { cls: "", text: "draft" };
  return null;
}

/// One row of the Issues section: an open issue no task tracks, or a
/// task's issue.
export interface IssueLine {
  number: number;
  title: string;
  url: string;
  /// The open issue, when no task tracks it.
  issue: IssueRow | null;
  task: TaskLink | null;
}

/// The open issues no task tracks, then the issues of the tasks still
/// going (not done or dropped).
export function issueLines(issues: readonly IssueRow[], tasks: readonly TaskLink[] | null | undefined): IssueLine[] {
  const open = new Set(issues.map(i => i.number));
  const tracked = (tasks || []).filter((t): t is TaskLink & { issue_number: number } =>
    t.issue_number != null && t.state !== "done" && t.state !== "dropped" && !open.has(t.issue_number));
  const seen = new Set<number>();
  return [
    ...issues.map(i => ({ number: i.number, title: i.title, url: i.url, issue: i, task: null })),
    ...tracked.filter(t => !seen.has(t.issue_number) && seen.add(t.issue_number)).map(t => ({
      number: t.issue_number,
      title: t.title,
      url: t.issue_url || "",
      issue: null,
      task: t,
    })),
  ];
}

/// The Issues section's count line: "14 open · 6 not tracked".
export function issueCount(lines: readonly Pick<IssueLine, "task">[]) {
  if (!lines.length) return "";
  const untracked = lines.filter(l => !l.task).length;
  return untracked === lines.length ? `${lines.length} open, none tracked` : `${lines.length} open · ${untracked} not tracked`;
}

/// The rows a section shows: all of them once Show all was clicked, else
/// the first `SHOWN`; `hidden` counts the rest.
export function firstRows<T>(rows: readonly T[], all: boolean): { rows: T[]; hidden: number } {
  if (all || rows.length <= SHOWN) return { rows: [...rows], hidden: 0 };
  return { rows: rows.slice(0, SHOWN), hidden: rows.length - SHOWN };
}

/// The longest slug of a title an issue's branch takes.
const BRANCH_SLUG = 30;

/// The branch Work on it suggests: "issue-52-app-crashes-when-the-phone",
/// the title's words cut at a word to fit `BRANCH_SLUG`.
export function issueBranch(number: number, title: string) {
  let slug = String(title || "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  if (slug.length > BRANCH_SLUG) {
    const cut = slug.slice(0, BRANCH_SLUG);
    const at = cut.lastIndexOf("-");
    slug = at > 0 ? cut.slice(0, at) : cut;
  }
  return slug ? `issue-${number}-${slug}` : `issue-${number}`;
}

/// What Work on it puts in the start form.
export function issuePrompt(i: Pick<IssueRow, "number" | "title" | "url">) {
  return `Work on issue #${i.number}: ${i.title} (${i.url}). Read the issue and its comments first, then propose and implement a fix.`;
}

/// What Continue puts in the start form.
export function prPrompt(p: Pick<PullRequestRow, "number" | "title" | "url">) {
  return `Continue PR #${p.number}: ${p.title} (${p.url}). Read the review comments and failing checks first.`;
}

/// What the project page hands the Pull requests and Issues sections: the
/// project's tasks and branch reviews, and what Review, Work on it and
/// Continue do. `continuePr` may fail, saying why, in the row.
export interface RepoActs {
  tasks: readonly TaskLink[];
  reviews: readonly PrReview[] | null | undefined;
  review?: (branch: string) => void;
  workOn: (issue: IssueRow) => void;
  continuePr: (pr: PullRequestRow) => Promise<void>;
}

/// What the window hands `makeThreads`. `openChat(chatId)` shows a chat in
/// the pane; `changed()` reloads the project's tasks once an issue became one.
export interface ThreadDeps {
  invoke: typeof Invoke;
  h: MakeEl;
  button: MakeButton;
  ago: Ago;
  redraw: () => void;
  openUrl: (url: string) => unknown;
  openChat: (chat: string) => void;
  changed: () => unknown;
}

/// One project's lists: undefined while they load, null from a server too
/// old to give them. `issuesError` and `prsError` say why the host could
/// not be read; `failed` is why a row's last action failed, by row key;
/// `all` the sections shown in full.
interface ProjectThreads {
  issues: IssueRow[] | null | undefined;
  threads: ThreadRow[] | null | undefined;
  prs: PullRequestRow[] | null | undefined;
  chats: ThreadChatRow[] | null | undefined;
  error: string;
  issuesError: string;
  prsError: string;
  busy: Set<string>;
  failed: Map<string, string>;
  all: Set<string>;
}

/// The project agent the lists belong to.
type Agent = Pick<ProjectAgent, "project_id" | "name">;

export function makeThreads(deps: ThreadDeps) {
  const { invoke, h, button, ago, redraw, openUrl, openChat, changed } = deps;
  const all = new Map<string, ProjectThreads>();

  const of = (id: string) => {
    let s = all.get(id);
    if (!s) {
      s = { issues: undefined, threads: undefined, prs: undefined, chats: undefined, error: "", issuesError: "", prsError: "", busy: new Set(), failed: new Map(), all: new Set() };
      all.set(id, s);
    }
    return s;
  };

  async function load(id: string) {
    const s = of(id);
    const [issues, threads, chats, prs] = await Promise.allSettled([
      invoke<IssueRow[] | null>("center_project_issues", { projectId: id }),
      invoke<ThreadRow[] | null>("center_project_threads", { projectId: id }),
      invoke<ThreadChatRow[] | null>("center_thread_chats", { projectId: id }),
      invoke<PullRequestRow[] | null>("center_project_pull_requests", { projectId: id }),
    ]);
    s.error = "";
    /// What a list becomes: what came back, else what it had or none.
    const settle = <T>(r: PromiseSettledResult<T[] | null>, had: T[] | null | undefined) => {
      if (r.status === "fulfilled") return r.value;
      s.error = reason(r.reason);
      return had ?? [];
    };
    s.threads = settle(threads, s.threads);
    s.chats = settle(chats, s.chats);
    // Why the host could not be read shows in each section, in place of an
    // empty list.
    s.issuesError = issues.status === "rejected" ? reason(issues.reason) : "";
    s.issues = issues.status === "fulfilled" ? issues.value : s.issues ?? [];
    s.prsError = prs.status === "rejected" ? reason(prs.reason) : "";
    // Without the list the tile shows the user's own threads as before.
    s.prs = prs.status === "fulfilled" ? prs.value : s.prs ?? null;
    redraw();
  }

  /// Runs `work` for row `key`, showing it busy and its error.
  async function act(id: string, key: string, work: () => Promise<void>) {
    const s = of(id);
    if (s.busy.has(key)) return;
    s.busy.add(key);
    s.error = "";
    s.failed.delete(key);
    redraw();
    try {
      await work();
    } catch (e) {
      s.error = reason(e);
      s.failed.set(key, s.error);
    } finally {
      s.busy.delete(key);
      redraw();
    }
  }

  const discuss = (agent: Agent, number: number) => act(agent.project_id, `d${number}`, async () => {
    const chat = await invoke<ThreadChatRow>("center_open_thread_chat", { projectId: agent.project_id, repo: agent.name, number });
    const s = of(agent.project_id);
    s.chats = [chat, ...(s.chats || []).filter(c => c.chat_id !== chat.chat_id)];
    openChat(chat.chat_id);
  });

  /// Makes issue `number` a task for the project agent to plan.
  function track(agent: Agent, number: number) {
    return act(agent.project_id, `i${number}`, async () => {
      await invoke("center_track_issue", { projectId: agent.project_id, number, agent: null, nodeId: null });
      const s = of(agent.project_id);
      s.issues = (s.issues || []).filter(i => i.number !== number);
      changed();
    });
  }

  function dismiss(agent: Agent, number: number) {
    return act(agent.project_id, `i${number}`, async () => {
      await invoke("center_dismiss_issue", { projectId: agent.project_id, number });
      const s = of(agent.project_id);
      s.issues = (s.issues || []).filter(i => i.number !== number);
    });
  }

  function link(text: string, url: string) {
    const a = h("a", "", text);
    a.href = "#";
    a.addEventListener("click", e => { e.preventDefault(); openUrl(url); });
    return a;
  }

  /// A section of the project page: its heading with the count, then the
  /// error, the loading line or its rows, with Show all past `SHOWN`.
  function section(agent: Agent, id: string, title: string, count: string, body: HTMLElement[], note?: HTMLElement) {
    const box = h("div");
    box.id = id;
    box.append(h("div", "cc-sec", h("h4", "", title), count ? h("small", "", count) : null, button("Refresh", "quiet small", () => load(agent.project_id))), ...body);
    if (note) box.append(note);
    return box;
  }

  /// The section's note in place of its rows: why the host could not be
  /// read, that it loads, or that there is none.
  function emptyNote(text: string, bad = false) {
    return h("div", "cc-panel cc-empty", h("span", bad ? "cc-err" : "", text));
  }

  /// The rows, the first `SHOWN` until Show all.
  function rowsPanel<T>(agent: Agent, key: string, lines: readonly T[], draw: (line: T) => HTMLElement) {
    const s = of(agent.project_id);
    const { rows, hidden } = firstRows(lines, s.all.has(key));
    const out = rows.map(draw);
    if (hidden) {
      out.push(h("div", "pj-task pj-more", h("span", "t dmuted", `${hidden} more`),
        h("span", "acts", button(`Show all ${lines.length}`, "quiet small", () => { s.all.add(key); redraw(); }))));
    } else if (s.all.has(key) && lines.length > SHOWN) {
      out.push(h("div", "pj-task pj-more", h("span", "t"), h("span", "acts", button("Show fewer", "quiet small", () => { s.all.delete(key); redraw(); }))));
    }
    return h("div", "cc-panel", ...out);
  }

  /// A row's actions and why its last one failed.
  function actsEl(s: ProjectThreads, keys: string[], buttons: HTMLButtonElement[]) {
    const err = keys.map(k => s.failed.get(k)).find(Boolean);
    return h("span", "acts", ...buttons, err ? h("span", "cc-err", err) : null);
  }

  /// The Pull requests section: every open PR of the repository, with its
  /// author, age, review, checks and task, Let the agent reply when someone
  /// wrote after the user, Continue, Review and Open.
  function prsSection(agent: Agent, ctx: RepoActs) {
    const s = of(agent.project_id);
    if (s.prs === undefined) load(agent.project_id);
    const list = s.prs;
    const title = "Pull requests";
    if (s.prsError && !list?.length) return section(agent, "sec-prs", title, "", [emptyNote(s.prsError, true)]);
    if (list === undefined) return section(agent, "sec-prs", title, "", [emptyNote("Reading the pull requests…")]);
    if (list === null) return section(agent, "sec-prs", title, "", [emptyNote("The server needs an update before it lists pull requests.")]);
    const lines = prLines(list, ctx.tasks, s.threads, ctx.reviews);
    const waiting = lines.filter(l => l.needs_reply).length;
    const count = lines.length ? `${lines.length} open${waiting ? ` · ${waiting} ${waiting === 1 ? "needs" : "need"} your reply` : ""}` : "";
    if (!lines.length) return section(agent, "sec-prs", title, "", [emptyNote("No open pull requests.")]);
    const draw = (l: PrLine) => {
      const n = l.pr.number;
      const chip = prChip(l);
      const meta = h("span", "m", ...prMeta(l, ago).map(t => h("span", "", t)));
      const buttons: HTMLButtonElement[] = [];
      if (l.needs_reply) {
        buttons.push(button(s.busy.has(`r${n}`) ? "Asking…" : "Let the agent reply", "pri", () => act(agent.project_id, `r${n}`, async () => {
          openChat(await invoke<string>("center_reply_thread", { projectId: agent.project_id, repo: l.repo || agent.name, number: n }));
        })));
      }
      const cont = button(s.busy.has(`c${n}`) ? "Reading…" : "Continue", l.needs_reply ? "" : "pri", () => act(agent.project_id, `c${n}`, () => ctx.continuePr(l.pr)));
      cont.disabled = s.busy.has(`c${n}`);
      cont.title = "Start a coding agent on this pull request's branch";
      buttons.push(cont);
      if (l.branch && ctx.review) buttons.push(button("Review", "", () => ctx.review!(l.branch!)));
      buttons.push(button("Open", "", () => openUrl(l.pr.url)));
      const row = h("div", "pj-task pj-issue", h("span", "t", link(`PR #${n}`, l.pr.url), ` ${l.pr.title}`),
        chip ? h("span", `pj-st ${chip.cls}`, chip.cls === "running" ? h("span", "dspin") : null, chip.text) : null,
        meta, actsEl(s, [`r${n}`, `c${n}`], buttons));
      row.title = l.pr.title;
      return row;
    };
    const body: HTMLElement[] = [];
    if (s.prsError) body.push(h("div", "cc-err", s.prsError));
    body.push(rowsPanel(agent, "prs", lines, draw));
    return section(agent, "sec-prs", title, count, body);
  }

  /// The Issues section: every open issue, one no task tracks with Work on
  /// it, Plan, Discuss and Not now, a task's with its task.
  function issuesSection(agent: Agent, ctx: RepoActs) {
    const s = of(agent.project_id);
    if (s.issues === undefined) load(agent.project_id);
    const list = s.issues;
    const title = "Issues";
    if (s.issuesError && !list?.length) return section(agent, "sec-issues", title, "", [emptyNote(s.issuesError, true)]);
    if (list === undefined) return section(agent, "sec-issues", title, "", [emptyNote("Reading the issues…")]);
    if (list === null) return section(agent, "sec-issues", title, "", [emptyNote("The server needs an update before it lists issues.")]);
    const lines = issueLines(list, ctx.tasks);
    if (!lines.length) return section(agent, "sec-issues", title, "", [emptyNote("No open issues.")]);
    const draw = (l: IssueLine) => {
      const n = l.number;
      const code = l.url ? link(`#${n}`, l.url) : h("span", "", `#${n}`);
      if (l.task) {
        const t = l.task;
        return h("div", "pj-task pj-issue", h("span", "t", code, ` ${l.title}`),
          h("span", `pj-st ${t.state}`, t.state === "running" ? h("span", "dspin") : null, t.state),
          h("span", "m", h("span", "", `task: ${t.title}`), t.pr_url ? link(prLabel(t.pr_url), t.pr_url) : null));
      }
      const i = l.issue!;
      const busy = s.busy.has(`i${n}`);
      const buttons = [
        button("Work on it", "pri", () => ctx.workOn(i)),
        button("Plan", "", () => track(agent, n)),
        button(s.busy.has(`d${n}`) ? "Opening…" : "Discuss", "", () => discuss(agent, n)),
        button("Not now", "", () => dismiss(agent, n)),
      ];
      for (const b of buttons) b.disabled = busy;
      buttons[0].title = "Start a coding agent on a new branch for this issue";
      return h("div", "pj-task pj-issue", h("span", "t", code, ` ${i.title}`), h("span", "m", issueMeta(i, ago)), actsEl(s, [`i${n}`, `d${n}`], buttons));
    };
    const body: HTMLElement[] = [];
    if (s.issuesError) body.push(h("div", "cc-err", s.issuesError));
    body.push(rowsPanel(agent, "issues", lines, draw));
    return section(agent, "sec-issues", title, issueCount(lines), body,
      h("small", "dmuted", "Not now hides an issue here; it stays open on the host. Issue text is the author's, and the agent treats it as data."));
  }

  /// The user's pull requests and issues, with replies and chats.
  function threadsPanel(agent: Agent) {
    const s = of(agent.project_id);
    if (s.threads === undefined) load(agent.project_id);
    const list = s.threads;
    const out: HTMLElement[] = [];
    if (list === undefined) return [h("div", "cc-panel cc-empty", h("span", "", "Reading your pull requests and issues…"))];
    if (list === null) return [];
    if (s.error) out.push(h("div", "cc-err", s.error));
    const waiting = list.filter(t => t.needs_reply).length;
    out.push(h("div", "cc-sec", h("h4", "", "Your pull requests and issues", h("small", "", waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} a reply` : "")),
      button("Refresh", "quiet small", () => load(agent.project_id))));
    if (!list.length) {
      out.push(h("div", "cc-panel cc-empty", h("span", "", "You have no open pull requests or issues here.")));
    } else {
      out.push(h("div", "cc-panel", ...list.map(t => {
        const st = t.needs_reply ? h("span", "pj-st blocked", "needs reply") : h("span", "pj-st done", "nothing to answer");
        const row = h("div", "pj-task pj-issue", h("span", "t", link(threadCode(t), t.url), ` ${t.title}`), st, h("span", "m", threadMeta(t, ago)));
        const acts = h("span", "acts");
        if (t.needs_reply) {
          acts.append(button(s.busy.has(`r${t.number}`) ? "Asking…" : "Let the agent reply", "pri", () => act(agent.project_id, `r${t.number}`, async () => {
            openChat(await invoke<string>("center_reply_thread", { projectId: agent.project_id, repo: t.repo, number: t.number }));
          })));
        }
        acts.append(button(s.busy.has(`d${t.number}`) ? "Opening…" : "Discuss", "", () => discuss(agent, t.number)),
          button("Open", "", () => openUrl(t.url)));
        row.append(acts);
        return row;
      })));
      out.push(h("small", "dmuted", "The agent drafts the reply and nothing posts until you approve its card."));
    }
    if (s.chats?.length) {
      out.push(h("div", "cc-sec", h("h4", "", "Chats about one thread")),
        h("div", "cc-panel", ...s.chats.map(c => {
          const row = h("div", "pj-task", h("span", "t", c.title || threadCode(c)));
          row.append(h("span", "acts", button("Open chat", "", () => openChat(c.chat_id))));
          return row;
        })));
    }
    return out;
  }

  return {
    load,
    issues: (id: string) => of(id).issues,
    issuesError: (id: string) => of(id).issuesError,
    threads: (id: string) => of(id).threads,
    prs: (id: string) => of(id).prs,
    prsSection,
    issuesSection,
    threadsPanel,
  };
}
