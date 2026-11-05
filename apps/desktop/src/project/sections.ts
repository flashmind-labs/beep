// A project page's sections (terminals, file sync, reviews, threads, runs)
// and the acts its agent's cards call.
import { ago } from "../ui/format.ts";
import { dbtn as button } from "../ui/parts.ts";
import { invoke, Channel, listen } from "../ui/tauri.ts";
import { el as h } from "../ui/dom.ts";
import type { Project } from "./projects-state.ts";
import { makeProjectLive, type LiveRow } from "./projectlive.ts";
import { agentFor, pageAgent, workerSession, type ProjectAgent } from "./agent.ts";
import type { BarTarget } from "./projectbar.ts";
import { type BaseBranches, DEFAULT_CHECKOUT } from "../agents/agentpick.ts";
import type { Picked } from "../agents/agentpicker.ts";
import type { AgentHost, AgentSession } from "../agents/agents.ts";
import { makeTerminals } from "../terminals/terminals.ts";
import { makeScreen } from "../terminals/termview.ts";
import { branchSession } from "../review/branchsession-state.ts";
import { loadSessions, sessions } from "../agents/sessions.ts";
import { showChanges } from "../agentpane/media.ts";
import { makeReviews, mergeBlock, type ReviewHead } from "../review/review.ts";
import { makeFileSync } from "./filesync.ts";
import { makeThreads, type IssueRow, type PullRequestRow, type RepoActs } from "./threads.ts";
import { makeThreadDetail } from "./threaddetail.ts";
import { makeRuns, type RunDetail } from "./runs.ts";
import { type Outcome, state, keptSection, views } from "../center/state.ts";
import { failure } from "../ui/pieces.ts";
import { openUrl } from "../center/openitems.ts";
import { shown, ownChats, go, drawTop } from "../center/views.ts";
import { reloadChats } from "../center/sidebar.ts";
import { openTile } from "./split.ts";
import { workOnIssue, continuePr, fixRun } from "./composer.ts";
import { loadProjectAgent } from "./loads.ts";
import { drawProjects } from "./page.ts";
import { loadThreadChats, threadChats } from "../center/threadcard.ts";

/// What the agent sections are doing: `busy` and `errors` by task or worker
/// id, `outcomes` of answered cards by query id. `tile` is the row of the
/// project's list picked, null for the first.
export const pa = { busy: new Set<string>(), errors: new Map<string, string>(), outcomes: new Map<string, Outcome>(), tile: null as string | null };

export const terms = makeTerminals({ invoke, Channel, makeScreen, h, button, ago, redraw: () => redrawProjects(), kept: keptSection("terminals") });
export const fsync = makeFileSync({ invoke, h, button, redraw: () => redrawProjects(), kept: keptSection("filesync") });

/// The project's branches and their reviews: the Branches list and the
/// review view.
export const revs = makeReviews({
  invoke, h, button, ago,
  redraw: () => redrawProjects(),
  openUrl,
  openTile: (key: string) => openTile(key),
  machines: () => state.home?.machines || [],
  diff: viewDiff,
  kept: keptSection("branches"),
});

/// Whether the project shown is live or polling; what its host changed is
/// read again as the server says.
export const liveMode = makeProjectLive({
  invoke, h,
  redraw: () => redrawProjects(),
  reload: (id: string, lists: boolean, ran: boolean) => {
    if (lists) thr.load(id);
    if (ran) runs.load(id);
  },
});
listen<LiveRow>("project-live", e => liveMode.heard(e.payload));

/// The project's open pull requests and issues, and the user's threads
/// there, for the project page's list.
export const thr = makeThreads({
  invoke, h, button, ago,
  redraw: () => redrawProjects(),
  openUrl,
  openChat: (chat: string) => { ownChats.add(chat); go("pane", { chat: { session: chat } }); reloadChats(); if (!threadChats?.has(chat)) loadThreadChats(); },
  changed: () => loadProjectAgent(),
  kept: keptSection("threads"),
});

/// A pull request or issue read in full, in the split's right column.
export const details = makeThreadDetail({ redraw: () => redrawProjects(), openUrl });

/// The project's workflow runs: the Actions section and each PR's checks.
export const runs = makeRuns({
  invoke, h, button,
  redraw: () => redrawProjects(),
  openUrl,
  fix: (detail: RunDetail) => { if (state.picked) fixRun(state.picked, detail); },
  mode: liveMode.chip,
  kept: keptSection("runs"),
});

/// Every 10 seconds while a project page shows: the runs move on, but only
/// while one goes.
export function tickRepo(name: string) {
  const agent = pageAgent(state.agents, name, state.subscribed);
  if (agent) runs.tick(agent.project_id);
}

export function projectsEl() { return views.get("projects")!.el; }

export function redrawProjects() {
  if (shown === "projects") drawProjects(projectsEl());
}

/// View diff: the session working on `branch`, if there is one, as a function that
/// opens it on the Changes tab, zoomed. A branch with no session has no diff to show.
function viewDiff(project: string, branch: string) {
  const data = revs.branches(project);
  const worktree = data?.branches.find(b => b.name === branch)?.worktree;
  const session = branchSession(sessions, data?.machine_id, worktree);
  if (!session) return null;
  return () => {
    go("pane", { agent: session });
    showChanges();
  };
}

/// The Projects page and its header, while it shows.
export function redrawProjectsPage() {
  if (shown === "projects") { drawProjects(projectsEl()); drawTop(); }
}

export const agentActs = {
  busy: (id: string) => pa.busy.has(id),
  error: (id: string) => pa.errors.get(id),
  outcome: (id: string) => pa.outcomes.get(id),
  openUrl,
  review: (branch: string) => { if (state.picked) revs.openBranch(state.picked, branch); },
  diff: (branch: string) => (state.picked ? viewDiff(state.picked, branch) : null),
  /// Merge… on a task in review asks the project agent to merge once the
  /// branch's newest review allows it and the branch has not moved since;
  /// otherwise it opens the review, which says what stands in the way.
  merge(task: { id: string; branch: string | null }) {
    const name = state.picked, branch = task.branch;
    if (!name || !branch) return;
    const r = (revs.reviews(name) || []).find(x => x.branch === branch);
    if (!r || !mergeBlock(r).startsWith("Merge always")) { revs.openBranch(name, branch); return; }
    return act(task.id, async () => {
      const now = await invoke<ReviewHead | null>("review_check", { id: r.id });
      if (!now || now.stale) { revs.openBranch(name, branch); return; }
      await invoke("review_merge", { review: r.id });
    });
  },
  openWorker(worker: Parameters<typeof workerSession>[0], title: string | null) {
    const at = workerSession(worker, title);
    if (at) go("pane", { agent: at });
  },
  async change(task: { id: string }, change: unknown) {
    await act(task.id, () => invoke("center_update_task", { task: task.id, change }));
  },
  async stop(worker: { id: string }) {
    await act(worker.id, () => invoke("center_stop_worker", { worker: worker.id }));
  },
};

/// Runs `call` for row `id`, showing it busy, then reloads the project.
export async function act(id: string, call: () => Promise<unknown>) {
  if (pa.busy.has(id)) return;
  pa.busy.add(id);
  pa.errors.delete(id);
  redrawProjects();
  try {
    await call();
  } catch (e) {
    pa.errors.set(id, failure(e));
  } finally {
    pa.busy.delete(id);
  }
  redrawProjects();
  loadProjectAgent();
}

/// What the Pull requests and Issues sections do on project `p`'s page:
/// Review opens a branch's review, Work on it and Continue fill the bar.
export function repoActs(p: Project): RepoActs {
  const view = state.pview.get(p.name)?.view;
  const agent = pageAgent(state.agents, p.name, state.subscribed);
  revs.ensure(p.name);
  return {
    tasks: view?.tasks ?? [],
    reviews: revs.reviews(p.name),
    review: agentActs.review,
    workOn: (issue: IssueRow) => workOnIssue(p.name, issue),
    continuePr: (pr: PullRequestRow) => continuePr(p.name, pr),
    checks: agent ? runs.checks(agent.project_id) : null,
    showRun: (run: number, host?: string | null) => {
      if (!agent) return;
      runs.show(agent.project_id, run, host);
      document.getElementById("sec-actions")?.scrollIntoView({ block: "nearest" });
    },
  };
}

/// Answers a card of the project chat. The call returns when the run it
/// resumes ends, so the outcome shows now and the view reloads after.
export function answerCard(agent: ProjectAgent, c: { query_id: string }, decision: string, said: string) {
  pa.outcomes.set(c.query_id, { said });
  redrawProjects();
  invoke("answer_approval", { session: agent.chat_id, queryId: c.query_id, decision, onEvent: new Channel() })
    .catch(e => pa.outcomes.set(c.query_id, { error: failure(e) }))
    .finally(() => { redrawProjects(); loadProjectAgent(); });
  // The tasks a plan starts show before its run ends.
  setTimeout(loadProjectAgent, 1500);
}

/// The bar under a project: it starts a coding agent in the project's checkout,
/// by default as it is (the agent makes its own branch), and opens its session.
export const pc = {
  project: null as string | null,
  machine: "auto",
  checkout: DEFAULT_CHECKOUT,
  hosts: null as AgentHost[] | null,
  branches: new Map<string, BaseBranches | null>(),
  starting: false,
  /// The harness, model and thinking picked, and the machine and harness
  /// the model was picked for, as `machine/agent`.
  picked: { agent: "", model: "", effort: null, query: "" } as Picked,
  modelFor: "",
  /// Whether the picker shows.
  open: false,
  /// What Work on it or Continue put in the bar, until it starts or is
  /// cleared.
  target: null as BarTarget | null,
};

