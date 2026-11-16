// A project page's sections (terminals, file sync, Git history, pull
// requests and issues, runs).
import { layoutChanged } from "../ui/skin.ts";
import { formatAgo } from "../ui/format.ts";
import { onPlaces, placeOf } from "../agents/place.ts";
import { onMoves } from "../agents/move-store.ts";
import { onClosing, sessionVisible, shownStatus } from "../agents/closing.ts";
import { invoke, indexedChannel, listen } from "../ui/tauri.ts";
import type { Project, SessionRow } from "./projects-state.ts";
import { makeProjectLive, type LiveRow } from "./projectlive.ts";
import type { BarTarget, BlockedHost, ReviewSession } from "./projectbar.ts";
import { type BaseBranches, DEFAULT_CHECKOUT } from "../agents/agentpick.ts";
import type { Picked } from "../agents/agentpicker.tsx";
import type { AgentHost} from "../agents/agents.ts";
import { lazyScreen, makeTerminals } from "../terminals/terminals.tsx";
import type { BranchSessionRow } from "../review/history-state.ts";
import { makeGitHistory } from "../review/history.tsx";
import { sessions } from "../agents/sessions.ts";
import { showChanges } from "../agentpane/media.ts";
import { makeFileSync } from "./filesync.ts";
import { makeThreads, type IssueRow, type PullRequestRow, type RepoActs } from "./threads.ts";
import { makeThreadDetail } from "./threaddetail.tsx";
import { makeRuns, type RunDetail } from "./runs.tsx";
import { state, keptSection } from "../center/state.ts";
import { openUrl } from "../center/openitems.ts";
import { currentLocation, go, drawTop } from "../center/views.tsx";
import { openTile } from "./split.tsx";
import { AG } from "../agents/agents.ts";
import { barOn, drawProjectComp, workOnIssue, startIssue, continuePr, startContinue, reviewPr, startReview, pickButton, earlierReview, fixRun, startFix, fixConflict, projectButtons, startConflictFix, startSyncRepair } from "./composer.tsx";
import type { ProjectPick } from "../agents/agentmodels.ts";
import type { Starting } from "../agents/draft-state.ts";
import { redrawSync } from "./syncpage.ts";
import { drawProjects } from "./page.tsx";

/// The row of the project's list picked, null for the first, and a run to
/// open on the Actions tile.
/// The tile picked, a run to open, and whether the project's board is blank,
/// which centers the agent bar on the page under the day's painting.
export const pa = { tile: null as string | null, run: null as { project: string; id: number; host: string } | null, blank: false };

export const terms = makeTerminals({ invoke, channel: indexedChannel, makeScreen: () => lazyScreen(() => import("../terminals/termview.ts").then(m => m.makeScreen)), ago: ms => formatAgo(ms, "short"), redraw: () => redrawProjects(), kept: keptSection("terminals") });
export const fsync = makeFileSync({
  invoke, redraw: () => { redrawProjects(); redrawSync(); }, fix: fixConflict, kept: keptSection("filesync"),
  fixButton: (project, c) => projectButtons(`conflict ${project}\n${c.path}`, { project, machine: c.holder, label: "Fix with an agent", hint: "Pick a model: an agent merges the two copies on the machine that has them", running: null, start: pick => startConflictFix(project, c, pick) }),
  repairButton: (project, machine, error) => repairButton(project, machine, error),
});

/// Fix with an agent on a blocked Git sync of `project` on `machine`.
export function repairButton(project: string, machine: string, error: string) {
  return projectButtons(`repair ${project}\n${machine}`, { project, machine, label: "Fix with an agent", hint: "Pick a model: an agent starts here with both checkouts and recent sessions in its prompt", running: null, start: pick => startSyncRepair(project, machine, error, pick) });
}

export const gitHistory = makeGitHistory({
  invoke,
  back: () => openTile("overview"),
  sessions: projectSessions,
  openSession: openBranchSession,
});

/// Whether the project shown is live or polling; what its host changed is
/// read again as the mesh says.
export const liveMode = makeProjectLive({
  invoke,
  redraw: () => redrawProjects(),
  reload: (id: string, lists: boolean, ran: boolean) => {
    if (lists) thr.load(id);
    if (ran) runs.load(id);
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
export const details = makeThreadDetail({ redraw: () => redrawProjects(), openUrl });

/// The project's workflow runs: the Actions section and each PR's checks.
export const runs = makeRuns({
  invoke,
  redraw: () => redrawProjects(),
  openUrl,
  fix: (detail: RunDetail) => { if (state.picked) fixRun(state.picked, detail); },
  startFix: (name, detail, pick) => startFix(name, detail, pick),
  button: (key, o) => projectButtons(key, o),
  mode: liveMode.mode,
  kept: keptSection("runs"),
  hostsChanged: () => redrawProjectsPage(),
});

layoutChanged.add(() => redrawProjects());
onPlaces(() => redrawProjects());
onMoves(() => redrawProjects());
onClosing(() => redrawProjects());

export function redrawProjects() {
  if (currentLocation()?.view === "projects") drawProjects();
  // The empty agent view has the same bar.
  else if (AG.draft && barOn(AG.draft)) drawProjectComp(AG.draft);
}

/// The project's sessions with their latest status and reported worktree.
function projectSessions(project: string): BranchSessionRow[] | undefined {
  const listed = state.sessions.get(project);
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
/// Work on it and Continue fill the bar.
export function repoActs(p: Project): RepoActs {
  return {
    workOn: (issue: IssueRow) => workOnIssue(p.name, issue),
    startIssue: (issue: IssueRow, pick: ProjectPick) => startIssue(p.name, issue, pick),
    continuePr: (pr: PullRequestRow) => continuePr(p.name, pr),
    startContinue: (pr: PullRequestRow, pick: ProjectPick) => startContinue(p.name, pr, pick),
    reviewPr: (pr: PullRequestRow) => reviewPr(p.name, pr),
    startReview: (pr: PullRequestRow, pick: ProjectPick) => startReview(p.name, pr, pick),
    button: start => pickButton(p.name, start),
    reviewOf: (pr: PullRequestRow) => earlierReview(p.name, pr.number),
    openReview: (session: ReviewSession) => go("pane", { agent: session }),
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
  hosts: null as AgentHost[] | null,
  /// The machines with a coding agent installed that Beep cannot run yet.
  blocked: [] as BlockedHost[],
  branches: new Map<string, BaseBranches | null>(),
  /// The start the bar sent, until its machine answers.
  starting: null as Starting | null,
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

