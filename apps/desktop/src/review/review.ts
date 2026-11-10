import { createElement, Fragment, type ReactNode } from "react";
import { removeWindowPart, showWindowParts } from "../center/window-root.ts";
import { ReviewFindings } from "./review-findings.tsx";
import { ReviewActions, ReviewHeading } from "./review-controls.tsx";
import { ReviewStatus } from "./review-status.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
// review/: a project's branches and reviews. Entry:
// makeReviews here.
// A project's branches and the review of one before a merge.
import type { invoke as Invoke } from "../ui/tauri.ts";
import { agentLabel } from "../project/projects-state.ts";
import type { Ago, MakeButton, MakeEl } from "../ui/dom.ts";
import type { Place } from "./branchwork.ts";
import { plural, reason } from "../ui/format.ts";
import { PageCache, REFRESH_MS, type SectionStore } from "../ui/pagecache.ts";

/// One branch of a project, as `project_branches` lists it (BranchRow).
export interface BranchRow {
  name: string;
  worktree: string | null;
  ahead: number;
  behind: number;
  head: string;
  subject: string;
  /// In milliseconds since the Unix epoch.
  committed_at: number;
  local: boolean;
  pushed: boolean;
}

/// A project's branches, read on one machine (BranchesView).
export interface BranchesView {
  machine_id: string;
  machine_name: string;
  default_branch: string;
  branches: BranchRow[];
}

/// The pull request a review's branch has (PrView).
export interface PrView {
  number: number;
  url: string;
  /// `none`, `pending`, `passing` or `failing`.
  checks: string;
}

/// One reviewer's part of the diff (ShardView).
export interface ShardView {
  area: string;
  files: number;
  lines: number;
  agent: string;
  /// `reading`, `verifying`, `done` or `failed`.
  state: string;
  /// What the reviewer does now while reading; why it failed.
  note: string;
  found: number;
}

/// One finding of a review (FindingView).
export interface FindingView {
  id: number;
  area: string;
  /// `high`, `medium` or `low`.
  severity: string;
  title: string;
  file: string;
  line: number | null;
  why: string;
  /// `waiting`, `verified`, `dropped` or `unchecked`.
  verdict: string;
  /// Why the verifier dropped it.
  dropped_why: string;
  dismissed: boolean;
}

/// A review of a branch (ReviewView).
export interface ReviewView {
  id: string;
  project: string;
  branch: string;
  base: string;
  head: string;
  /// In milliseconds since the Unix epoch.
  head_at: number;
  started_at: number;
  machine_name: string;
  changed_lines: number;
  files: number;
  /// `clean`, `conflicts` or `unknown`.
  merge: string;
  conflicts: string[];
  pr: PrView | null;
  shards: ShardView[];
  findings: FindingView[];
}

/// Where a review's branch is now (ReviewHead); `head` is null once it is gone.
export interface ReviewHead {
  head: string | null;
  stale: boolean;
}

/// The task Fix with worker made (FixView).
export interface FixView {
  id: string;
  slug: string;
  title: string;
  /// The task's state, as a project's page lists it.
  state: string;
}

/// A review's state, for its pill.
export type ReviewState = "running" | "blocked" | "changes" | "ready";

/// One fact of the facts row: an optional spinner or dot, then its words.
export interface ReviewFact {
  text: string;
  bold?: string;
  code?: string;
  pre?: string;
  dot?: string;
  spin?: boolean;
  bad?: boolean;
}

/// A shard's state chip.
export interface ShardChip {
  cls: string;
  text: string;
  spin?: boolean;
}

/// What the review helpers read of a review.
type Shards = { shards?: readonly Pick<ShardView, "state">[] } | null | undefined;
type Findings<F> = { findings?: readonly F[] } | null | undefined;
type Verdicts = Pick<FindingView, "dismissed" | "verdict">;
type Mergeable = Pick<ReviewView, "merge" | "conflicts" | "pr" | "shards" | "findings">;

/// The branches a project lists; one that failed to read before any arrived
/// has only its default branch and rows.
type Branches = Pick<BranchesView, "default_branch" | "branches"> & Partial<Pick<BranchesView, "machine_id" | "machine_name">>;

/// How often a running review is read again.

/// Findings neither dismissed nor dropped by the verifier.
export function openFindings<F extends Verdicts>(r: Findings<F>) {
  return (r?.findings || []).filter(f => !f.dismissed && f.verdict !== "dropped");
}

/// Whether a reviewer or a verifier still works.
export function isRunning(r: Shards) {
  return (r?.shards || []).some(s => s.state === "reading" || s.state === "verifying");
}

/// "running", "blocked" (conflicts or failing checks), "changes" (open
/// findings) or "ready".
export function reviewState(r: Pick<ReviewView, "merge" | "pr" | "shards" | "findings">): ReviewState {
  if (isRunning(r)) return "running";
  if (r.merge === "conflicts" || r.pr?.checks === "failing") return "blocked";
  if (openFindings(r).length) return "changes";
  return "ready";
}

/// The pill's words for each state.
export const STATE_LABEL: Record<ReviewState, string> = { running: "Reviewing", blocked: "Blocked", changes: "Needs changes", ready: "Ready" };

/// What to say when the branch moved since the review, from what
/// `CheckBranchReview` gave; "" when it did not, or is not known yet.
export function staleText(r: Pick<ReviewView, "branch">, now: ReviewHead | null | undefined) {
  if (!now?.stale) return "";
  const head = now.head ? now.head.slice(0, 7) : null;
  return head ? `${r.branch} moved to ${head} since this review.` : `${r.branch} is gone since this review.`;
}

/// Why Merge is off, or "" when it may ask. `now` is where the branch is
/// now, when read.
export function mergeBlock(r: Mergeable, now?: ReviewHead | null) {
  if (isRunning(r)) return "Actions open when the review is done";
  if (now?.stale) return "Merge is off: the branch moved since this review";
  const why = [];
  if (r.pr?.checks === "failing") why.push("CI fails");
  if (r.merge === "conflicts") why.push(`${plural(r.conflicts.length, "file conflicts", "files conflict")}`);
  if (why.length) return `Merge is off while ${why.join(" and ")}`;
  const open = openFindings(r).length;
  if (open) return `Merge is off while ${plural(open, "finding is", "findings are")} open. Fix them or dismiss them.`;
  if (!r.pr) return "Merge needs a pull request for the branch";
  return "Merge always asks first. It never runs on its own.";
}

/// The facts row: changed lines, shards, CI, merge, head and when.
export function reviewFacts(r: Mergeable & Pick<ReviewView, "changed_lines" | "started_at" | "head_at" | "head">, ago: Ago) {
  const out: ReviewFact[] = [
    { bold: String(r.changed_lines), text: " changed lines" },
    { bold: String(r.shards.length), text: r.shards.length === 1 ? " shard" : " shards" },
  ];
  const ci = r.pr?.checks;
  if (!r.pr) out.push({ text: "No pull request", dot: "off" });
  else if (ci === "failing") out.push({ text: "CI failing", dot: "bad", bad: true });
  else if (ci === "pending") out.push({ text: "CI running", spin: true });
  else if (ci === "passing") out.push({ text: "CI passing", dot: "" });
  else out.push({ text: "No CI reported", dot: "off" });
  if (r.merge === "conflicts") out.push({ text: `Conflicts in ${plural(r.conflicts.length, "file", "files")}`, bad: true });
  else if (r.merge === "clean") out.push({ text: "Merges cleanly" });
  else out.push({ text: "Merge not checked" });
  const since = (ms: number) => (ago(ms) === "now" ? "just now" : `${ago(ms)} ago`);
  const when = isRunning(r) ? `started ${since(r.started_at)}` : since(r.head_at);
  out.push({ code: r.head.slice(0, 7), text: ` · ${when}`, pre: "head " });
  return out;
}

/// A shard's second line: "268 lines · 6 files · Claude Code on build-box".
export function shardLine(s: Pick<ShardView, "lines" | "files" | "agent">, machine: string | null | undefined) {
  const who = machine ? `${agentLabel(s.agent)} on ${machine}` : agentLabel(s.agent);
  return `${plural(s.lines, "line", "lines")} · ${plural(s.files, "file", "files")} · ${who}`;
}

/// A shard's state chip.
export function shardChip(s: Pick<ShardView, "state"> & Partial<Pick<ShardView, "note" | "found">>): ShardChip {
  if (s.state === "reading") return { cls: "running", text: s.note || "reading", spin: true };
  if (s.state === "verifying") return { cls: "running", text: "verifying", spin: true };
  if (s.state === "failed") return { cls: "bad", text: "failed" };
  return { cls: "done", text: s.found ? `done · ${plural(s.found, "finding", "findings")}` : "done · nothing found" };
}

/// The findings section's subtitle.
export function findingsSub(r: Pick<ReviewView, "shards" | "findings">) {
  if (isRunning(r)) return "reviewing";
  const open = openFindings(r).length;
  return open ? `${open} open, ranked` : "none open";
}

/// What the empty findings panel says once the review is done.
export function readySummary(r: Pick<ReviewView, "shards" | "findings" | "changed_lines">) {
  const dropped = (r.findings || []).filter(f => f.verdict === "dropped");
  const dismissed = (r.findings || []).filter(f => f.dismissed && f.verdict !== "dropped").length;
  const parts = [`${plural(r.shards.length, "reviewer", "reviewers")} read all ${r.changed_lines} lines.`];
  if (dropped.length === 1) parts.push(`The verifier dropped 1 finding that was not real${dropped[0].dropped_why ? `: ${dropped[0].dropped_why}` : "."}`);
  else if (dropped.length) parts.push(`The verifier dropped ${dropped.length} findings that were not real.`);
  if (dismissed) parts.push(`${plural(dismissed, "finding was", "findings were")} dismissed.`);
  return parts.join(" ");
}

/// What the window hands `makeReviews`. `machines()` gives Home's machines,
/// for where a worktree goes.
export interface ReviewDeps {
  invoke: typeof Invoke;
  h: MakeEl;
  button: MakeButton;
  ago: Ago;
  redraw: () => void;
  openUrl: (url: string) => unknown;
  openTile: (key: string) => void;
  machines?: () => readonly (Place & { online: boolean })[];
  /// What opens the session working on a branch of a project at its changes; null when none does.
  diff?: (project: string, branch: string) => (() => void) | null;
  /// Where the branches are kept between windows; in memory only without it.
  kept?: SectionStore<KeptBranches>;
}

/// A project's branches and reviews as the page cache keeps them.
export interface KeptBranches {
  branches: BranchesView | null;
  reviews: ReviewView[] | null;
}

/// One project's branches and reviews. `now` is where the open review's
/// branch is now.
interface ProjectReviews {
  branches: Branches | null | undefined;
  /// Null from a server from before reviews.
  reviews: ReviewView[] | null | undefined;
  open: string | null;
  review: ReviewView | null;
  starting: string | null;
  error: string;
  note: string;
  busy: string;
  now?: ReviewHead & { id: string };
}

export function makeReviews(deps: ReviewDeps) {
  const { invoke, h, ago, redraw, openUrl, openTile } = deps;
  const kept = deps.kept ?? new PageCache(null).section<KeptBranches>("branches");
  const all = new Map<string, ProjectReviews>();
  let unfollow: (() => void) | undefined;
  let polling: string | null = null;
  let detailView: { project: string; host: HTMLElement } | null = null;

  /// A project's branches and reviews, from the cache until they are read.
  const of = (name: string) => {
    let t = all.get(name);

    if (!t) {
      const k = kept.get(name);
      all.set(name, t = { branches: k?.branches, reviews: k?.reviews, open: null, review: null, starting: null, error: "", note: "", busy: "" });
    }

    return t;
  };

  /// Projects whose read is on its way (a load meanwhile is dropped), and when
  /// each was last tried, so a failing machine is not asked on every draw.
  const reading = new Set<string>();
  const tried = new Map<string, number>();

  /// Reads `name` when it is due: never read, or `every` passed since the last
  /// try (the refresh time by default).
  function ensure(name: string, every = REFRESH_MS): Promise<void> | undefined {
    const last = tried.get(name);
    const due = last === undefined ? kept.due(name, every) : Date.now() - last >= every;
    if (due && !reading.has(name)) return load(name);
  }

  const loads = new Map<string, Promise<void>>();
  function load(name: string): Promise<void> {
    const pending = loads.get(name);
    if (pending) return pending;
    const read = readList(name).finally(() => loads.delete(name));
    loads.set(name, read);
    return read;
  }

  async function readList(name: string) {
    const t = of(name);
    if (reading.has(name)) return;
    reading.add(name);
    tried.set(name, Date.now());
    // Kept lists stay up meanwhile, with a spinner in their header.
    if (t.branches !== undefined) queueMicrotask(redraw);
    let failed = false;
    t.error = "";
    const [branches, reviews] = await Promise.all([
      invoke<BranchesView | null>("project_branches", { project: name }).catch((e): Branches | null => { failed = true; t.error = reason(e); return t.branches ?? { default_branch: "", branches: [] }; }),
      invoke<ReviewView[] | null>("reviews_list", { project: name }).catch(e => { failed = true; t.error = reason(e); console.warn("reviews:", e); return t.reviews ?? []; }),
    ]);
    reading.delete(name);
    t.branches = branches;
    t.reviews = reviews;
    // Kept once both answered; a failed read keeps the last good lists.
    if (!failed) kept.put(name, { branches: branches as BranchesView | null, reviews });
    redraw();
  }

  function keep(name: string, r: ReviewView) {
    const t = of(name);
    if (t.open === r.id) t.review = r;
    t.reviews = [r, ...(t.reviews || []).filter(x => x.branch !== r.branch)];
  }

  /// Reads once per review where its branch is now; a server from before
  /// the check, or an offline machine, leaves it unknown.
  async function checkHead(name: string, r: ReviewView) {
    const t = of(name);
    if (t.now?.id === r.id || isRunning(r)) return;
    t.now = { id: r.id, head: null, stale: false };

    try {
      const now = await invoke<ReviewHead | null>("review_check", { id: r.id });
      if (now && t.now?.id === r.id) t.now = { id: r.id, ...now };
    } catch (e) {
      console.warn("review head:", e);
    }

    redraw();
  }

  async function read(name: string) {
    const t = of(name);
    if (!t.open) return;
    const id = t.open;

    try {
      const r = await invoke<ReviewView>("review_get", { id });
      if (t.open !== id) return;
      keep(name, r);
      t.error = "";
    } catch (e) {
      t.error = reason(e);
    }

    redraw();
  }

  /// Follows reviews while the section shows; null stops.
  function watch(name: string | null) {
    if (polling === name) return;
    unfollow?.();
    polling = name;
    if (!name) return;
    unfollow = subscribeChanges(topic("reviews"), { deliver: "latest", receive: async () => {
      await loads.get(name);
      await Promise.all([load(name), read(name)]);
      if (of(name).error) throw new Error(of(name).error);
    } });
  }

  /// Opens `branch`'s newest review, starting one when it has none or
  /// `again` asks for a fresh one.
  async function openBranch(name: string, branch: string, again = false) {
    const t = of(name);
    t.error = "";
    t.note = "";
    const had = !again && (t.reviews || []).find(r => r.branch === branch);
    openTile("review");

    if (had) {
      t.open = had.id;
      t.review = had;
      read(name);
      return;
    }

    t.open = null;
    t.review = null;
    t.starting = branch;
    redraw();

    try {
      const r = await invoke<ReviewView>("review_start", { project: name, branch });
      t.open = r.id;
      keep(name, r);
    } catch (e) {
      t.error = reason(e);
    } finally {
      t.starting = null;
    }

    redraw();
  }

  async function act<T>(name: string, what: string, call: () => Promise<T>, done: (out: T) => void) {
    const t = of(name);
    t.busy = what;
    t.error = "";
    t.note = "";
    redraw();

    try {
      done(await call());
    } catch (e) {
      t.error = reason(e);
    } finally {
      t.busy = "";
    }

    redraw();
  }

  const dismiss = (name: string, r: ReviewView, f: FindingView) => act(name, `f${f.id}`,
    () => invoke<ReviewView>("review_dismiss", { review: r.id, finding: f.id, dismissed: !f.dismissed }),
    out => keep(name, out));

  const fix = (name: string, r: ReviewView) => act(name, "fix", () => invoke<FixView>("review_fix", { review: r.id }),
    task => { of(name).note = task.state === "running" ? `Started ${task.title}. It shows under Tasks.` : `Made ${task.title}; it starts when a worker slot frees.`; });

  const post = (name: string, r: ReviewView) => act(name, "post", () => invoke<string>("review_post", { review: r.id }),
    () => { of(name).note = "Asked the project agent to post the review. Approve its card to post."; });

  const merge = (name: string, r: ReviewView) => act(name, "merge", () => invoke<string>("review_merge", { review: r.id }),
    () => { of(name).note = "Asked the project agent to merge. Approve its card to merge."; });

  // ---------- Review ----------

  function reviewDetail(name: string, error: string | null) {
    if (!detailView || detailView.project !== name) {
      if (detailView) removeWindowPart("review-detail");
      detailView = { project: name, host: h("div", "pj-det") };
    }
    const t = of(name), r = t.review;
    const now = r && t.now?.id === r.id ? t.now : null;
    const status = createElement(ReviewStatus, {
      key: "status",
      starting: t.starting, review: r, error: t.error, note: t.note, stale: r ? staleText(r, now) : "",
      branches: () => openTile("branches"), again: () => { if (r) void openBranch(name, r.branch, true); },
    });
    let heading: ReactNode = null, findings: ReactNode = null, actions: ReactNode = null;
    if (t.starting || !r) watch(null);
    else {
      checkHead(name, r);
      heading = createElement(ReviewHeading, { key: "heading", review: r, now, ago, diff: deps.diff?.(name, r.branch) ?? null, openPr: (url: string) => void openUrl(url), again: () => void openBranch(name, r.branch, true) });
      findings = createElement("div", { key: "findings", className: "pj-cols" }, createElement(ReviewFindings, { review: r, busy: t.busy, dismiss: (finding: FindingView) => void dismiss(name, r, finding) }));
      actions = createElement(ReviewActions, { key: "actions", review: r, now, busy: t.busy, fix: () => void fix(name, r), post: () => void post(name, r), merge: () => void merge(name, r) });
      watch(name);
    }
    showWindowParts([{ part: "review-detail", host: detailView.host, identity: name, content: createElement(Fragment, null,
      error && createElement("div", { className: "cc-note" }, createElement("span", { className: "ddot bad" }), createElement("span", { className: "cc-err" }, `Could not refresh the project: ${error}`)), heading, status, findings, actions) }]);
    return detailView.host;
  }

  return {
    load,
    ensure,
    branches: (name: string) => of(name).branches,
    reviews: (name: string) => of(name).reviews,
    openBranch,
    reviewDetail,
    stop: () => { watch(null); removeWindowPart("review-detail"); detailView = null; },
  };
}
