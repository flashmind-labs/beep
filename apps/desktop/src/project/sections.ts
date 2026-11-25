import { prSession } from "./pr-session.ts";
// A project page's sections (terminals, file sync, Git history, pull
// requests and issues, runs).
import { drawRail } from "../center/sidebar.tsx";
import { projectRows } from "./projectrows.ts";
import { railProject, shell } from "../center/window-shell.tsx";
import { pageShown } from "../ui/active.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedEach } from "../ui/snapshot-state.ts";
import { layoutChanged } from "../ui/skin.ts";
import { formatAgo } from "../ui/format.ts";
import { onPlaces, placeOf } from "../agents/place.ts";
import { onMoves } from "../agents/move-store.ts";
import { onClosing, sessionVisible, shownStatus } from "../agents/closing.ts";
import { invoke, indexedChannel, listen, openUrl } from "../ui/tauri.ts";
import { localProjectId } from "./localid.ts";
import type { Project, SessionRow } from "./projects-state.ts";
import { followedProject, makeProjectLive, type LiveRow } from "./projectlive.ts";
import type { BarTarget, TaskSession } from "./projectbar.ts";
import { type BaseBranches, DEFAULT_CHECKOUT } from "../agents/agentpick.ts";
import type { Picked } from "../agents/agentpicker.tsx";
import type { AgentHost } from "../agents/agents-state.ts";
import { lazyScreen, makeTerminals } from "../terminals/terminals.tsx";
import type { BranchSessionRow } from "../review/history-state.ts";
import { makeGitHistory } from "../review/history.tsx";
import { sessions } from "../agents/sessions.ts";
import { showChanges } from "../agentpane/media.ts";
import { makeFileSync, type SyncDetail } from "./filesync.ts";
import { makeThreads, type ListsRow, type IssueRow, type PullRequestRow, type RepoActs } from "./threads.ts";
import { makeThreadDetail } from "./threaddetail.tsx";
import { makeRuns, type RunDetail, type RunsRow } from "./runs.tsx";
import { state, keptSection, sessionsOf } from "../center/state.ts";
import { currentLocation, go, drawTop } from "../center/nav.ts";
import { openTile } from "./split.tsx";
import { AG } from "../agents/agents-state.ts";
import { barOn, drawProjectComp, workOnIssue, startIssue, discussThread, workOnPr, startPrWork, reviewPr, startReview, pickButton, earlierReview, issueSession, fixSession, sessionState, fixRun, startFix, fixConflict, projectButtons, startConflictFix, startSyncRepair } from "./composer.tsx";
import type { ProjectPick } from "../agents/agentmodels.ts";
import { redrawSync } from "../center/redraw.ts";
import { drawProjects } from "./page.tsx";
import { rememberGitRow } from "./loads.tsx";
import { assign, teamLogins } from "./team-view.tsx";

/// The row of the project's list picked, null for the first, and a run to
/// open on the Actions tile.
/// The tile picked, a run to open, and whether the project's board is blank,
/// which centers the agent bar on the page under the day's painting.
export const pa = { tile: null as string | null, run: null as { project: string; id: number; host: string } | null, blank: false };

export const terms = makeTerminals({ invoke, channel: indexedChannel, makeScreen: () => lazyScreen(() => import("../terminals/termview.ts").then(m => m.makeScreen)), ago: ms => formatAgo(ms, "short"), redraw: () => redrawProjects(), kept: keptSection("terminals") });
export const fsync = makeFileSync({
  invoke, redraw: () => { redrawProjects(); redrawSync(); }, fix: fixConflict, kept: keptSection("filesync"),
  fixButton: (project, c) => projectButtons(`conflict ${project}\n${c.path}`, { project, machine: c.holder, choose: false, label: "Fix with an agent", hint: "Pick a model: an agent merges the two copies on the machine that has them", session: null, start: pick => startConflictFix(project, c, pick) }),
  repairButton: (project, machine, error) => repairButton(project, machine, error),
});

// Each project's sync detail comes with the `sync` updates, as the machines hold it.
subscribeEach(topic("sync"), event => {
  for (const { args, pushed } of pushedEach<SyncDetail | null>(event.data, "project_sync_detail")) {
    if (typeof args.project === "string" && "value" in pushed && pushed.value) fsync.take(args.project, pushed.value);
  }
});

/// Fix with an agent on a blocked Git sync of `project` on `machine`.
export function repairButton(project: string, machine: string, error: string) {
  return projectButtons(`repair ${project}\n${machine}`, { project, machine, choose: false, label: "Fix with an agent", hint: "Pick a model: an agent starts here with both checkouts and recent sessions in its prompt", session: null, start: pick => startSyncRepair(project, machine, error, pick) });
}

export const gitHistory = makeGitHistory({
  invoke,
  back: () => openTile("overview"),
  sessions: projectSessions,
  openSession: openBranchSession,
  changedCheckout: (project, row) => { rememberGitRow(project, row); redrawProjectsPage(); },
});

/// Whether the project shown is live or polling; what its host changed is
/// read again as the mesh says.
export const liveMode = makeProjectLive({
  invoke,
  redraw: () => redrawProjects(),
  reload: (id, update) => {
    const lists = update.lists && update.view<ListsRow | null>("center_project_lists", { projectId: id });
    if (lists) thr.take(id, lists);

    if (update.runs) runs.take(id, update.view<RunsRow>("center_project_runs", { projectId: id }),
      run => update.view<RunDetail>("center_project_run", { projectId: id, runId: run.id, host: run.host ?? null }));
  },
});
listen<LiveRow>("project-live", e => liveMode.heard(e.payload));

/// The project's open pull requests and issues, for the project page's list.
export const thr = makeThreads({
  invoke,
  redraw: () => redrawProjects(),
  openUrl,
  kept: keptSection("threads"),
});

/// A pull request or issue read in full, in the split's right column.
export const details = makeThreadDetail({ redraw: () => redrawProjects() });

/// The project's workflow runs: the Actions section and each PR's checks.
export const runs = makeRuns({
  invoke,
  redraw: () => redrawProjects(),
  openUrl,
  fix: (detail: RunDetail) => { if (state.picked) fixRun(state.picked, detail); },
  startFix: (name, detail, pick, machine) => startFix(name, detail, pick, machine),
  fixSession: (name, detail) => sessionState(fixSession(name, detail), "Fixing"),
  button: (key, o) => projectButtons(key, o),
  mode: liveMode.mode,
  kept: keptSection("runs"),
  hostsChanged: () => redrawProjectsPage(),
});

layoutChanged.add(() => redrawProjects());
onPlaces(() => redrawProjects());
onMoves(() => redrawProjects());
onClosing(() => redrawProjects());

/// Follows the imported project this window shows, on its page or in its rail, while visible.
export function syncProjectFollow() {
  const project = followedProject(
    pageShown(),
    currentLocation()?.view ?? null,
    state.picked,
    railProject(shell.getState().side.rail),
    projectRows(),
  );
  void liveMode.follow(project ? localProjectId(project) : null);
}

export function redrawProjects() {
  const view = currentLocation()?.view;
  if (view === "projects") drawProjects();

  // The project's sessions beside one of them or beside its page show its card.
  if (view === "pane" || railProject(shell.getState().side.rail)) drawRail();
  // The empty agent view has the same bar.
  if (view !== "projects" && AG.draft && barOn(AG.draft)) drawProjectComp(AG.draft);
}

/// The project's sessions with their latest status and reported worktree.
function projectSessions(project: string): BranchSessionRow[] | undefined {
  const listed = sessionsOf(project);
  if (!listed) return undefined;
  const key = (s: SessionRow) => `${s.machine_id}:${s.agent}:${s.id}`;
  const latest = new Map(sessions.map(s => [key(s), s]));
  return listed.filter(sessionVisible).map(s => {
    const live = latest.get(key(s));
    const session = live && live.last_activity >= s.last_activity ? live : s;
    return { ...session, status: shownStatus(session), worktree: placeOf(session)?.folder };
  });
}

function openBranchSession(session: SessionRow, changes = false) {
  go("pane", { agent: session });
  if (changes) showChanges();
}

/// The Projects page and its header, while it shows.
export function redrawProjectsPage() {
  if (currentLocation()?.view === "projects") { drawProjects(); drawTop(); }
}

/// What the Pull requests and Issues sections do on project `p`'s page:
/// Work on it and Work on PR fill the bar.
export function repoActs(p: Project): RepoActs {
  return {
    workOn: (issue: IssueRow) => workOnIssue(p.name, issue),
    startIssue: (issue: IssueRow, pick: ProjectPick, machine: string | null) => startIssue(p.name, issue, pick, machine),
    discuss: topic => discussThread(p.name, topic),
    prSession,
    workOnPr: (pr: PullRequestRow) => workOnPr(p.name, pr),
    startPrWork: (pr: PullRequestRow, pick: ProjectPick, machine: string | null) => startPrWork(p.name, pr, pick, machine),
    openSession: (session: TaskSession) => go("pane", { agent: session }),
    reviewPr: (pr: PullRequestRow) => reviewPr(p.name, pr),
    startReview: (pr: PullRequestRow, pick: ProjectPick, machine: string | null) => startReview(p.name, pr, pick, machine),
    button: start => pickButton(p.name, start),
    reviewOf: (pr: PullRequestRow) => earlierReview(p.name, pr.number),
    issueSession: (issue: IssueRow) => sessionState(issueSession(p.name, issue), "Working"),
    assignees: () => teamLogins(p.name),
    assign: (issue: IssueRow, login: string) => assign(p.name, issue.number, [login]),
  };
}

/// The bar under a project: it starts a coding agent in the project's checkout,
/// by default as it is (the agent makes its own branch), and opens its session.
export const pc = {
  project: null as string | null,
  /// The machine picked; the bar starts on another while it has no checkout
  /// and agent there.
  machine: "",
  /// The machine the project is being added to, until it is ready.
  adding: null as AgentHost | null,
  checkout: DEFAULT_CHECKOUT,
  branches: new Map<string, BaseBranches | null>(),
  /// True while the bar's files upload and its start is handed over.
  starting: false,
  /// The harness, model and thinking picked, and the machine and harness
  /// the model was picked for, as `machine/agent`.
  picked: { agent: "", model: "", effort: null, fast: null, query: "" } as Picked,
  modelFor: "",
  /// Whether the picker shows.
  open: false,
  /// What Work on it or Work on PR put in the bar, until it starts or is
  /// cleared.
  target: null as BarTarget | null,
};

