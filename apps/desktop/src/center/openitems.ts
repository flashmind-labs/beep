// Home's Open PRs and issues tab, and importing a repo from it.
import { reason } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { importFound } from "../addproject/addproject-state.ts";
import { keepLastImport, lastImport } from "../addproject/importproject-state.ts";
import { openScreen } from "./nav.ts";
import { foundFor, type ProjectRow } from "../project/projects-state.ts";
import { projectStart, projectsWithOpen, replyStart, type GitItemRow, type GitItemsView, type Hosts, type StartProject } from "./openitems-state.ts";
import { agentButtons, earlierReview, startPrWork, startReview, workOnIssue } from "../project/composer.tsx";
import type { MenuItem } from "../ui/menu.tsx";
import { AGENT_ACTIVE } from "../agents/sessionmenu-state.ts";
import { agentLabel } from "../project/projects-state.ts";
import type { AgentButtonItem } from "../review/feedback-view.tsx";
import { state, homeDataChanged } from "./state.ts";
import { currentLocation, go } from "./nav.ts";
import { loadFound } from "../project/addflow.ts";
import { single } from "../ui/active.ts";
import type { Pushed } from "../ui/snapshot-state.ts";
import { prSession } from "../project/pr-session.ts";

const redrawHome = () => homeDataChanged.notify();
const homeButtons = agentButtons(redrawHome);

/// Review with agent on a PR of `project`: starts the reviewer at once in
/// the project's checkout, on an online machine with it. Review again goes
/// to the session that reviewed it before, which the button opens while it
/// works.
export function reviewButton(item: GitItemRow, project: StartProject, machines: Hosts): AgentButtonItem {
  const machine = projectStart(project, machines).machine;
  const earlier = earlierReview(project.name, item.number);
  const pr = { number: item.number, title: item.title, url: item.url, mine: item.authored };
  return homeButtons(`review ${item.url}`, {
    project: project.name, machine, choose: true,
    label: earlier ? "Review again" : "Review",
    hint: earlier ? "Reviews what changed in the chat that reviewed this PR before" : "Pick a model: an agent reviews this PR and posts its comments on it",
    session: earlier && AGENT_ACTIVE.has(earlier.status) ? { kind: "running", label: "Reviewing", agent: agentLabel(earlier.agent), hint: "Open the review", open: () => go("pane", { agent: earlier }) } : null,
    start: (pick, at) => startReview(project.name, pr, pick, at),
  });
}

/// Continue opens the session that created the PR. Work on PR starts a session on its branch.
export function continueButton(item: GitItemRow, project: StartProject, machines: Hosts): AgentButtonItem {
  const machine = projectStart(project, machines).machine;
  const session = prSession(item.url, redrawHome);
  return homeButtons(`continue ${item.url}`, {
    project: project.name, machine, choose: true, label: "Work on PR", hint: "Start an agent on this pull request's branch",
    session: session === "checking" ? { kind: "checking" } : session
      ? { kind: "done", label: "Continue", hint: "Open the session that created this PR", open: () => go("pane", { agent: session }) } : null,
    start: (pick, at) => startPrWork(project.name, item, pick, at),
  });
}

/// The row menu of an item of `project`: Fix with agent fills the project's
/// bar to work on an issue, Reply with agent opens the start form, and Open
/// project shows the project's issues or pull requests.
export function itemMenu(item: GitItemRow, project: StartProject, machines: Hosts): MenuItem[] {
  const reply = { label: "Reply with agent", on: () => go("pane", { agentnew: { ...replyStart(item, project, machines), assistant: false } }) };
  const open = { label: "Open project", on: () => go("projects", { project: project.name, tile: item.kind === "issue" ? "issues" : "prs" }) };
  if (item.kind === "pull_request") return [reply, open];

  const fix = { label: "Fix with agent", on: () => { go("projects", { project: project.name }); workOnIssue(project.name, item); } };
  return [fix, reply, { kind: "separator" }, open];
}

export type OpenRepo = ReturnType<typeof projectsWithOpen>[number];

/// Repositories being imported from their row, by key, and why one failed.
export const rowImports = new Map<string, "busy" | { error: string }>();

/// What `import_project` takes for an open-items repository: `org/repo`
/// on GitHub, its URL elsewhere.
function importRepo(p: OpenRepo) {
  return p.host === "github.com" ? p.repo : p.url;
}

/// The machine Import puts it on, in its projects folder: the last one used,
/// else the first online.
function importMachine() {
  const machines = (state.home?.machines || []).filter(m => m.online && m.can_host !== false);
  const last = lastImport();
  return machines.find(m => m.id === last?.machine) || machines[0] || null;
}

/// Imports from the row with the last machine and folder; the row turns
/// into the project once it is in. With no machine online, the sheet.
export async function quickImport(p: OpenRepo) {
  if (rowImports.get(p.key) === "busy") return;
  const machine = importMachine();
  if (!machine) return importWithSettings(p);
  rowImports.set(p.key, "busy");
  homeDataChanged.notify();

  try {
    // A checkout the machines found imports as it is, not as a second clone.
    if (!state.found) await loadFound();
    const here = foundFor(state.found, p.repo);
    if (here) await importFound(here);
    else keepLastImport(await invoke<ProjectRow>("import_project", { repo: importRepo(p), machineId: machine.id, folder: null, worktrees: [] }));
    rowImports.delete(p.key);
  } catch (e) {
    const error = reason(e);
    rowImports.set(p.key, { error });
    console.warn(`importing ${p.repo}:`, error);
  }

  homeDataChanged.notify();
}

/// The Import a project sheet for the row's repository, on the last
/// machine used.
function importWithSettings(p: OpenRepo) {
  openScreen({ screen: "importproject", repo: importRepo(p), machine: importMachine()?.id ?? null, openAfter: false });
}

/// Reads the open items through the machines and the saved tokens; the
/// desktop keeps them a few minutes, so this runs less often than Home's
/// refresh. Keep the current list until every source has answered: global
/// progress events can belong to another read. A failure shows in the tab.
export const refreshGit = single(async () => {
  try {
    takeGit({ value: await invoke<GitItemsView | null>("git_items") });
  } catch (e) {
    console.warn("open PRs and issues:", e);
    takeGit({ error: String(e) });
  }
});

/// Takes what a read or the forge push gave for the open items.
export function takeGit(items: Pushed<GitItemsView | null>) {
  if ("error" in items) {
    state.gitError = items.error;
  } else {
    state.git = items.value;
    state.gitError = null;
  }

  if (currentLocation()?.view === "home") homeDataChanged.notify();
}
