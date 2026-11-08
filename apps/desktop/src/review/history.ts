// The project's Git navigator and its selected commit or worktree diff.
import { button, el } from "../ui/dom.ts";
import { dbtn, errorNote, svgEl, writeClipboard } from "../ui/parts.ts";
import { ago, plural, reason } from "../ui/format.ts";
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { SessionRow } from "../project/projects-state.ts";
import { agentLabel } from "../project/projects-state.ts";
import { branchSessions } from "./branchsession-state.ts";
import { diffView, type DiffViewHandle } from "./diff.ts";
import { type DiffView, STATUS_LETTER } from "./diff-state.ts";
import { appendCommits, dayGroups, diffPaths, selectionKey, type BranchSessionRow, type ChangedFile, type GitCommit, type GitSelection, type GitWorktree, type HistoryPage } from "./history-state.ts";

interface HistoryDeps {
  invoke: typeof Invoke;
  back: () => void;
  sessions: (project: string) => readonly BranchSessionRow[] | undefined;
  openSession: (session: SessionRow) => void;
  reviewBranch: (project: string, branch: string) => void;
  canReview: (project: string, branch: string) => boolean;
}
interface CommitList {
  revision: string | null;
  search: string;
  rows: GitCommit[];
  next: number | null;
  loading: boolean;
  loaded: boolean;
  error: string;
  version: number;
}
interface FileRead { rows: ChangedFile[]; loading: boolean; loaded: boolean; error: string }
interface ProjectHistory {
  name: string;
  data?: HistoryPage;
  lists: Map<string, CommitList>;
  files: Map<string, FileRead>;
  commits: Map<string, GitCommit>;
  open: Set<string>;
  selected: GitSelection | null;
  path: string | null;
  tab: "changes" | "details" | "sessions";
  query: string;
  nav: HTMLElement;
  tree: HTMLElement;
  detail: HTMLElement;
  foot: HTMLElement;
  search: HTMLInputElement;
  view: DiffViewHandle | null;
  viewKey: string;
  timer?: ReturnType<typeof setTimeout>;
}
const CHEVRON = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="m6 4 4 4-4 4"/></svg>';

export function makeGitHistory(deps: HistoryDeps) {
  const projects = new Map<string, ProjectHistory>();
  let active = 0;
  const waiting: (() => void)[] = [];
  async function read<T>(command: string, args: Record<string, unknown>): Promise<T> {
    await new Promise<void>(resolve => {
      if (active < 4) { ++active; resolve(); } else waiting.push(resolve);
    });
    try { return await deps.invoke<T>(command, args); }
    finally { const next = waiting.shift(); if (next) next(); else --active; }
  }

  function list(p: ProjectHistory, revision: string | null = null, search = "") {
    const key = `${revision || "all"}\0${search}`;
    let found = p.lists.get(key);
    if (!found) {
      found = { revision, search, rows: [], next: null, loading: false, loaded: false, error: "", version: 0 };
      p.lists.set(key, found);
    }
    return found;
  }

  function project(name: string) {
    let p = projects.get(name);
    if (p) return p;
    const tree = el("div", "gh-tree");
    const foot = el("div", "gh-nav-foot");
    const search = el("input");
    search.type = "search";
    search.maxLength = 256;
    search.placeholder = "Search commit messages";
    search.setAttribute("aria-label", "Search commit messages");
    const nav = el("aside", "pj-list gh-nav",
      el("div", "gh-nav-head", button("Project", "gh-back", deps.back), el("b", "", "Git"),
        button("Refresh", "gh-collapse", () => refresh(name)), button("Collapse all", "gh-collapse", () => {
          p!.open.clear();
          for (const node of p!.tree.querySelectorAll<HTMLDetailsElement>("details")) node.open = false;
        })), el("label", "gh-search", search), tree, foot);
    nav.setAttribute("aria-label", "Git navigation");
    const detail = el("div", "pj-det gh-detail");
    p = { name, lists: new Map(), files: new Map(), commits: new Map(), open: new Set(["history"]), selected: null,
      path: null, tab: "changes", query: "", nav, tree, detail, foot, search, view: null, viewKey: "" };
    projects.set(name, p);
    const current = p;
    search.addEventListener("input", () => {
      current.query = search.value;
      clearTimeout(current.timer);
      current.timer = setTimeout(() => {
        const found = list(current, null, current.query);
        current.open.add("history");
        renderTree(current);
        if (!found.loaded && !found.loading) void load(current, found);
      }, 250);
    });
    return p;
  }

  async function load(p: ProjectHistory, target: CommitList, more = false) {
    if (target.loading) return;
    const version = ++target.version;
    target.loading = true;
    target.error = "";
    renderTree(p);
    try {
      const data = await read<HistoryPage>("project_git_history", {
        project: p.name, machineId: p.data?.machine_id ?? null, revision: target.revision,
        offset: more ? target.next ?? 0 : 0, search: target.search,
      });
      if (version !== target.version) return;
      if (!p.data || (!target.revision && !target.search && !more)) p.data = data;
      target.rows = more ? appendCommits(target.rows, data.commits) : data.commits;
      target.next = data.next;
      target.loaded = true;
      for (const c of data.commits) p.commits.set(c.id, c);
      if (!p.selected && data.commits.length) {
        p.open.add(`commit:${data.commits[0].id}`);
        p.open.add(`changes:${data.commits[0].id}`);
        select(p, { kind: "commit", commit: data.commits[0], origin: "All history" });
      }
      if (target.search) for (const [day] of dayGroups(target.rows)) p.open.add(`day:${target.search}:${day}`);
    } catch (e) {
      if (version === target.version) target.error = reason(e);
    } finally {
      if (version === target.version) { target.loading = false; renderTree(p); renderDetail(p); }
    }
  }

  function node(p: ProjectHistory, key: string, label: (Node | string)[], children: () => (Node | null)[], on?: () => void) {
    const summary = el("summary", "", svgEl(CHEVRON), ...label);
    const details = el("details", "gh-node", summary);
    details.open = p.open.has(key);
    details.dataset.key = key;
    const body = el("div", "gh-children");
    const draw = () => body.replaceChildren(...children().filter((n): n is Node => !!n));
    if (details.open) draw();
    details.append(body);
    details.addEventListener("toggle", () => {
      // Detached nodes can still deliver a queued toggle after a redraw.
      if (!details.isConnected) return;
      if (details.open) { p.open.add(key); draw(); } else p.open.delete(key);
    });
    summary.addEventListener("click", () => { on?.(); });
    return details;
  }

  function fileRead(p: ProjectHistory, selected: GitSelection): FileRead {
    const key = selectionKey(selected);
    let read = p.files.get(key);
    if (!read) { read = { rows: [], loading: false, loaded: false, error: "" }; p.files.set(key, read); }
    return read;
  }

  async function readFiles(p: ProjectHistory, selected: GitSelection) {
    const result = fileRead(p, selected);
    if (result.loaded || result.loading || !p.data) return;
    result.loading = true;
    try {
      if (selected.kind === "commit") {
        result.rows = await read<ChangedFile[]>("project_git_files", { project: p.name, machineId: p.data.machine_id, commit: selected.commit.id });
      } else {
        const diff = await read<DiffView>("work_diff", { machineId: p.data.machine_id, root: selected.worktree.path, only: [], lockfiles: false, uncommitted: true });
        result.rows = diff.files;
        if (diff.more) result.error = `${diff.more} more files are beyond the diff limit.`;
      }
      result.loaded = true;
    } catch (e) { result.error = reason(e); }
    finally { result.loading = false; renderTree(p); if (p.selected && selectionKey(p.selected) === selectionKey(selected)) renderDetail(p); }
  }

  function changedFiles(p: ProjectHistory, selected: GitSelection) {
    const read = fileRead(p, selected);
    if (!read.loaded && !read.loading && !read.error) void readFiles(p, selected);
    const rows: HTMLElement[] = read.rows.map(f => {
      const b = button(el("span", "gh-status", STATUS_LETTER[f.status]), "gh-file", () => {
        p.selected = selected; p.path = f.path; p.tab = "changes"; renderTree(p); renderDetail(p);
      });
      b.append(el("span", "", f.path));
      b.dataset.path = f.path;
      b.dataset.selection = selectionKey(selected);
      b.title = f.old_path ? `${f.old_path} -> ${f.path}` : f.path;
      b.setAttribute("aria-current", String(!!p.selected && selectionKey(p.selected) === selectionKey(selected) && p.path === f.path));
      return b;
    });
    if (read.loading) rows.push(el("p", "gh-empty", "Reading changed files…"));
    if (read.error) rows.push(el("div", "", errorNote(read.error), button("Try again", "gh-more", () => { read.error = ""; read.loaded = false; void readFiles(p, selected); })));
    if (read.loaded && !rows.length) rows.push(el("p", "gh-empty", selected.kind === "working" ? "Working tree clean." : "This commit changes no files."));
    return rows;
  }

  function commitNode(p: ProjectHistory, c: GitCommit, origin: string, prefix = "") {
    const selected: GitSelection = { kind: "commit", commit: c, origin };
    const key = `${prefix}commit:${c.id}`;
    const row = node(p, key, [el("span", `gh-dot${c.parents.length > 1 ? " merge" : ""}`), el("span", "gh-subject", c.subject || "Untitled commit"), el("code", "", c.id.slice(0, 7))], () => [
      node(p, `${prefix}changes:${c.id}`, [el("span", "", "Changes")], () => changedFiles(p, selected), () => select(p, selected)),
    ], () => { select(p, selected); void readFiles(p, selected); });
    row.classList.add("gh-commit");
    const summary = row.querySelector("summary")!;
    summary.title = `${c.subject}\n${c.id}`;
    summary.setAttribute("aria-current", String(p.selected?.kind === "commit" && p.selected.commit.id === c.id));
    return row;
  }

  function listStatus(p: ProjectHistory, found: CommitList) {
    const children: HTMLElement[] = [];
    if (found.loading) children.push(el("p", "gh-empty", "Reading history…"));
    if (found.error) children.push(el("div", "", errorNote(found.error), button("Try again", "gh-more", () => void load(p, found, found.rows.length > 0))));
    if (found.loaded && !found.rows.length) children.push(el("p", "gh-empty", found.search ? "No commits match this search." : "No commits yet."));
    if (!found.loading && found.next !== null) children.push(button("Load older commits", "gh-more", () => void load(p, found, true)));
    return children;
  }

  function listRows(p: ProjectHistory, found: CommitList, origin: string, prefix = "") {
    return [...found.rows.map(c => commitNode(p, c, origin, prefix)), ...listStatus(p, found)];
  }

  function renderTree(p: ProjectHistory) {
    const found = list(p, null, p.query);
    const history = node(p, "history", [el("b", "", "All history"), el("small", "", found.rows.length ? `${found.rows.length}${found.next !== null ? "+" : ""}` : "")], () => {
      const days = dayGroups(found.rows);
      if (!days.length) return listRows(p, found, "All history");
      // Open the newest day on first read; other dates stay folded.
      if (!p.tree.dataset.started) { p.open.add(`day:${p.query}:${days[0][0]}`); p.tree.dataset.started = "true"; }
      const rows = days.map(([day, commits]) => node(p, `day:${p.query}:${day}`, [el("span", "", day), el("small", "", String(commits.length))], () => commits.map(c => commitNode(p, c, "All history"))));
      return [...rows, ...listStatus(p, found)];
    });
    const refs = (p.data?.refs || []).filter(r => r.kind !== "tag");
    const branches = node(p, "branches", [el("b", "", "Branches"), el("small", "", String(refs.length))], () => refs.map(ref => {
      const branchList = list(p, ref.head);
      return node(p, `branch:${ref.kind}:${ref.name}`, [el("code", "", ref.name), el("small", "", ref.kind === "remote" ? "remote" : "")], () => {
        if (!branchList.loaded && !branchList.loading && !branchList.error) void load(p, branchList);
        return listRows(p, branchList, `Branches / ${ref.name}`, `${ref.name}:`);
      });
    }));
    const paths = p.data?.worktrees.map(w => w.path) || [];
    const worktrees = node(p, "worktrees", [el("b", "", "Worktrees"), el("small", "", String(paths.length))], () => (p.data?.worktrees || []).map(w => {
      const selected: GitSelection = { kind: "working", worktree: w };
      return node(p, `worktree:${w.path}`, [el("code", "", w.branch || w.head?.slice(0, 7) || "Unborn checkout"), el("small", "", w.main ? "main checkout" : "")], () => [
        el("div", "gh-location", `${p.data?.machine_name}\n`, el("code", "", w.path)),
        node(p, `working:${w.path}`, [el("span", "", "Uncommitted changes")], () => changedFiles(p, selected), () => { select(p, selected); void readFiles(p, selected); }),
        node(p, `sessions:${w.path}`, [el("span", "", "Sessions")], () => sessionRows(p, w)),
      ]);
    }));
    const scroll = p.tree.scrollTop;
    const focused = document.activeElement as HTMLElement | null;
    const focusedNode = focused?.closest<HTMLDetailsElement>("details[data-key]");
    const focusKey = p.tree.contains(focused) ? focusedNode?.dataset.key : null;
    const focusedPath = focused?.dataset.path;
    const focusedSelection = focused?.dataset.selection;
    p.tree.replaceChildren(history, branches, worktrees);
    if (focusedPath && focusedSelection) {
      [...p.tree.querySelectorAll<HTMLElement>(".gh-file")].find(b => b.dataset.path === focusedPath && b.dataset.selection === focusedSelection)?.focus({ preventScroll: true });
    } else if (focusKey) {
      [...p.tree.querySelectorAll<HTMLDetailsElement>("details[data-key]")].find(d => d.dataset.key === focusKey)?.querySelector<HTMLElement>("summary")?.focus({ preventScroll: true });
    }
    p.tree.scrollTop = scroll;
    p.foot.textContent = p.data ? `All branches · ${p.data.machine_name}` : "";
  }

  function sessionRows(p: ProjectHistory, w: GitWorktree) {
    const all = deps.sessions(p.name);
    const related = branchSessions(all || [], p.data?.machine_id, w.path, p.data?.worktrees.map(w => w.path));
    return related.length ? related.map(s => {
      const b = button(el("b", "", s.title || "Untitled session"), "gh-session-link", () => deps.openSession(s));
      b.append(el("span", "", `${agentLabel(s.agent)} · ${s.status} · ${ago(s.last_activity)}`));
      return b;
    }) : [el("p", "gh-empty", all === undefined ? "Reading sessions…" : "No sessions in this worktree.")];
  }

  function select(p: ProjectHistory, selected: GitSelection) {
    p.selected = selected; p.path = null; p.tab = "changes";
    renderDetail(p);
    for (const summary of p.tree.querySelectorAll("summary[aria-current]")) summary.setAttribute("aria-current", String(summary.parentElement?.dataset.key?.endsWith(`commit:${selected.kind === "commit" ? selected.commit.id : ""}`)));
  }

  async function parent(p: ProjectHistory, id: string) {
    let c = p.commits.get(id);
    if (!c) { const found = list(p, id); await load(p, found); c = p.commits.get(id); }
    if (c) { p.open.add("history"); select(p, { kind: "commit", commit: c, origin: "Parent commit" }); }
  }

  function renderDetail(p: ProjectHistory) {
    const selected = p.selected;
    if (!selected || !p.data) {
      const found = list(p, null, p.query);
      p.detail.replaceChildren(el("p", "gh-empty", found.loading ? "Reading Git history…" : "Select a commit or worktree to see its changes."));
      return;
    }
    const c = selected.kind === "commit" ? selected.commit : null;
    const w = selected.kind === "working" ? selected.worktree : null;
    const head = el("header", "gh-detail-head", el("div", "gh-breadcrumb", c ? `${selected.kind === "commit" ? selected.origin : ""} / ${c.id.slice(0, 7)}` : `Worktrees / ${w!.branch || "Detached HEAD"}`));
    const titleRow = el("div", "gh-title-row", el("h3", "", c ? c.subject || "Untitled commit" : `Uncommitted changes in ${w!.branch || "checkout"}`));
    const ref = c && p.data.refs.find(r => r.kind === "local" && r.head === c.id);
    if (ref && deps.canReview(p.name, ref.name)) titleRow.append(dbtn("Review branch", "quiet small", () => deps.reviewBranch(p.name, ref.name)));
    const copy = dbtn(c ? "Copy SHA" : "Copy path", "quiet small", async () => { await writeClipboard(c?.id || w!.path); copy.textContent = "Copied"; });
    titleRow.append(copy);
    head.append(titleRow, el("p", "", c ? `${c.author} · ${new Date(c.committed_at).toLocaleString()}` : `${w!.path} · ${p.data.machine_name}`));
    const tabs = el("nav", "gh-tabs"); tabs.setAttribute("aria-label", "Git views");
    for (const [key, label] of [["changes", "Changes"], ["details", "Details"], ["sessions", "Sessions"]] as const) {
      const b = button(label, "", () => { p.tab = key; renderDetail(p); });
      b.setAttribute("aria-current", String(p.tab === key)); tabs.append(b);
    }
    const body = el("div", "gh-detail-body");
    if (p.tab === "details") {
      const facts = el("div", "gh-facts", el("span", "", c ? "Commit" : "Worktree"), el("code", "", c?.id || w!.path));
      if (c) {
        facts.append(el("span", "", "Author"), el("span", "", c.author), el("span", "", "Parents"));
        facts.append(c.parents.length ? el("span", "", ...c.parents.map(id => button(id.slice(0, 7), "gh-parent", () => void parent(p, id)))) : el("span", "", "Initial commit"));
      }
      body.append(facts);
    } else if (p.tab === "sessions") {
      const currentTree = w || (ref && p.data.worktrees.find(w => w.branch === ref.name));
      body.append(el("div", "gh-session-view", ...(currentTree ? [el("p", "gh-empty", "Sessions in this worktree"), ...sessionRows(p, currentTree)] : [el("p", "gh-empty", "No recorded session link for this commit. Worktree sessions are listed under Worktrees.")])));
    } else {
      const path = p.path;
      const key = `${selectionKey(selected)}\0${path || ""}`;
      if (p.viewKey !== key || !p.view) {
        p.viewKey = key;
        const location = p.data;
        p.view = diffView({
          readOnly: true,
          load: async (only, lockfiles) => {
            const requested = only.length ? only : path ? [path] : [];
            const paths = diffPaths(fileRead(p, selected).rows, requested);
            const view = c ? await read<DiffView>("project_git_diff", { project: p.name, machineId: location.machine_id, commit: c.id, only: paths, lockfiles: lockfiles || !!path })
              : await read<DiffView>("work_diff", { machineId: location.machine_id, root: w!.path, only: paths, lockfiles: lockfiles || !!path, uncommitted: true });
            return path ? { ...view, files: view.files.filter(f => f.path === path), more: 0 } : view;
          },
          review: { key: `history:${p.name}:${key}`, heading: () => c?.subject || "Worktree changes", send: async () => "Open a session to send a review." },
        });
      }
      if (path) body.append(el("div", "gh-diff-toolbar", el("code", "", path), button("Show all files", "", () => { p.path = null; renderDetail(p); })));
      body.append(p.view.el);
    }
    p.detail.replaceChildren(head, tabs, body);
  }

  function panels(name: string) {
    const p = project(name);
    const found = list(p);
    if (!found.loaded && !found.loading && !found.error) void load(p, found);
    renderTree(p); renderDetail(p);
    return { navigation: p.nav, detail: p.detail };
  }

  function refresh(name: string) {
    const p = project(name);
    p.files.clear(); p.view = null; p.viewKey = "";
    for (const value of p.lists.values()) { ++value.version; value.loading = false; value.loaded = false; value.error = ""; }
    void load(p, list(p, null, p.query));
  }
  return { panels, refresh };
}
