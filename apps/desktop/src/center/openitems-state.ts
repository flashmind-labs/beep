// Home's open PRs and issues, broken down by project: what each row says,
// apart from the DOM so it can be tested.
import type { Project } from "../project/projects-state.ts";

/// A saved git host, as `git_items` lists it (GitHostRow).
export interface GitHostRow {
  host: string;
  forgejo: boolean;
  account: string | null;
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
  comments: number;
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

/// Items a project shows before "Show all".
export const ITEMS_SHOWN = 3;

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

/// The repository's web page from an item's.
export function repoUrl(url: string | null | undefined) {
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

/// What Review prefills in Start a coding agent: the prompt and, when the
/// project is imported, the folder on an online machine that has it.
export function reviewStart(
  item: Pick<GitItemRow, "host" | "url">,
  project: { name?: string; machines?: readonly { id: string; folder: string }[] } | null | undefined,
  machines: readonly { id: string; online: boolean }[] | null | undefined,
) {
  const tool = item.host === "github.com" ? "gh" : "teacli";
  const prompt = `Review PR ${item.url}. Read the diff with ${tool}, read the code around each change, and run the tests that cover it. Report real defects with file, line and how they fail, and say what you ran. Post your review only after I approve it.`;
  const online = new Set((machines || []).filter(m => m.online).map(m => m.id));
  const at = project?.machines?.find(m => online.has(m.id) && m.folder) || null;
  const places = (project?.machines || []).filter(m => m.folder).map(m => ({ machine: m.id, cwd: m.folder }));
  return { prompt, at: at ? { machine: at.id, cwd: at.folder, places, ...(project?.name ? { project: project.name } : {}) } : null };
}
