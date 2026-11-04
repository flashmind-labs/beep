// A project's issues and pull requests, and the user's threads there.
import type { invoke as Invoke } from "../ui/tauri.ts";
import { reason } from "../addproject/importproject-state.ts";
import type { Ago, MakeButton, MakeEl } from "../ui/dom.ts";
import type { ProjectAgent, TaskRow } from "./agent.ts";
import { STATE_LABEL, reviewState } from "../review/review.ts";
import type { ReviewState, ReviewView } from "../review/review.ts";
import { plural } from "../ui/format.ts";
import { emptyPanel, freshNote, stateChip, secHead } from "../ui/parts.ts";
import { PageCache, REFRESH_MS, freshness, type SectionStore } from "./pagecache.ts";
import type { ChecksRow } from "./runs.ts";

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
  /// Missing from a list kept by an older app.
  labels?: string[];
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

/// "#52", "PR #41".
export function threadCode(t: Pick<ThreadRow, "kind" | "number">) {
  return t.kind === "pr" ? `PR #${t.number}` : `#${t.number}`;
}

/// The line under an issue: who opened it, when, and its comments.
export function issueMeta(i: Partial<Pick<IssueRow, "author" | "opened_at" | "comments">>, ago: Ago) {
  const out = [`opened by ${i.author || "someone"}`];
  if (i.opened_at) out.push(ago(i.opened_at));
  if (i.comments) out.push(plural(i.comments, "comment", "comments"));
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

/// The project's lists as the server keeps them (ListsRow).
export interface ListsRow {
  prs: PullRequestRow[];
  prs_error: string | null;
  issues: IssueRow[];
  issues_error: string | null;
  /// In milliseconds since the Unix epoch; null before the first read.
  synced_at: number | null;
}

/// What the project page hands the Pull requests and Issues sections: the
/// project's tasks and branch reviews, each PR's checks from its workflow runs,
/// and what Review, Work on it, Continue and a checks link do.
export interface RepoActs {
  tasks: readonly TaskLink[];
  reviews: readonly PrReview[] | null | undefined;
  checks?: readonly ChecksRow[] | null;
  showRun?: (run: number, host?: string | null) => void;
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
  /// Where the lists are kept between windows; in memory only without it.
  kept?: SectionStore<KeptThreads>;
}

/// A project's lists as the page cache keeps them.
export interface KeptThreads {
  issues: IssueRow[] | null;
  threads: ThreadRow[] | null;
  prs: PullRequestRow[] | null;
  chats: ThreadChatRow[] | null;
}

/// One project's lists: undefined while they load, null from a server too old
/// to give them.
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
  /// The labels the Issues section is filtered to; none shows every issue.
  labels: Set<string>;
  /// When the server last read the lists from the host; null when unknown.
  syncedAt: number | null;
}

/// The project agent the lists belong to.
type Agent = Pick<ProjectAgent, "project_id" | "name">;

export function makeThreads(deps: ThreadDeps) {
  const { invoke, h, button, ago, redraw, openUrl, openChat, changed } = deps;
  const kept = deps.kept ?? new PageCache(null).section<KeptThreads>("threads");
  const all = new Map<string, ProjectThreads>();

  /// A project's lists, from the cache until they are read.
  const of = (id: string) => {
    let s = all.get(id);
    if (!s) {
      const k = kept.get(id);
      s = { issues: k?.issues, threads: k?.threads, prs: k?.prs, chats: k?.chats, error: "", issuesError: "", prsError: "", busy: new Set(), failed: new Map(), all: new Set(), labels: new Set(), syncedAt: null };
      all.set(id, s);
    }
    return s;
  };

  /// Projects whose read is on its way (a load meanwhile is dropped), and when
  /// each was last tried, so a failing host is not asked on every draw.
  const reading = new Set<string>();
  const tried = new Map<string, number>();

  /// Reads `id` when it is due: never read, or `every` passed since the last
  /// try (the page cache's refresh time by default; opening the project asks
  /// sooner).
  function ensure(id: string, every = REFRESH_MS): Promise<void> | undefined {
    const last = tried.get(id);
    const due = last === undefined ? kept.due(id, every) : Date.now() - last >= every;
    if (due && !reading.has(id)) return load(id);
  }

  /// The section header's note: a spinner while kept lists read again,
  /// else how old they are: when the server synced them from the host, or
  /// on a server too old to say, when this page read them.
  const fresh = (id: string) => {
    const synced = of(id).syncedAt;
    const f = freshness(synced ?? kept.at(id), reading.has(id), Date.now());
    return freshNote(synced ? { ...f, text: f.text.replace(/^updated/, "synced") } : f);
  };

  /// The open issues and pull requests: from the server's copy, or read from
  /// the host each time on a server too old to keep one.
  async function lists(id: string, refresh: boolean): Promise<[PromiseSettledResult<IssueRow[] | null>, PromiseSettledResult<PullRequestRow[] | null>]> {
    const s = of(id);
    const copy = await invoke<ListsRow | null>("center_project_lists", { projectId: id, refresh });
    if (copy) {
      s.syncedAt = copy.synced_at;
      return [
        copy.issues_error ? { status: "rejected", reason: copy.issues_error } : { status: "fulfilled", value: copy.issues },
        copy.prs_error ? { status: "rejected", reason: copy.prs_error } : { status: "fulfilled", value: copy.prs },
      ];
    }
    s.syncedAt = null;
    return Promise.allSettled([
      invoke<IssueRow[] | null>("center_project_issues", { projectId: id }),
      invoke<PullRequestRow[] | null>("center_project_pull_requests", { projectId: id }),
    ]);
  }

  async function load(id: string, refresh = false) {
    const s = of(id);
    if (reading.has(id)) return;
    reading.add(id);
    tried.set(id, Date.now());
    // Kept lists stay up meanwhile, with a spinner in their header.
    if (s.issues !== undefined) queueMicrotask(redraw);
    const [both, threads, chats] = await Promise.allSettled([
      lists(id, refresh),
      invoke<ThreadRow[] | null>("center_project_threads", { projectId: id }),
      invoke<ThreadChatRow[] | null>("center_thread_chats", { projectId: id }),
    ]);
    reading.delete(id);
    const [issues, prs] = both.status === "fulfilled" ? both.value : [both, both] as const;
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
    // Kept once the host answered; a failed read keeps the last good lists.
    if (issues.status === "fulfilled" && prs.status === "fulfilled" && !s.error) {
      kept.put(id, { issues: s.issues ?? null, threads: s.threads ?? null, prs: s.prs ?? null, chats: s.chats ?? null });
    }
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

  function link(text: string, url: string) {
    const a = h("a", "", text);
    a.href = "#";
    a.addEventListener("click", e => { e.preventDefault(); openUrl(url); });
    return a;
  }

  /// Let the agent reply on thread `number`.
  const replyButton = (agent: Agent, repo: string, number: number) =>
    button(of(agent.project_id).busy.has(`r${number}`) ? "Asking…" : "Let the agent reply", "pri", () => act(agent.project_id, `r${number}`, async () => {
      openChat(await invoke<string>("center_reply_thread", { projectId: agent.project_id, repo, number }));
    }));

  /// A row's actions and why its last one failed.
  function actsEl(s: ProjectThreads, keys: string[], buttons: HTMLButtonElement[]) {
    const err = keys.map(k => s.failed.get(k)).find(Boolean);
    return h("span", "acts", ...buttons, err ? h("span", "cc-err", err) : null);
  }

  /// A pull request's buttons on the project page: Let the agent reply when
  /// someone wrote after the user, Continue and Review, and why the last one
  /// failed.
  function prButtons(agent: Agent, ctx: RepoActs, l: PrLine) {
    const s = of(agent.project_id);
    const n = l.pr.number;
    const buttons: HTMLButtonElement[] = [];
    if (l.needs_reply) buttons.push(replyButton(agent, l.repo || agent.name, n));
    const cont = button(s.busy.has(`c${n}`) ? "Reading…" : "Continue", l.needs_reply ? "" : "pri", () => act(agent.project_id, `c${n}`, () => ctx.continuePr(l.pr)));
    cont.disabled = s.busy.has(`c${n}`);
    cont.title = "Start a coding agent on this pull request's branch";
    buttons.push(cont);
    if (l.branch && ctx.review) buttons.push(button("Review", "", () => ctx.review!(l.branch!)));
    return actsEl(s, [`r${n}`, `c${n}`], buttons);
  }

  /// An open issue's buttons: Work on it, Plan and Discuss.
  function issueButtons(agent: Agent, ctx: RepoActs, i: IssueRow) {
    const s = of(agent.project_id);
    const n = i.number;
    const buttons = [
      button("Work on it", "pri", () => ctx.workOn(i)),
      button("Plan", "", () => track(agent, n)),
      button(s.busy.has(`d${n}`) ? "Opening…" : "Discuss", "", () => discuss(agent, n)),
    ];
    for (const b of buttons) b.disabled = s.busy.has(`i${n}`);
    buttons[0].title = "Start a coding agent on a new branch for this issue";
    return actsEl(s, [`i${n}`, `d${n}`], buttons);
  }

  /// The user's pull requests and issues, with replies and chats.
  function threadsPanel(agent: Agent) {
    const s = of(agent.project_id);
    ensure(agent.project_id);
    const list = s.threads;
    const out: HTMLElement[] = [];
    if (list === undefined) return [emptyPanel(h("span", "", "Reading your pull requests and issues…"))];
    if (list === null) return [];
    if (s.error) out.push(h("div", "cc-err", s.error));
    const waiting = list.filter(t => t.needs_reply).length;
    out.push(h("div", "cc-sec", h("h4", "", "Your pull requests and issues", h("small", "", waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} a reply` : "")),
      fresh(agent.project_id), button("Refresh", "quiet small", () => load(agent.project_id))));
    if (!list.length) {
      out.push(emptyPanel(h("span", "", "You have no open pull requests or issues here.")));
    } else {
      out.push(h("div", "cc-panel", ...list.map(t => {
        const st = t.needs_reply ? stateChip("blocked", "needs reply") : stateChip("done", "nothing to answer");
        const row = h("div", "pj-task pj-issue", h("span", "t", link(threadCode(t), t.url), ` ${t.title}`), st, h("span", "m", threadMeta(t, ago)));
        const acts = h("span", "acts");
        if (t.needs_reply) acts.append(replyButton(agent, t.repo, t.number));
        acts.append(button(s.busy.has(`d${t.number}`) ? "Opening…" : "Discuss", "", () => discuss(agent, t.number)),
          button("Open", "", () => openUrl(t.url)));
        row.append(acts);
        return row;
      })));
    }
    if (s.chats?.length) {
      out.push(secHead("Chats about one thread"),
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
    ensure,
    loading: (id: string) => reading.has(id),
    issues: (id: string) => of(id).issues,
    issuesError: (id: string) => of(id).issuesError,
    threads: (id: string) => of(id).threads,
    prs: (id: string) => of(id).prs,
    syncedAt: (id: string) => of(id).syncedAt,
    prsError: (id: string) => of(id).prsError,
    /// Every open pull request with its task, review and reply, waiting first.
    prLines: (agent: Agent, ctx: RepoActs) => prLines(of(agent.project_id).prs || [], ctx.tasks, of(agent.project_id).threads, ctx.reviews),
    /// Every open issue, with the task that tracks it.
    issueLines: (agent: Agent, ctx: RepoActs) => issueLines(of(agent.project_id).issues || [], ctx.tasks),
    prButtons,
    issueButtons,
    threadsPanel,
  };
}
