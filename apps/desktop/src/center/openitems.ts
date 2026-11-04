// Home's Open PRs and issues tab, and importing a repo from it.
import { ago, plural } from "../ui/format.ts";
import { dbtn as button, note, svgEl as icon } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { pane } from "../bar/pane.ts";
import { keepLastImport, lastImport, openImportProject } from "../addproject/importproject.ts";
import { folderUnder, reason } from "../addproject/importproject-state.ts";
import { el as h, type Kid } from "../ui/dom.ts";
import { type ProjectRow } from "../project/projects-state.ts";
import { openMenu } from "../ui/menu.ts";
import { ITEMS_SHOWN, itemChips, projectsWithOpen, replyPrompt, reviewStart, type GitItemRow, type GitItemsView } from "./openitems-state.ts";
import { IC, state } from "./state.ts";
import { projectDot, section, tag, rowButton, line } from "./pieces.ts";
import { draw } from "./home.ts";
import { shown, go } from "./views.ts";
import { reloadChats, projectRows, openProjects } from "./sidebar.ts";
import { type LabelFilter, labelFilter, labelChip } from "../project/split.ts";
import { refresh } from "./refresh.ts";

export function openUrl(url: string) {
  invoke("open_url", { url }).catch(e => console.warn("opening a link:", e));
}

/// What Review needs of a project: where it is checked out.
type ReviewProject = Parameters<typeof reviewStart>[1];

/// Review prefills Start a coding agent: in the project's folder on an
/// online machine when it is imported, else on Auto.
function reviewItem(item: GitItemRow, project: ReviewProject) {
  const start = reviewStart(item, project, state.home?.machines);
  go("pane", { agentnew: start });
}

/// Reply opens a new chat with Beep that asks for help with the item; Beep
/// reads it with its GitHub tools, and posting asks first.
async function replyItem(item: GitItemRow) {
  go("pane", { chat: { session: null } });
  try {
    await pane.compose(replyPrompt(item));
  } catch (e) {
    console.warn("starting a reply:", e);
  }
  reloadChats();
}

export function openItemRow(item: GitItemRow, project: ReviewProject, f?: LabelFilter) {
  const kindIcon = icon(item.kind === "pull_request" ? IC.pr : IC.issue);
  const tags = (item.labels || []).map(l => f ? labelChip(l, f, "i") : h("i", "pj-tag", l));
  const title = h("button", "t", h("span", "num", `#${item.number}`), h("b", "", item.title), ...tags);
  title.type = "button";
  title.title = item.url;
  const right = [
    ...itemChips(item).map(c => tag(c.cls, c.text)),
    h("span", "age", ago(item.updated_at)),
    item.kind === "pull_request" ? rowButton("Review", "small", () => reviewItem(item, project)) : null,
    rowButton("Reply", "small", () => replyItem(item)),
  ];
  const row = h("div", "cc-row cc-line cc-gr it", h("span", "ic", kindIcon), title, h("span", "r", ...right));
  row.addEventListener("click", () => openUrl(item.url));
  return row;
}

/// A repository's head row among open items.
export function repoLine(i: number | undefined, repo: string, sub: string, right: Kid[], open: () => void) {
  const head = line(projectDot(i), repo, sub, right, open);
  head.classList.add("cc-gr");
  return head;
}

type OpenRepo = ReturnType<typeof projectsWithOpen>[number];

/// One project's row, its counts and its newest items.
function openProjectRows(p: OpenRepo, i: number, f?: LabelFilter) {
  const counts = [p.prs ? plural(p.prs, "PR", "PRs") : null, p.issues ? plural(p.issues, "issue", "issues") : null]
    .filter(Boolean).map(t => h("span", "", t));
  const right = [...counts, p.waiting ? tag("wait", `${p.waiting} waiting on you`) : null, h("span", "age", ago(p.last))];
  const sub = p.project ? p.host : `${p.host} · not imported`;
  if (!p.project) right.push(rowImportButton(p));
  const head = repoLine(i, p.repo, sub, right, () => (p.project ? openProjects(p.project.name) : openUrl(p.url)));
  if (!p.project) {
    head.addEventListener("contextmenu", e => openMenu(e, p.repo, [
      { label: "Import…", on: () => quickImport(p) },
      { label: "Import with settings…", on: () => importWithSettings(p) },
    ]));
  }
  const all = state.gitAll.has(p.key);
  const rows = [head, ...(all ? p.items : p.items.slice(0, ITEMS_SHOWN)).map(item => openItemRow(item, p.project, f))];
  if (p.items.length > ITEMS_SHOWN) {
    const more = button(all ? "Show fewer" : `Show all ${p.items.length}`, "quiet small", () => {
      if (all) state.gitAll.delete(p.key); else state.gitAll.add(p.key);
      draw();
    });
    rows.push(h("div", "cc-gr it more", more));
  }
  return rows;
}

/// Repositories being imported from their row, by key, and why one failed.
const rowImports = new Map<string, "busy" | { error: string }>();

/// What `import_project` takes for an open-items repository: `org/repo`
/// on GitHub, its URL elsewhere.
function importRepo(p: OpenRepo) {
  return p.host === "github.com" ? p.repo : p.url;
}

/// The row's Import button, or how its import stands.
function rowImportButton(p: OpenRepo): HTMLElement {
  const now = rowImports.get(p.key);
  if (now === "busy") return h("span", "age", h("span", "dspin"), " Importing…");
  const b = rowButton("Import", "small", () => quickImport(p));
  b.title = importTitle(p);
  if (now) {
    const failed = tag("bad", "Import failed");
    failed.title = now.error;
    return h("span", "", failed, " ", b);
  }
  return b;
}

/// Where Import puts it: the last machine and folder used, else the server's
/// default on the first online machine.
function importPlace(p: OpenRepo) {
  const machines = (state.home?.machines || []).filter(m => m.online);
  const last = lastImport();
  const machine = machines.find(m => m.id === last?.machine) || machines[0] || null;
  const folder = machine && last?.root && machine.id === last.machine ? folderUnder(last.root, importRepo(p)) : null;
  return { machine, folder };
}

function importTitle(p: OpenRepo) {
  const { machine, folder } = importPlace(p);
  if (!machine) return "No machine is online; Import opens the sheet";
  return `Imports on ${machine.name}${folder ? `, into ${folder}` : ""}. Right-click for Import with settings…`;
}

/// Imports from the row with the last machine and folder; the row turns
/// into the project once it is in. With no machine online, the sheet.
async function quickImport(p: OpenRepo) {
  if (rowImports.get(p.key) === "busy") return;
  const { machine, folder } = importPlace(p);
  if (!machine) return importWithSettings(p);
  rowImports.set(p.key, "busy");
  draw();
  try {
    const project = await invoke<ProjectRow>("import_project", { repo: importRepo(p), machineId: machine.id, folder, worktrees: [] });
    keepLastImport(project);
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
    machines: state.home?.machines || [],
    macId: state.mac?.id || null,
    repo: importRepo(p),
    machine: machine?.id ?? null,
    folder,
    onDone: () => refresh(),
  });
}

/// Open PRs beside open issues on the user's git hosts, each a row per
/// project with its items under it. A host that failed says why above.
export function gitSection(view: GitItemsView) {
  const all = projectRows() || [];
  const column = (kind: GitItemRow["kind"], title: string, none: string) => {
    const { items, labels: f } = labelFilter(`home:${kind}`, view.items.filter(i => i.kind === kind), i => i.labels, draw);
    const chips = f.all.length ? h("div", "pj-labels", ...f.all.map(l => labelChip(l, f, "button"))) : null;
    const body = items.length ? projectsWithOpen({ items }, all).flatMap((p, n) => openProjectRows(p, n, f)) : [h("div", "cc-empty", h("span", "", none))];
    return section("", title, items.length, null, h("div", "cc-panel cc-open", chips, ...body));
  };
  const el = h("div", "cc-git", ...view.hosts.filter(x => x.error).map(x => note("bad", x.error)),
    h("div", "cc-cols", column("pull_request", "Pull requests", "No open pull requests."), column("issue", "Issues", "No open issues.")));
  el.id = "sec-git";
  return el;
}

/// An open PR or issue in an Overview box: number and title, its repo and
/// first state chip, opening it on its host.
export function itemLine(item: GitItemRow) {
  const chip = itemChips(item)[0];
  const row = line(icon(item.kind === "pull_request" ? IC.pr : IC.issue), `#${item.number} ${item.title}`, item.repo,
    [chip ? tag(chip.cls, chip.text) : null, ago(item.updated_at)], () => openUrl(item.url));
  row.title = item.url;
  return row;
}

/// `load` run so that a call while one runs is dropped.
export function single(load: () => Promise<void>) {
  let running = false;
  return async () => {
    if (running) return;
    running = true;
    try {
      await load();
    } finally {
      running = false;
    }
  };
}

/// Asks the server for the open items; it keeps them a few minutes, so
/// this runs less often than Home's refresh.
export const refreshGit = single(async () => {
  try {
    state.git = await invoke<GitItemsView | null>("git_items");
  } catch (e) {
    console.warn("open PRs and issues:", e);
  }
  if (shown === "home") draw();
});
