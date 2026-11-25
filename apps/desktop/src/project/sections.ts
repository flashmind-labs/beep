import { prSession } from "./pr-session.ts";
// A project page's sections (terminals, file sync, Git history, pull
// requests and issues, runs).
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
import type { TaskSession } from "./projectbar.ts";
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
import { currentLocation, go } from "../center/nav.ts";
import { openTile } from "./split.tsx";
import { workOnIssue, startIssue, discussThread, workOnPr, startPrWork, reviewPr, startReview, pickButton, earlierReview, issueSession, fixSession, sessionState, fixRun, startFix, fixConflict, projectButtons, startConflictFix, startSyncRepair } from "./composer.tsx";
import type { ProjectPick } from "../agents/agentmodels.ts";
import { redraw, redrawSync } from "../center/redraw.ts";
import { rememberGitRow } from "./loads.tsx";
import { assign, teamLogins } from "./team-view.tsx";

export const terms = makeTerminals({ invoke, channel: indexedChannel, makeScreen: () => lazyScreen(() => import("../terminals/termview.ts").then(m => m.makeScreen)), ago: ms => formatAgo(ms, "short"), redraw: () => redraw("projects"), kept: keptSection("terminals") });
export const fsync = makeFileSync({
  invoke, redraw: () => { redraw("projects"); redrawSync(); }, fix: fixConflict, kept: keptSection("filesync"),
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
  changedCheckout: (project, row) => { rememberGitRow(project, row); redraw("projectsPage"); },
});

/// Whether the project shown is live or polling; what its host changed is
/// read again as the mesh says.
export const liveMode = makeProjectLive({
  invoke,
  redraw: () => redraw("projects"),
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
  redraw: () => redraw("projects"),
  openUrl,
  kept: keptSection("threads"),
});

/// A pull request or issue read in full, in the split's right column.
export const details = makeThreadDetail({ redraw: () => redraw("projects") });

/// The project's workflow runs: the Actions section and each PR's checks.
export const runs = makeRuns({
  invoke,
  redraw: () => redraw("projects"),
  openUrl,
  fix: (detail: RunDetail) => { if (state.picked) fixRun(state.picked, detail); },
  startFix: (name, detail, pick, machine) => startFix(name, detail, pick, machine),
  fixSession: (name, detail) => sessionState(fixSession(name, detail), "Fixing"),
  button: (key, o) => projectButtons(key, o),
  mode: liveMode.mode,
  kept: keptSection("runs"),
  hostsChanged: () => redraw("projectsPage"),
});

layoutChanged.add(() => redraw("projects"));
onPlaces(() => redraw("projects"));
onMoves(() => redraw("projects"));
onClosing(() => redraw("projects"));

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
