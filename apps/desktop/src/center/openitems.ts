// Home's Open PRs and issues tab, and importing a repo from it.
import { reason } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { keepLastImport, lastImport, openImportProject } from "../addproject/importproject.tsx";
import { foundFor, type ProjectRow } from "../project/projects-state.ts";
import { importFound } from "../addproject/addproject.tsx";
import { projectsWithOpen, type GitItemRow, type GitItemsView } from "./openitems-state.ts";
import { state } from "./state.ts";
import { draw } from "./home.tsx";
import { currentLocation, go } from "./views.tsx";
import { loadFound } from "./sidebar.tsx";
import { refresh } from "./refresh.ts";
import { single } from "../ui/active.ts";

export function openUrl(url: string) {
  invoke("open_url", { url }).catch(e => console.warn("opening a link:", e));
}

/// Reply opens the agent's start form with a prompt to help reply to the item.
export function replyItem(item: GitItemRow) {
  go("pane", { agentnew: { prompt: `Help me reply to ${item.url}` } });
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
