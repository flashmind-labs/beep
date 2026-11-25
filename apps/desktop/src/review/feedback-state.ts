// Review with agent, apart from the DOM: what the reviewer is asked to
// review, how its submitted findings become draft comments, and when the
// button cannot start one.
import { type DiffFile, type ReviewComment, diffRows, lineKey } from "./diff-state.ts";

// ----- What the diff shows -----

/// The base the node measures the changes of `view` against, so the reviewer
/// reads the same diff with git: `HEAD` for the uncommitted changes, else the
/// merge base with the base branch (the working tree included).
export function reviewBase(view: { merge_base: string }, uncommitted: boolean) {
  return uncommitted || !view.merge_base ? "HEAD" : view.merge_base;
}

// ----- The review -----

/// One finding as the reviewer submitted it (CodeFinding).
interface CodeFinding {
  file: string;
  line: number | null;
  /// `high`, `medium` or `low`.
  severity: string;
  summary: string;
  scenario: string;
}

/// What the reviewer submitted (SubmittedReview).
export interface SubmittedReview {
  /// `approve`, `changes` or `comment`.
  verdict: string;
  summary: string;
  findings: CodeFinding[];
}

/// One review as the node keeps it (ReviewRun).
export interface ReviewRun {
  id: string;
  parent: string | null;
  cwd: string;
  base: string;
  agent: string;
  started_at: number;
  review: SubmittedReview | null;
  error: string | null;
}

/// The reviews a `reviews` update carries (the node's `Reviews` reply), every run the node keeps;
/// null when the update holds none, as after a refused snapshot.
export function reviewsOf(data: unknown): ReviewRun[] | null {
  const runs = (data as { Sessions?: { Reviews?: { runs?: unknown } } } | null)?.Sessions?.Reviews?.runs;
  if (!Array.isArray(runs)) return null;
  return runs.filter((r): r is ReviewRun => typeof r === "object" && r !== null && typeof (r as ReviewRun).id === "string");
}

/// Where a review is: still running, handed in, or failed.
export function runState(run: Pick<ReviewRun, "review" | "error">): "running" | "done" | "failed" {
  if (run.review) return "done";
  return run.error ? "failed" : "running";
}

/// The most findings one review gives comments, as the node takes at most.
const MAX_FINDINGS = 30;
const MAX_BODY = 600;

/// One finding, as a comment is made of it.
export interface Finding {
  path: string;
  side: "old" | "new";
  line: number | null;
  body: string;
}

const CONTROL = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g;

/// Text from the agent as a comment holds it: no control characters, cut
/// at the longest a comment goes. It is shown as text and never read as markup
/// or as an instruction.
function cleanBody(s: string) {
  return s.replace(CONTROL, "").trim().slice(0, MAX_BODY * 2);
}

/// A path as the diff names it: relative, without `./` or the checkout's
/// own root in front. A leading `a/` or `b/` stays, as it may be a folder.
function cleanPath(path: string, root: string) {
  let p = path.trim().replace(/\\/g, "/");
  const r = root.replace(/\\/g, "/").replace(/\/+$/, "");
  if (r && p.startsWith(`${r}/`)) p = p.slice(r.length + 1);
  return p.replace(/^\.\//, "");
}

/// A finding's comment: its severity and summary, then how it fails.
function body(f: CodeFinding) {
  const head = `${f.severity}: ${f.summary.trim()}`;
  const scenario = f.scenario.trim();
  return cleanBody(scenario ? `${head}\n\n${scenario}` : head);
}

/// The findings of a submitted review as comments are made of them, on the
/// new side of the diff; one with no file or text is left out.
export function findingsOf(review: SubmittedReview, root = ""): Finding[] {
  const out: Finding[] = [];

  for (const f of review.findings.slice(0, MAX_FINDINGS)) {
    const path = cleanPath(f.file, root).replace(CONTROL, "");
    const text = body(f);
    if (!path || path.length > 512 || !f.summary.trim()) continue;
    const line = Number.isInteger(f.line) && (f.line ?? 0) > 0 ? f.line : null;
    out.push({ path, side: "new", line, body: text });
  }

  return out;
}

/// The verdict as a few words: "approves", "asks for changes", "comments".
function verdictWords(verdict: string) {
  if (verdict === "approve") return "approves";
  if (verdict === "changes") return "asks for changes";
  return "comments";
}

// ----- Draft comments -----

/// The lines `f` has, by side and number, as comments are placed by.
function linesOf(f: DiffFile) {
  const keys = new Set<string>();
  for (const r of diffRows(f)) if (r.kind === "line") keys.add(r.k === "-" ? lineKey("old", r.old!) : lineKey("new", r.new!));
  return keys;
}

/// The draft comments for `findings`, written by `agent`. One on a line the
/// diff has sits on that line; one on a line it does not have becomes a
/// comment on its file, starting with the line; one on a file the diff
/// does not have stays as it is, and shows under the files that are gone.
export function toComments(findings: readonly Finding[], files: readonly DiffFile[], agent: string): ReviewComment[] {
  const by = new Map(files.map(f => [f.path, f]));
  const have = new Map<string, Set<string>>();
  return findings.map(f => {
    const file = by.get(f.path);
    if (f.line == null) return { path: f.path, side: "new", line: null, text: f.body, by: agent };
    if (!file) return { path: f.path, side: f.side, line: f.line, text: f.body, by: agent };
    let keys = have.get(f.path);
    if (!keys) have.set(f.path, keys = linesOf(file));
    if (keys.has(lineKey(f.side, f.line))) return { path: f.path, side: f.side, line: f.line, text: f.body, by: agent };
    const where = f.side === "old" ? `Old line ${f.line}` : `Line ${f.line}`;
    return { path: f.path, side: "new", line: null, text: `${where}: ${f.body}`, by: agent };
  });
}

/// Whether two comments say the same thing in the same place.
const same = (a: ReviewComment, b: ReviewComment) => a.path === b.path && a.side === b.side && a.line === b.line && a.text === b.text;

/// `add` after `draft`, without a comment that is already in it (or twice in `add`),
/// and how many were new.
export function mergeComments(draft: readonly ReviewComment[], add: readonly ReviewComment[]) {
  const out = draft.slice();
  let added = 0;

  for (const c of add) {
    if (out.some(x => same(x, c))) continue;
    out.push(c);
    added++;
  }

  return { draft: out, added };
}

/// What the header says once a review is in: "Claude Code asks for changes, 2 comments added".
export function doneNote(agentName: string, verdict: string, added: number, total: number) {
  const head = `${agentName} ${verdictWords(verdict)}`;
  if (!total) return `${head}, nothing to report`;
  if (!added) return `${head}, no new comments`;
  return `${head}, ${added} ${added === 1 ? "comment" : "comments"} added`;
}

// ----- The button -----

/// What decides whether a review can start: the machine the diff is on and
/// what it offers, and the checkout.
export interface CanStart {
  /// The machine the diff's checkout is on; null for a diff with none.
  machine: { id: string; name: string } | null;
  /// The checkout the diff was read from, once it has been read.
  root: string | null;
  /// The machines that run coding agents, as read; null until read.
  hosts: readonly { machine_id: string; agents: readonly string[] }[] | null;
  /// Whether the diff has any change to review.
  changes: boolean;
}

/// Why a review cannot start, or "" when it can.
export function whyNot(c: CanStart): string {
  if (!c.machine) return "This diff has no checkout on a machine to review";
  if (c.root == null) return "Reading the changes";
  if (!c.changes) return "There are no changes to review";
  if (!c.hosts) return "Looking for coding agents";
  const host = c.hosts.find(h => h.machine_id === c.machine!.id);
  if (!host) return `${c.machine.name} is offline, so the checkout cannot be read`;
  if (!host.agents.length) return `${c.machine.name} has no coding agent`;
  return "";
}
