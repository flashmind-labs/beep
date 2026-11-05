// review/: a project's branches and reviews. Entry:
// makeReviews here.
// A project's branches and the review of one before a merge.
import { harnessEl } from "../ui/harnessicon.ts";
import type { invoke as Invoke } from "../ui/tauri.ts";
import { agentLabel } from "../project/projects-state.ts";
import type { Ago, MakeButton, MakeEl } from "../ui/dom.ts";
import { worktreeMachines, worktreeName } from "./branchwork.ts";
import type { Place } from "./branchwork.ts";
import type { TileData, TileRow } from "../project/board.ts";
import { plural, reason } from "../ui/format.ts";
import { emptyPanel, errorNote, freshNote, note, stateChip, svgEl, secHead, writeClipboard } from "../ui/parts.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";
import { PageCache, REFRESH_MS, freshness, type SectionStore } from "../ui/pagecache.ts";

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

/// The machine a worktree goes on: one of `worktreeMachines`, or the one
/// the branches were read on.
type Target = Partial<Place>;

/// How often a running review is read again.
const POLL_MS = 4000;

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

/// The Branches tile: branches with work first (the default branch last),
/// each with its PR, worker or how far ahead it is.
export function branchTile(
  data: { default_branch: string; branches: readonly Pick<BranchRow, "name" | "ahead" | "behind" | "worktree" | "pushed" | "committed_at" | "subject">[] } | null | undefined,
  reviews: readonly (Pick<ReviewView, "branch"> & Parameters<typeof reviewState>[0])[] | null | undefined,
  ago: Ago,
): TileData {
  if (data === undefined) return { count: "", rows: [], empty: "Loading…" };
  if (data === null) return { count: "", rows: [], empty: "The server lists no branches" };
  const byBranch = new Map((reviews || []).map(r => [r.branch, r]));
  const rows: (TileRow & { work: boolean })[] = data.branches.map(b => {
    const r = byBranch.get(b.name), state = r && reviewState(r);
    const isDefault = b.name === data.default_branch;
    let note;
    if (isDefault) note = b.behind ? `default · ${b.behind} behind` : "default";
    else if (state) note = STATE_LABEL[state].toLowerCase();
    else if (b.ahead) note = `+${b.ahead}${b.worktree ? " · worktree" : ""}${b.pushed ? "" : " · not pushed"}`;
    else note = `merged · ${ago(b.committed_at)}`;
    return {
      code: b.name,
      note,
      chip: state && state !== "ready" && !isDefault ? { cls: state === "running" ? "run" : state === "blocked" ? "bad" : "wait", text: STATE_LABEL[state].toLowerCase() } : null,
      title: `${b.name}: ${b.subject}`,
      work: !isDefault && b.ahead > 0,
    };
  });
  const work = rows.filter(r => r.work).length;
  rows.sort((a, b) => Number(b.work) - Number(a.work));
  return { count: work ? `${work} with work` : data.branches.length ? String(data.branches.length) : "", rows, empty: "No branches" };
}

const TICK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5 9-10"/></svg>';

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
  /// The branch whose Other machine… list shows.
  picking?: string | null;
}

export function makeReviews(deps: ReviewDeps) {
  const { invoke, h, button, ago, redraw, openUrl, openTile } = deps;
  const machines = deps.machines || (() => []);
  const kept = deps.kept ?? new PageCache(null).section<KeptBranches>("branches");
  const all = new Map<string, ProjectReviews>();
  let timer: ReturnType<typeof setInterval> | undefined;
  let polling: string | null = null;

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

  async function load(name: string) {
    const t = of(name);
    if (reading.has(name)) return;
    reading.add(name);
    tried.set(name, Date.now());
    // Kept lists stay up meanwhile, with a spinner in their header.
    if (t.branches !== undefined) queueMicrotask(redraw);
    let failed = false;
    const [branches, reviews] = await Promise.all([
      invoke<BranchesView | null>("project_branches", { project: name }).catch((e): Branches | null => { failed = true; t.error = reason(e); return t.branches ?? { default_branch: "", branches: [] }; }),
      invoke<ReviewView[] | null>("reviews_list", { project: name }).catch(e => { failed = true; console.warn("reviews:", e); return t.reviews ?? []; }),
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

  /// Polls the open review of `name` while it runs and shows; null stops.
  function watch(name: string | null) {
    if (polling === name) return;
    clearInterval(timer);
    polling = name;
    if (!name) return;
    timer = setInterval(() => {
      if (document.visibilityState === "visible") read(name);
    }, POLL_MS);
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

  // ---------- Branches ----------

  /// Gives `branch` a worktree on `machine`, then reads the branches again.
  const addWorktree = (name: string, branch: string, machine: Target) => act(name, `wt:${branch}`,
    () => invoke<BranchRow>("add_branch_worktree", { project: name, branch, machineId: machine.id }),
    b => { const t = of(name); t.picking = null; t.note = `Added ${b.worktree || worktreeName(branch)} on ${machine.name}.`; load(name); });

  /// A branch without a worktree: one on the machine the list was read
  /// on, or Other machine… to pick among the online ones.
  function worktreeActs(name: string, t: ProjectReviews, data: Branches, b: BranchRow) {
    const where = worktreeMachines(machines(), data.machine_id);
    const here: Target = where.find(m => m.id === data.machine_id) || { id: data.machine_id, name: data.machine_name };
    const busy = t.busy === `wt:${b.name}`;
    if (t.picking === b.name) {
      return [
        ...where.map(m => { const x = button(m.name, "small", () => addWorktree(name, b.name, m)); x.disabled = busy; return x; }),
        button("Cancel", "quiet small", () => { t.picking = null; redraw(); }),
      ];
    }
    const go = button(busy ? "Adding…" : "Add worktree", "small", () => addWorktree(name, b.name, here));
    go.disabled = busy;
    go.title = `git worktree add ${worktreeName(b.name)}`;
    const other = where.length > 1 ? button("Other machine…", "quiet small", () => { t.picking = b.name; redraw(); }) : null;
    return other ? [go, other] : [go];
  }

  function branchesPanel(name: string) {
    const t = of(name);
    ensure(name);
    const out: HTMLElement[] = [];
    if (t.error) out.push(h("div", "cc-err", t.error));
    if (t.note && t.note.startsWith("Added ")) out.push(note("", t.note));
    const data = t.branches;
    if (data === null) {
      out.push(emptyPanel(h("b", "", "The server needs an update before it lists branches.")));
      return out;
    }
    if (!data) {
      out.push(emptyPanel(h("span", "dspin"), h("span", "", "Reading the branches…")));
      return out;
    }
    const byBranch = new Map((t.reviews || []).map(r => [r.branch, r]));
    const rows = data.branches.map(b => {
      const r = byBranch.get(b.name);
      const isDefault = b.name === data.default_branch;
      const worktree = b.worktree ? h("span", "br-tag", "worktree") : null;
      if (worktree) worktree.title = b.worktree!;
      const marks = [
        isDefault ? h("span", "br-tag", "default") : null,
        !isDefault && b.ahead ? h("span", "br-add", `+${b.ahead}`) : null,
        !isDefault && b.behind ? h("span", "br-tag", `${b.behind} behind`) : null,
        worktree,
        b.pushed ? null : h("span", "br-tag", "not pushed"),
      ];
      const acts = h("span", "acts rv-acts");
      const diff = isDefault ? null : deps.diff?.(name, b.name);
      if (diff) acts.append(button("View diff", "quiet small", diff));
      if (r) acts.append(button(`Open review · ${STATE_LABEL[reviewState(r)].toLowerCase()}`, "small", () => openBranch(name, b.name)));
      if (!isDefault && b.ahead > 0) acts.append(button(r ? "Review again" : "Review", r ? "quiet small" : "pri small", () => openBranch(name, b.name, !!r)));
      if (!isDefault && !b.worktree) acts.append(...worktreeActs(name, t, data, b));
      const title = h("span", "t", h("code", "", b.name), ...marks);
      const row = h("div", "pj-task pj-br", title, h("span", "m", h("span", "", `${b.subject} · ${ago(b.committed_at)}`)), acts.childNodes.length ? acts : null);
      row.querySelector<HTMLElement>(".m span")!.title = b.subject;
      row.addEventListener("contextmenu", e => {
        const items: MenuItem[] = [];
        if (diff) items.push({ label: "View diff", on: diff });
        if (r) items.push({ label: "Open review", on: () => openBranch(name, b.name) });
        if (!isDefault && b.ahead > 0) items.push({ label: r ? "Review again" : "Review", on: () => openBranch(name, b.name, !!r) });
        if (!isDefault && !b.worktree) {
          const here = worktreeMachines(machines(), data.machine_id).find(m => m.id === data.machine_id) || { id: data.machine_id, name: data.machine_name };
          items.push({ label: `Worktree on ${here.name}`, on: () => addWorktree(name, b.name, here) });
        }
        items.push({ label: "Copy name", on: () => void writeClipboard(b.name) });
        openMenu(e, b.name, items);
      });
      return row;
    });
    out.push(h("div", "", secHead("Branches", data.machine_name ? h("small", "", `read on ${data.machine_name}`) : null,
      freshNote(freshness(kept.at(name), reading.has(name), Date.now())), button("Refresh", "quiet small", () => load(name))),
      rows.length ? h("div", "cc-panel", ...rows) : emptyPanel(h("span", "", "No branches"))));
    return out;
  }

  // ---------- Review ----------

  function fact(f: ReviewFact) {
    const box = h("span", `rv-fact${f.bad ? " bad" : ""}`);
    if (f.spin) box.append(h("span", "dspin"));
    else if (f.dot != null) box.append(h("span", `ddot${f.dot ? " " + f.dot : ""}`));
    if (f.pre) box.append(f.pre);
    if (f.bold) box.append(h("b", "", f.bold));
    if (f.code) box.append(h("code", "", f.code));
    box.append(f.text);
    return box;
  }

  function findingRow(name: string, r: ReviewView, f: FindingView) {
    const dropped = f.verdict === "dropped";
    const ver = dropped ? h("span", "")
      : f.verdict === "waiting" ? h("span", "rv-ver wait", h("span", "dspin"), "verifying")
      : f.verdict === "verified" ? h("span", "rv-ver", svgEl(TICK), "Verified")
      : h("span", "rv-ver wait", "Not verified");
    const text = h("span", "t", h("b", "", f.title), dropped ? null : h("code", "", f.line ? `${f.file}:${f.line}` : f.file),
      h("span", "", dropped ? `Not real: ${f.dropped_why || "the verifier found no problem"}` : f.why));
    const right = h("span", "rv-side", ver);
    if (!dropped && !isRunning(r)) {
      const b = button(f.dismissed ? "Bring back" : "Dismiss", "quiet small", () => dismiss(name, r, f));
      b.disabled = of(name).busy === `f${f.id}`;
      right.append(b);
    }
    return h("div", `rv-find${dropped || f.dismissed ? " gone" : ""}`,
      h("span", `chip ${dropped ? "" : f.severity}`, dropped ? "dropped" : f.dismissed ? "dismissed" : f.severity), text, right);
  }

  function reviewPanel(name: string) {
    const t = of(name);
    const out: HTMLElement[] = [];
    if (t.starting) {
      out.push(emptyPanel(h("span", "dspin"), h("b", "", `Starting the review of ${t.starting}`)));
      watch(null);
      return out;
    }
    const r = t.review;
    if (!r) {
      if (t.error) out.push(errorNote(t.error));
      out.push(emptyPanel(h("b", "", "No review open"), button("Branches", "", () => openTile("branches"))));
      watch(null);
      return out;
    }
    const st = reviewState(r);
    const running = st === "running";
    checkHead(name, r);
    const now = t.now?.id === r.id ? t.now : null;
    const stale = staleText(r, now);
    const pill = h("span", `rv-pill ${stale ? "changes" : st}`, running ? h("span", "dspin") : null, stale ? "Stale" : STATE_LABEL[st]);
    const tools = h("span", "rv-tools");
    const pr = r.pr;
    const diff = deps.diff?.(name, r.branch);
    if (diff) tools.append(button("View diff", "", diff));
    if (pr) tools.append(button(`PR #${pr.number}`, "", () => openUrl(pr.url)));
    if (!running) {
      const again = button("Run again", "", () => openBranch(name, r.branch, true));
      again.title = "Reviews the branch head again";
      tools.append(again);
    }
    out.push(h("div", "rv-head", h("h3", "", `Review ${r.branch} into ${r.base}`), pill));
    out.push(h("div", "rv-facts", ...reviewFacts(r, ago).map(fact), tools));
    if (t.error) out.push(errorNote(t.error));
    if (t.note) out.push(note("", t.note));
    if (stale) {
      out.push(h("div", "cc-note", h("span", "ddot warn"),
        h("span", "", h("b", "", stale), ` The findings are for ${r.head.slice(0, 7)}; Merge is off until the new head is reviewed.`),
        button("Review again", "pri", () => openBranch(name, r.branch, true))));
    }
    const blocked: (Node | string)[] = [];
    if (r.merge === "conflicts") {
      blocked.push(h("b", "", `Conflicts with ${r.base} in ${plural(r.conflicts.length, "file", "files")}`),
        h("span", "", ...r.conflicts.slice(0, 6).flatMap((f, i) => [i ? ", " : "", h("code", "", f)]), r.conflicts.length > 6 ? ` and ${r.conflicts.length - 6} more` : ""));
    }
    if (pr?.checks === "failing") blocked.push(h("b", "", "CI failing"), h("span", "", `Checks fail on PR #${pr.number}. The review still ran.`));
    if (blocked.length) out.push(h("div", "cc-note rv-cfl", h("span", "ddot bad"), h("span", "rv-cfl-t", ...blocked)));

    const findings = r.findings || [];
    let body;
    if (findings.length) {
      body = h("div", "cc-panel", ...findings.map(f => findingRow(name, r, f)));
      if (running) body.append(h("div", "cc-empty rv-more", "More findings show as the reviewers finish."));
    } else if (running) {
      body = emptyPanel(h("span", "dspin"), h("span", "", "Findings show as the reviewers finish."));
    } else {
      body = emptyPanel(h("b", "", "No findings"), h("span", "rv-sum", readySummary(r)));
    }
    if (!running && findings.length && !openFindings(r).length) body.append(h("div", "rv-sum-row", readySummary(r)));
    const findSec = h("div", "", secHead("Findings", h("small", "", findingsSub(r))), body);
    const shards = h("div", "",
      secHead("Shards"),
      h("div", "cc-panel", ...r.shards.map(s => {
        const c = shardChip(s);
        const chipEl = stateChip(c.cls, c.text, c.spin);
        chipEl.title = s.note || c.text;
        return h("div", "rv-shard", harnessEl(s.agent),
          h("span", "t", h("b", "", s.area), h("span", "", shardLine(s, r.machine_name))), chipEl);
      })));
    out.push(h("div", "pj-cols", findSec, shards));

    const open = openFindings(r).length;
    const why = mergeBlock(r, now);
    const fixB = button(t.busy === "fix" ? "Starting…" : "Fix with worker", open && !running ? "pri" : "", () => fix(name, r));
    fixB.disabled = running || !open || !!t.busy;
    fixB.title = open ? `Starts a task on ${r.branch} with ${plural(open, "finding", "findings")}` : "";
    const postB = button(t.busy === "post" ? "Asking…" : "Post review…", "", () => post(name, r));
    postB.disabled = running || !pr || !!t.busy;
    postB.title = pr ? "The project agent posts it; its card asks you first" : "The branch has no pull request";
    const mergeOk = !running && why.startsWith("Merge always");
    const mergeB = button(t.busy === "merge" ? "Asking…" : "Merge…", mergeOk ? "pri" : "", () => merge(name, r));
    mergeB.disabled = !mergeOk || !!t.busy;
    out.push(h("div", "rv-bar", fixB, postB, mergeB, h("small", "", why)));
    watch(running ? name : null);
    return out;
  }

  return {
    load,
    ensure,
    branches: (name: string) => of(name).branches,
    reviews: (name: string) => of(name).reviews,
    openBranch,
    branchesPanel,
    reviewPanel,
    stop: () => watch(null),
  };
}
