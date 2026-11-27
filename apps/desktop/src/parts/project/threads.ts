// A project's open issues and pull requests.
import type { invoke as Invoke } from "../../ui/tauri.ts";
import type { Ago } from "../../ui/format.ts";
import { plural, reason } from "../../ui/format.ts";
import { openMenu, type MenuItem } from "../../ui/menu.tsx";
import { PageCache, REFRESH_MS, type SectionStore } from "../../ui/pagecache.ts";
import type { Pushed } from "../../ui/snapshot-state.ts";
import type { ThreadAction, ThreadActions } from "../../screens/project/threaddetail-view.tsx";
import type { AgentState, PickButton } from "../review/feedback-view.tsx";
import type { ProjectPick } from "../../stores/agents/agentmodels.ts";
import { AGENT_ACTIVE } from "../../stores/agents/sessionmenu-state.ts";
import { agentLabel } from "../../stores/project/projects-state.ts";
import type { TaskSession } from "../../stores/project/projectbar.ts";
import type { SessionState } from "./composer.tsx";
import type { ProjectRef } from "../../stores/project/localid.ts";
import { sameLogin } from "../../stores/project/team-state.ts";

/// An open issue (IssueRow).
export interface IssueRow {
  number: number;
  title: string;
  labels: string[];
  author: string;
  /// In milliseconds since the Unix epoch.
  opened_at: number;
  comments: number;
  url: string;
  /// A pull request is linked to it, to close it. Missing from a list kept by an older app.
  linked_pr?: boolean;
  /// The forge logins it is assigned to.
  assignees: string[];
}

/// An issue as the desktop sends it or an older app kept it, maybe without its assignees.
type ReadIssue = Omit<IssueRow, "assignees"> & { assignees?: string[] };

/// `issues` with their assignees, none where a list kept by an older app has no field for them.
const withAssignees = (issues: readonly ReadIssue[]): IssueRow[] => issues.map(issue => ({ ...issue, assignees: issue.assignees ?? [] }));

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
  /// Missing from a list kept by an older app.
  labels?: string[];
}

/// The branch a pull request is made from (PullRequestHead).
export interface PullRequestHead {
  number: number;
  branch: string;
  /// The branch is in a fork, so `origin` lacks it.
  fork: boolean;
}

/// The line under an issue: who opened it, when, and its comments.
export function issueMeta(i: Partial<Pick<IssueRow, "author" | "opened_at" | "comments">>, ago: Ago) {
  const out = [`opened by ${i.author || "someone"}`];
  if (i.opened_at) out.push(ago(i.opened_at));
  if (i.comments) out.push(plural(i.comments, "comment", "comments"));
  return out.join(" · ");
}

/// Every open pull request, the newest first.
export function sortedPrs(prs: readonly PullRequestRow[]): PullRequestRow[] {
  return [...prs].sort((a, b) => b.created_at - a.created_at || b.number - a.number);
}

/// The pieces under a pull request: who opened it, and when.
export function prMeta(pr: PullRequestRow, ago: Ago) {
  const out = [pr.mine ? "you" : pr.author || "someone"];
  if (pr.created_at) out.push(ago(pr.created_at));
  return out;
}

/// What Work on it puts in the bar for an issue.
export function issuePrompt(issue: Pick<IssueRow, "number" | "title" | "url">) {
  return `Work on issue #${issue.number}: ${issue.title} (${issue.url}). Read the issue and its comments first, then propose and implement a fix.`;
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

/// The project's lists as last read (ListsRow).
export interface ListsRow {
  prs: PullRequestRow[];
  prs_error: string | null;
  issues: ReadIssue[];
  issues_error: string | null;
}

/// The thread a discussion starts from.
export type DiscussionTopic = Pick<IssueRow, "number" | "title" | "url"> & { kind: "issue" | "pr" };

/// The project page's thread actions. The menu can fill the bar, and the
/// agent pickers start a session at once.
export interface RepoActs {
  workOn: (issue: IssueRow) => void;
  startIssue: (issue: IssueRow, pick: ProjectPick, machine: string | null) => Promise<void>;
  discuss: (topic: DiscussionTopic) => void;
  workOnPr: (pr: PullRequestRow) => Promise<void>;
  prSession: (url: string, redraw: () => void) => TaskSession | "checking" | null;
  startPrWork: (pr: PullRequestRow, pick: ProjectPick, machine: string | null) => Promise<void>;
  reviewPr: (pr: PullRequestRow) => Promise<void>;
  startReview: (pr: PullRequestRow, pick: ProjectPick, machine: string | null) => Promise<void>;
  /// The agent button's picker, whose pick runs `start`.
  button: (start: (pick: ProjectPick, machine: string | null) => void) => PickButton;
  /// The session that reviewed the PR before, which Open review opens.
  reviewOf: (pr: PullRequestRow) => TaskSession | null;
  openSession: (s: TaskSession) => void;
  /// The session working on the issue, which its button opens.
  issueSession: (issue: IssueRow) => SessionState | null;
  /// The team's logins an issue can be assigned to; none without a team.
  assignees: () => readonly string[];
  assign: (issue: IssueRow, login: string) => Promise<void>;
}

/// What the window hands `makeThreads`.
export interface ThreadDeps {
  invoke: typeof Invoke;
  redraw: () => void;
  openUrl: (url: string) => unknown;
  /// Where the lists are kept between windows; in memory only without it.
  kept?: SectionStore<KeptThreads>;
}

/// A project's lists as the page cache keeps them.
export interface KeptThreads {
  issues: ReadIssue[] | null;
  prs: PullRequestRow[] | null;
}

/// One project's lists: undefined while they load.
interface ProjectThreads {
  issues: IssueRow[] | null | undefined;
  prs: PullRequestRow[] | null | undefined;
  issuesError: string;
  prsError: string;
  busy: Set<string>;
  failed: Map<string, string>;
  /// The agent buttons' pickers, by row key.
  buttons: Map<string, PickButton>;
}

export function makeThreads(deps: ThreadDeps) {
  const { invoke, redraw, openUrl } = deps;
  const kept = deps.kept ?? new PageCache(null).section<KeptThreads>("threads");
  const all = new Map<string, ProjectThreads>();

  /// A project's lists, from the cache until they are read.
  const of = (id: string) => {
    let s = all.get(id);

    if (!s) {
      const k = kept.get(id);
      s = { issues: k === undefined ? undefined : k.issues && withAssignees(k.issues), prs: k?.prs, issuesError: "", prsError: "", busy: new Set(), failed: new Map(), buttons: new Map() };
      all.set(id, s);
    }

    return s;
  };

  /// Projects whose read is on its way (a reload meanwhile runs once after),
  /// and when each was last tried, so a failing host is not asked on every draw.
  const reading = new Set<string>();
  const reloadAgain = new Map<string, boolean>();
  const tried = new Map<string, number>();
  /// How many pushes each project's lists took, so a read out meanwhile does not replace them.
  const pushes = new Map<string, number>();

  /// Reads `id` when it is due: never read, or `every` passed since the last
  /// try (the page cache's refresh time by default; opening the project asks
  /// sooner).
  function ensure(id: string, every = REFRESH_MS): Promise<void> | undefined {
    const last = tried.get(id);
    const due = last === undefined ? kept.due(id, every) : Date.now() - last >= every;
    if (due && !reading.has(id)) return load(id);
  }

  async function load(id: string, refresh = false) {
    if (reading.has(id)) {
      reloadAgain.set(id, (reloadAgain.get(id) ?? false) || refresh);
      return;
    }
    const s = of(id);
    reading.add(id);
    let force = refresh;

    try {
      while (true) {
        tried.set(id, Date.now());
        // Kept lists stay up meanwhile, with a spinner in their header.
        if (s.issues !== undefined) queueMicrotask(redraw);

        const before = pushes.get(id);

        try {
          const lists = await invoke<ListsRow | null>("center_project_lists", { projectId: id, refresh: force });
          // A push that came while the read was out is newer than its answer.
          if (pushes.get(id) === before) apply(id, s, lists);
        } catch (e) {
          if (pushes.get(id) === before) fail(s, reason(e));
        }

        const next = reloadAgain.get(id);
        if (next === undefined) break;
        reloadAgain.delete(id);
        force = next;
      }
    } finally {
      reading.delete(id);
    }

    redraw();
  }

  /// Takes the lists as `lists`: the answer of a read or a push.
  function apply(id: string, s: ProjectThreads, lists: ListsRow | null) {
    // Why the host could not be read shows in each section, in place of an empty list.
    s.issuesError = lists?.issues_error ?? "";
    s.prsError = lists?.prs_error ?? "";
    s.issues = lists && !lists.issues_error ? withAssignees(lists.issues) : s.issues ?? [];
    s.prs = lists && !lists.prs_error ? lists.prs : s.prs ?? null;
    // Kept once the host answered; a failed read keeps the last good lists.
    if (lists && !lists.issues_error && !lists.prs_error) kept.put(id, { issues: s.issues, prs: s.prs });
  }

  function fail(s: ProjectThreads, error: string) {
    s.issuesError = s.prsError = error;
    s.issues ??= [];
    s.prs ??= null;
  }

  /// Takes the lists a project update pushed, as the newest.
  function take(id: string, pushed: Pushed<ListsRow | null>) {
    pushes.set(id, (pushes.get(id) ?? 0) + 1);
    if ("error" in pushed) fail(of(id), pushed.error);
    else apply(id, of(id), pushed.value);

    redraw();
  }

  /// Runs `work` for row `key`, showing it busy and its error.
  async function act(id: string, key: string, work: () => Promise<void>) {
    const s = of(id);
    if (s.busy.has(key)) return;
    s.busy.add(key);
    s.failed.delete(key);
    redraw();

    try {
      await work();
    } catch (e) {
      s.failed.set(key, reason(e));
    } finally {
      s.busy.delete(key);
      redraw();
    }
  }

  /// A row's actions and why its last one failed.
  function actsEl(s: ProjectThreads, keys: string[], buttons: ThreadActions["buttons"]): ThreadActions {
    return { buttons, error: keys.map(k => s.failed.get(k)).find(Boolean) ?? null };
  }

  /// A pull request's work, review and discussion actions on the project page.
  function prButtons(project: ProjectRef, ctx: RepoActs, pr: PullRequestRow) {
    const s = of(project.project_id);
    const n = pr.number;
    const row = () => s.prs?.find(p => p.number === n) ?? pr;
    const session = ctx.prSession(pr.url, redraw);
    const work = buttonOf(s, `c${n}`, () => ctx.button((pick, machine) => void act(project.project_id, `c${n}`, () => ctx.startPrWork(row(), pick, machine))));
    const workState: AgentState = session === "checking" ? { kind: "checking" }
      : session ? { kind: "done", label: "Continue", hint: "Open the session that created this PR", open: () => ctx.openSession(session) }
      : s.busy.has(`c${n}`) ? { kind: "starting" }
      : { kind: "ready", label: "Work on PR", hint: "Start an agent on this pull request's branch" };
    const buttons: ThreadActions["buttons"] = [{ kind: "agent", key: "continue", button: work, state: workState }];
    const reviewed = ctx.reviewOf(pr);
    if (reviewed && !AGENT_ACTIVE.has(reviewed.status)) buttons.push({ kind: "run", key: "open-review", label: "Open review", primary: false, disabled: false, title: "Opens the agent's review of this PR. Right-click to review it again.", run: () => ctx.openSession(reviewed) });
    else {
      const button = buttonOf(s, `a${n}`, () => ctx.button((pick, machine) => void act(project.project_id, `a${n}`, () => ctx.startReview(row(), pick, machine))));
      const state: AgentState = reviewed ? { kind: "running", label: "Reviewing", agent: agentLabel(reviewed.agent), hint: "Open the review", open: () => ctx.openSession(reviewed) }
        : s.busy.has(`a${n}`) ? { kind: "starting" }
        : { kind: "ready", label: "Review with agent", hint: "Pick a model: an agent reviews this PR and posts its comments on it" };
      buttons.push({ kind: "agent", key: "agent-review", button, state });
    }
    buttons.push(discussionButton(ctx, { ...pr, kind: "pr" }));
    return actsEl(s, [`c${n}`, `a${n}`], buttons);
  }

  /// An open issue's work and discussion actions on the project page.
  function issueButtons(project: ProjectRef, ctx: RepoActs, i: IssueRow): ThreadActions {
    const s = of(project.project_id);
    const key = `w${i.number}`;
    const button = buttonOf(s, key, () => ctx.button((pick, machine) => void act(project.project_id, key, () => ctx.startIssue(s.issues?.find(x => x.number === i.number) ?? i, pick, machine))));
    const state: AgentState = ctx.issueSession(i) ?? (s.busy.has(key) ? { kind: "starting" } : { kind: "ready", label: "Work on it", hint: "Pick a model: an agent starts on a new branch for this issue" });
    return actsEl(s, [key, `g${i.number}`], [{ kind: "agent", key: "work", button, state }, discussionButton(ctx, { ...i, kind: "issue" })]);
  }

  /// Fills the agent composer with a prompt about the thread.
  function discussionButton(ctx: RepoActs, topic: DiscussionTopic): ThreadAction {
    return { kind: "run", key: "discuss", label: "Discuss with agent", primary: false, disabled: false, title: "Fill the agent prompt for this thread", run: () => ctx.discuss(topic) };
  }

  /// The agent button of row `key`, made once.
  function buttonOf(s: ProjectThreads, key: string, make: () => PickButton) {
    let button = s.buttons.get(key);
    if (!button) s.buttons.set(key, button = make());
    return button;
  }

  /// A pull request row's right-click menu: what its buttons do.
  function prMenu(e: MouseEvent, project: ProjectRef, ctx: RepoActs, pr: PullRequestRow) {
    const n = pr.number;
    const items = (): MenuItem[] => {
      const session = ctx.prSession(pr.url, () => update(items()));
      const actions: MenuItem[] = [{ label: "Open on forge", on: () => openUrl(pr.url) }];
      if (session === "checking") actions.push({ label: "Finding session", disabled: true, on: () => {} });
      else if (session) actions.push({ label: "Continue", on: () => ctx.openSession(session) });
      else actions.push({ label: "Work on PR", on: () => void act(project.project_id, `c${n}`, () => ctx.workOnPr(pr)) });

      const reviewed = ctx.reviewOf(pr);
      if (reviewed) actions.push({ label: "Open review", on: () => ctx.openSession(reviewed) });
      actions.push({ label: reviewed ? "Review again" : "Agent review", on: () => void act(project.project_id, `a${n}`, () => ctx.reviewPr(pr)) });
      return actions;
    };
    const update = openMenu(e, `#${n} ${pr.title}`, items());
  }

  /// An issue row's right-click menu: what its buttons do.
  function issueMenu(e: MouseEvent, project: ProjectRef, ctx: RepoActs, i: IssueRow) {
    const logins = ctx.assignees().filter(login => !i.assignees.some(assignee => sameLogin(assignee, login)));
    const assign: MenuItem[] = logins.length ? [{ kind: "section", label: "Assign to", items: logins.map(login => ({ label: login, on: () => void act(project.project_id, `g${i.number}`, () => ctx.assign(i, login)) })) }] : [];
    openMenu(e, `#${i.number} ${i.title}`, [
      { label: "Open on forge", on: () => openUrl(i.url) },
      { label: "Work on it", on: () => ctx.workOn(i) },
      ...assign,
    ]);
  }

  return {
    load,
    take,
    ensure,
    issues: (id: string) => of(id).issues,
    issuesError: (id: string) => of(id).issuesError,
    prs: (id: string) => of(id).prs,
    prsError: (id: string) => of(id).prsError,
    prButtons,
    issueButtons,
    prMenu,
    issueMenu,
  };
}
