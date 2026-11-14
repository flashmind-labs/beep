// A project's open issues and pull requests.
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { Ago } from "../ui/format.ts";
import { plural, reason } from "../ui/format.ts";
import { openMenu, type MenuItem } from "../ui/menu.tsx";
import { PageCache, REFRESH_MS, type SectionStore } from "../ui/pagecache.ts";
import type { ThreadActions } from "./threaddetail-view.tsx";
import type { ReviewSession } from "./projectbar.ts";
import type { ProjectRef } from "./localid.ts";

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
interface ListsRow {
  prs: PullRequestRow[];
  prs_error: string | null;
  issues: IssueRow[];
  issues_error: string | null;
}

/// What the project page hands the Pull requests and Issues sections: what
/// Work on it, Continue, Agent review and Open review do.
export interface RepoActs {
  workOn: (issue: IssueRow) => void;
  continuePr: (pr: PullRequestRow) => Promise<void>;
  reviewPr: (pr: PullRequestRow) => Promise<void>;
  /// The session that reviewed the PR before, which Open review opens.
  reviewOf: (pr: PullRequestRow) => ReviewSession | null;
  openReview: (s: ReviewSession) => void;
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
  issues: IssueRow[] | null;
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
      s = { issues: k?.issues, prs: k?.prs, issuesError: "", prsError: "", busy: new Set(), failed: new Map() };
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

  async function load(id: string, refresh = false) {
    const s = of(id);
    if (reading.has(id)) return;
    reading.add(id);
    tried.set(id, Date.now());
    // Kept lists stay up meanwhile, with a spinner in their header.
    if (s.issues !== undefined) queueMicrotask(redraw);

    try {
      const lists = await invoke<ListsRow | null>("center_project_lists", { projectId: id, refresh });
      // Why the host could not be read shows in each section, in place of an empty list.
      s.issuesError = lists?.issues_error ?? "";
      s.prsError = lists?.prs_error ?? "";
      s.issues = lists && !lists.issues_error ? lists.issues : s.issues ?? [];
      s.prs = lists && !lists.prs_error ? lists.prs : s.prs ?? null;
      // Kept once the host answered; a failed read keeps the last good lists.
      if (lists && !lists.issues_error && !lists.prs_error) kept.put(id, { issues: s.issues, prs: s.prs });
    } catch (e) {
      s.issuesError = s.prsError = reason(e);
      s.issues ??= [];
      s.prs ??= null;
    } finally {
      reading.delete(id);
    }

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

  /// A pull request's buttons on the project page: Continue, and Agent review
  /// or Open review, and why the last one failed.
  function prButtons(project: ProjectRef, ctx: RepoActs, pr: PullRequestRow) {
    const s = of(project.project_id);
    const n = pr.number;
    const buttons: ThreadActions["buttons"] = [{ key: "continue", label: s.busy.has(`c${n}`) ? "Reading…" : "Continue", primary: true, disabled: s.busy.has(`c${n}`), title: "Start a coding agent on this pull request's branch", run: () => { void act(project.project_id, `c${n}`, () => ctx.continuePr(pr)); } }];
    const reviewed = ctx.reviewOf(pr);
    if (reviewed) buttons.push({ key: "open-review", label: "Open review", primary: false, disabled: false, title: "Opens the agent's review of this PR. Right-click to review it again.", run: () => ctx.openReview(reviewed) });
    else buttons.push({ key: "agent-review", label: s.busy.has(`a${n}`) ? "Reading…" : "Agent review", primary: false, disabled: s.busy.has(`a${n}`), title: "Starts an agent that reviews this PR and posts its comments on it", run: () => { void act(project.project_id, `a${n}`, () => ctx.reviewPr(pr)); } });
    return actsEl(s, [`c${n}`, `a${n}`], buttons);
  }

  /// An open issue's button: Work on it.
  function issueButtons(ctx: RepoActs, i: IssueRow): ThreadActions {
    return { buttons: [{ key: "work", label: "Work on it", primary: true, disabled: false, title: "Start a coding agent on a new branch for this issue", run: () => ctx.workOn(i) }], error: null };
  }

  /// A pull request row's right-click menu: what its buttons do.
  function prMenu(e: MouseEvent, project: ProjectRef, ctx: RepoActs, pr: PullRequestRow) {
    const n = pr.number;
    const items: MenuItem[] = [
      { label: "Open on forge", on: () => openUrl(pr.url) },
      { label: "Continue", on: () => void act(project.project_id, `c${n}`, () => ctx.continuePr(pr)) },
    ];
    const reviewed = ctx.reviewOf(pr);
    if (reviewed) items.push({ label: "Open review", on: () => ctx.openReview(reviewed) });
    items.push({ label: reviewed ? "Review again" : "Agent review", on: () => void act(project.project_id, `a${n}`, () => ctx.reviewPr(pr)) });
    openMenu(e, `#${n} ${pr.title}`, items);
  }

  /// An issue row's right-click menu: what its buttons do.
  function issueMenu(e: MouseEvent, ctx: RepoActs, i: IssueRow) {
    openMenu(e, `#${i.number} ${i.title}`, [
      { label: "Open on forge", on: () => openUrl(i.url) },
      { label: "Work on it", on: () => ctx.workOn(i) },
    ]);
  }

  return {
    load,
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
