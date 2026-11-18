// Home's Open PRs and issues tab, and importing a repo from it.
import { reason } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { keepLastImport, lastImport, openImportProject } from "../addproject/importproject.tsx";
import { foundFor, type ProjectRow } from "../project/projects-state.ts";
import { importFound } from "../addproject/addproject.tsx";
import { projectStart, projectsWithOpen, replyStart, type GitItemRow, type GitItemsView, type Hosts, type StartProject } from "./openitems-state.ts";
import { agentButtons, earlierReview, startContinue, startReview, workOnIssue } from "../project/composer.tsx";
import type { MenuItem } from "../ui/menu.tsx";
import { AGENT_ACTIVE } from "../agents/sessionmenu-state.ts";
import { agentLabel } from "../project/projects-state.ts";
import type { AgentButtonItem } from "../review/feedback-view.tsx";
import { state } from "./state.ts";
import { draw } from "./home.tsx";
import { currentLocation, go } from "./views.tsx";
import { loadFound } from "./sidebar.tsx";
import { refresh } from "./refresh.ts";
import { single } from "../ui/active.ts";

export function openUrl(url: string) {
  invoke("open_url", { url }).catch(e => console.warn("opening a link:", e));
}

const homeButtons = agentButtons(() => draw());

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

/// Continue on one of the user's PRs of `project`: goes on in the chat on
/// its branch, or starts an agent there, as the project page's does.
export function continueButton(item: GitItemRow, project: StartProject, machines: Hosts): AgentButtonItem {
  const machine = projectStart(project, machines).machine;
  return homeButtons(`continue ${item.url}`, {
    project: project.name, machine, choose: true, label: "Continue", hint: "Pick a model: goes on in the chat on this pull request's branch, or starts an agent there",
    session: null, start: (pick, at) => startContinue(project.name, item, pick, at),
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
  draw();

  try {
    // A checkout the machines found imports as it is, not as a second clone.
    if (!state.found) await loadFound();
    const here = foundFor(state.found, p.repo);
    if (here) await importFound(here);
    else keepLastImport(await invoke<ProjectRow>("import_project", { repo: importRepo(p), machineId: machine.id, folder: null, worktrees: [] }));
    rowImports.delete(p.key);
    await refresh();
  } catch (e) {
    const error = reason(e);
    rowImports.set(p.key, { error });
    console.warn(`importing ${p.repo}:`, error);
  }

  draw();
}

/// The Import a project sheet for the row's repository, on the last
/// machine used.
function importWithSettings(p: OpenRepo) {
  const machine = importMachine();
  openImportProject({
    machines: state.home?.machines || [],
    macId: state.mac?.id || null,
    repo: importRepo(p),
    machine: machine?.id ?? null,
    onDone: () => refresh(),
  });
}

/// Reads the open items through the machines and the saved tokens; the
/// desktop keeps them a few minutes, so this runs less often than Home's
/// refresh. Keep the current list until every source has answered: global
/// progress events can belong to another read. A failure shows in the tab.
export const refreshGit = single(async () => {
  try {
    state.git = await invoke<GitItemsView | null>("git_items");
    state.gitError = null;
  } catch (e) {
    console.warn("open PRs and issues:", e);
    state.gitError = String(e);
  }

  if (currentLocation()?.view === "home") draw();
});
