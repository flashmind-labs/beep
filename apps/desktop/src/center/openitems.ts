// Home's Open PRs and issues tab, and importing a repo from it.
import { ago, reason } from "../ui/format.ts";
import { svgEl as icon } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { pane } from "../bar/pane.ts";
import { keepLastImport, lastImport, openImportProject } from "../addproject/importproject.ts";
import { folderUnder } from "../addproject/importproject-state.ts";
import { el as h } from "../ui/dom.ts";
import { foundFor, type ProjectRow } from "../project/projects-state.ts";
import { importFound } from "../addproject/addproject.ts";
import { itemChips, projectsWithOpen, type GitItemRow, type GitItemsView } from "./openitems-state.ts";
import { state } from "./state.ts";
import { line } from "../ui/pieces.ts";
import { draw } from "./home.ts";
import { currentLocation, go } from "./views.ts";
import { reloadChats, loadFound } from "./sidebar.ts";
import { refresh } from "./refresh.ts";
import { IC } from "../ui/icons.ts";

export function openUrl(url: string) {
  invoke("open_url", { url }).catch(e => console.warn("opening a link:", e));
}

/// Reply opens a new chat with Beep that asks for help with the item; Beep
/// reads it with its GitHub tools, and posting asks first.
export async function replyItem(item: GitItemRow) {
  go("pane", { chat: { session: null } });

  try {
    await pane.compose(`Help me reply to ${item.url}`);
  } catch (e) {
    console.warn("starting a reply:", e);
  }

  reloadChats();
}

export type OpenRepo = ReturnType<typeof projectsWithOpen>[number];

/// Repositories being imported from their row, by key, and why one failed.
export const rowImports = new Map<string, "busy" | { error: string }>();

/// What `import_project` takes for an open-items repository: `org/repo`
/// on GitHub, its URL elsewhere.
function importRepo(p: OpenRepo) {
  return p.host === "github.com" ? p.repo : p.url;
}

/// Where Import puts it: the last machine and folder used, else the server's
/// default on the first online machine.
function importPlace(p: OpenRepo) {
  const machines = (state.home?.machines || []).filter(m => m.online && m.can_host !== false);
  const last = lastImport();
  const machine = machines.find(m => m.id === last?.machine) || machines[0] || null;
  const folder = machine && last?.root && machine.id === last.machine ? folderUnder(last.root, importRepo(p)) : null;
  return { machine, folder };
}

export function importTitle(p: OpenRepo) {
  const { machine, folder } = importPlace(p);
  if (!machine) return "No machine is online; Import opens the sheet";
  return `Imports on ${machine.name}${folder ? `, into ${folder}` : ""}. Right-click for Import with settings…`;
}

/// Imports from the row with the last machine and folder; the row turns
/// into the project once it is in. With no machine online, the sheet.
export async function quickImport(p: OpenRepo) {
  if (rowImports.get(p.key) === "busy") return;
  const { machine, folder } = importPlace(p);
  if (!machine) return importWithSettings(p);
  rowImports.set(p.key, "busy");
  draw();

  try {
    // A checkout the machines found imports as it is, not as a second clone.
    if (!state.found) await loadFound();
    const here = foundFor(state.found, p.repo);
    if (here) await importFound(here);
    else keepLastImport(await invoke<ProjectRow>("import_project", { repo: importRepo(p), machineId: machine.id, folder, worktrees: [] }));
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
/// machine and folder used.
function importWithSettings(p: OpenRepo) {
  const { machine, folder } = importPlace(p);
  openImportProject({
    machines: (state.home?.machines || []).filter(m => m.can_host !== false),
    macId: state.mac?.id || null,
    repo: importRepo(p),
    machine: machine?.id ?? null,
    folder,
    onDone: () => refresh(),
  });
}

/// An open PR or issue in an Overview box: number and title, with its first
/// state chip and repo below, opening it on its host.
export function itemLine(item: GitItemRow) {
  const chip = itemChips(item)[0];
  const sub = chip ? h("span", "cc-sub", h("i", `chip ${chip.cls}`, chip.text), item.repo) : item.repo;
  const row = line(icon(item.kind === "pull_request" ? IC.pr : IC.issue), `#${item.number} ${item.title}`, sub,
    [ago(item.updated_at)], () => openUrl(item.url));
  row.title = item.url;
  return row;
}

/// Calls during a read share its promise and request one fresh read after it.
export function single(load: () => Promise<void>) {
  let running: Promise<void> | undefined;
  let dirty = false;
  return async () => {
    if (running) { dirty = true; return running; }
    running = (async () => {
      do { dirty = false; await load(); } while (dirty);
    })();

    try { await running; }
    finally { running = undefined; }
  };
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
