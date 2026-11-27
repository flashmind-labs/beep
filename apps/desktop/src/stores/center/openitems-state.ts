// Home's open PRs and issues, broken down by project: what each row says,
// apart from the DOM so it can be tested.
import type { Project } from "../project/projects-state.ts";

/// A saved git host, as `git_items` lists it (GitHostRow).
interface GitHostRow {
  error: string | null;
}

/// An open PR or issue, as `git_items` lists it (GitItemRow).
export interface GitItemRow {
  host: string;
  repo: string;
  number: number;
  kind: "pull_request" | "issue";
  title: string;
  url: string;
  /// In milliseconds since the Unix epoch.
  updated_at: number;
  authored: boolean;
  assigned: boolean;
  review_requested: boolean;
  draft: boolean;
  /// `none`, `approved` or `changes_requested`.
  review: string;
  checks_failing: boolean;
  /// Missing from a list kept by an older app.
  labels?: string[];
  /// When it opened, in milliseconds since the Unix epoch. A host that does
  /// not say leaves it out, and the lists keep such rows under Any time and
  /// sort them last.
  opened_at?: number | null;
}

/// What `git_items` gives (GitItemsView).
export interface GitItemsView {
  items: GitItemRow[];
  hosts: GitHostRow[];
}

/// The fields of an item the helpers read.
type Item = Pick<GitItemRow, "kind" | "review_requested" | "authored" | "review" | "checks_failing">;

/// A project as the open rows need it.
type OpenProject = Pick<Project, "name" | "imported" | "machines">;

/// One repository's row: its counts and items. `P` is the project row.
export interface OpenRow<P = OpenProject> {
  key: string;
  repo: string;
  host: string;
  url: string;
  /// The imported project it belongs to, when there is one.
  project: P | null;
  prs: number;
  issues: number;
  waiting: number;
  last: number;
  items: GitItemRow[];
}

/// Whether an item waits on the user: a PR asking for their review, or
/// one of theirs with changes requested or failing checks.
export function waitsOnYou(item: Item) {
  if (item.kind !== "pull_request") return false;
  return item.review_requested || (item.authored && (item.review === "changes_requested" || item.checks_failing));
}

/// Orders items by the latest activity, newest first.
export function newestFirst(a: Pick<GitItemRow, "updated_at">, b: Pick<GitItemRow, "updated_at">) {
  return b.updated_at - a.updated_at;
}

/// How Home orders open items: by repository, or across them by latest
/// update or by when they opened.
export const GIT_SORTS = [
  { id: "default", label: "Default" },
  { id: "activity", label: "Recent activity" },
  { id: "created", label: "Newest" },
] as const;
export type GitSort = typeof GIT_SORTS[number]["id"];

export function parseGitSort(value: string | null): GitSort {
  return GIT_SORTS.find(sort => sort.id === value)?.id ?? "default";
}

/// The time an item sorts and shows by: when it opened for `created`, its
/// latest update otherwise. A host that does not say when it opened sorts it last.
export function itemTime(sort: GitSort, item: Pick<GitItemRow, "updated_at" | "opened_at">) {
  return sort === "created" ? item.opened_at ?? 0 : item.updated_at;
}

/// The stretch of time Home keeps its open items to: all of them, this week from Monday, or
/// one day, `back` days before today.
export type GitPeriod = { kind: "all" } | { kind: "week" } | { kind: "day"; back: number };

/// The items `sort` times inside `period`, in local time.
export function inPeriod<T extends Pick<GitItemRow, "updated_at" | "opened_at">>(items: readonly T[], period: GitPeriod, sort: GitSort, now: number): T[] {
  if (period.kind === "all") return [...items];
  const day = new Date(now);
  day.setHours(0, 0, 0, 0);
  day.setDate(day.getDate() - (period.kind === "week" ? (day.getDay() + 6) % 7 : period.back));
  const from = day.getTime();
  day.setDate(day.getDate() + 1);
  const to = period.kind === "week" ? Infinity : day.getTime();

  return items.filter(item => { const at = itemTime(sort, item); return at >= from && at < to; });
}

/// The day `back` days before today: Today, Yesterday, then its date.
export function dayLabel(back: number, now: number) {
  if (back < 2) return back ? "Yesterday" : "Today";
  const day = new Date(now);
  day.setDate(day.getDate() - back);
  return day.toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" });
}

/// The repository's web page from an item's.
function repoUrl(url: string | null | undefined) {
  return String(url || "").replace(/\/(pull|pulls|issues)\/\d+([/?#].*)?$/, "");
}

/// One row per repository with its counts and its items, newest first.
export function projectsWithOpen<P extends OpenProject>(
  view: { items?: readonly GitItemRow[] } | null | undefined,
  projects: readonly P[] | null | undefined,
) {
  const byName = new Map((projects || []).filter(p => p.imported).map(p => [p.name.toLowerCase(), p]));
  const rows = new Map<string, OpenRow<P>>();

  for (const item of view?.items || []) {
    const key = `${item.host} ${item.repo}`;
    let row = rows.get(key);

    if (!row) {
      const project = byName.get(item.repo.toLowerCase()) || null;
      row = { key, repo: item.repo, host: item.host, url: repoUrl(item.url), project, prs: 0, issues: 0, waiting: 0, last: 0, items: [] };
      rows.set(key, row);
    }

    if (item.kind === "pull_request") row.prs++;
    else row.issues++;
    if (waitsOnYou(item)) row.waiting++;
    row.last = Math.max(row.last, item.updated_at);
    row.items.push(item);
  }

  for (const row of rows.values()) row.items.sort(newestFirst);
  return [...rows.values()].sort((a, b) => (Math.sign(b.waiting) - Math.sign(a.waiting)) || b.last - a.last);
}

/// The state chips of an item.
export function itemChips(item: Item & Pick<GitItemRow, "draft" | "assigned">) {
  const out: { text: string; cls: string }[] = [];

  if (item.kind === "pull_request") {
    if (item.review_requested) out.push({ text: "your review", cls: "wait" });
    if (item.draft) out.push({ text: "draft", cls: "" });
    if (item.review === "changes_requested") out.push({ text: "changes requested", cls: "wait" });
    else if (item.review === "approved") out.push({ text: "approved", cls: "run" });
    if (item.checks_failing) out.push({ text: "checks failing", cls: "bad" });
  } else if (item.assigned) out.push({ text: "assigned", cls: "" });

  return out;
}

/// A project for Review and Reply: its name and checkouts.
export interface StartProject { name: string; machines?: readonly { id: string; folder: string }[] }
export type Hosts = readonly { id: string; online: boolean }[] | null | undefined;

/// What Reply prefills: a prompt to help reply, in the item's project.
export function replyStart(item: Pick<GitItemRow, "url">, project: StartProject, machines: Hosts) {
  return { prompt: `Help me reply to ${item.url}`, at: projectStart(project, machines) };
}

/// Where an agent on `project` starts: an online machine with a checkout of
/// it when one has it.
export function projectStart(project: StartProject, machines: Hosts) {
  const online = new Set((machines || []).filter(m => m.online).map(m => m.id));
  const places = (project.machines || []).filter(m => m.folder).map(m => ({ machine: m.id, cwd: m.folder }));
  const at = places.find(p => online.has(p.machine)) ?? null;
  return { machine: at?.machine ?? null, cwd: at?.cwd ?? null, places, project: project.name };
}
