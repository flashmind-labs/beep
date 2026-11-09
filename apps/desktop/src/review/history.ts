import { createElement, type ReactNode } from "react";
import { showWindowParts } from "../center/window-root.ts";
import { HistoryFacts } from "./history-facts.tsx";
import { HistoryHeader } from "./history-header.tsx";
import { HistoryNavigation } from "./history-navigation.tsx";
import { HistoryPath, HistorySessions } from "./history-sessions.tsx";
import { TreeNode } from "./history-tree.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
// The project's Git navigator and its selected commit or worktree diff.
import { el } from "../ui/dom.ts";
import { ago, reason } from "../ui/format.ts";
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { SessionRow } from "../project/projects-state.ts";
import { agentLabel } from "../project/projects-state.ts";
import { branchSessions } from "./branchsession-state.ts";
import { windowDiffOwner } from "./diff-owner.ts";
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
  chrome: HTMLElement;
  tree: HTMLElement;
  detail: HTMLElement;
  header: { element: HTMLElement };
  sessionsView: { element: HTMLElement } | null;
  pathView: { element: HTMLElement } | null;
  foot: HTMLElement;
  facts: { element: HTMLElement } | null;
  view: DiffViewHandle | null;
  viewKey: string;
  timer?: ReturnType<typeof setTimeout>;
}

export function makeGitHistory(deps: HistoryDeps) {
  const projects = new Map<string, ProjectHistory>();
  let following: ProjectHistory | null = null;
  let watched = "";
  let unfollow: (() => void) | undefined;

  function follow(p: ProjectHistory) {
    if (following !== p) stop();
    following = p;
    if (!p.data) return;
    const roots = [...new Set([p.data.root, ...(p.selected?.kind === "working" ? [p.selected.worktree.path] : [])])].sort();
    const key = JSON.stringify([p.name, p.data.machine_id, roots]);
    if (watched === key) return;
    unfollow?.();
    watched = key;
    unfollow = subscribeChanges(roots.map(root => topic("files", root)), async () => {
      await refresh(p.name);
      const error = list(p, null, p.query).error;
      if (error) throw new Error(error);
    }, p.data.machine_id, () => p.nav.isConnected && p.detail.isConnected);
  }

  function stop() {
    if (following) clearTimeout(following.timer);
    unfollow?.();
    unfollow = undefined;
    following = null;
    watched = "";
  }
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
    const chrome = el("div");
    chrome.style.display = "contents";
    const nav = el("aside", "pj-list gh-nav", chrome, tree, foot);
    nav.setAttribute("aria-label", "Git navigation");
    const detail = el("div", "pj-det gh-detail");
    const headerElement = el("div");
    headerElement.style.display = "contents";
    p = { name, lists: new Map(), files: new Map(), commits: new Map(), open: new Set(["history"]), selected: null,
      path: null, tab: "changes", query: "", nav, chrome, tree, detail, header: { element: headerElement }, sessionsView: null, pathView: null, foot, facts: null, view: null, viewKey: "" };
    projects.set(name, p);
    drawNavigation(p);
    return p;
  }

  function renderHistory(p: ProjectHistory, slot: string, host: HTMLElement, content: ReactNode) {
    showWindowParts([{ part: `git-history:${p.name}:${slot}`, host, content }]);
  }

  function drawNavigation(p: ProjectHistory) {
    renderHistory(p, "navigation", p.chrome, createElement(HistoryNavigation, {
      footer: p.foot, machine: p.data?.machine_name ?? null, query: p.query, back: deps.back,
      refresh: () => void refresh(p.name), collapse: () => { p.open.clear(); renderTree(p); },
      search: (query: string) => {
        p.query = query;
        clearTimeout(p.timer);
        p.timer = setTimeout(() => {
          const found = list(p, null, p.query);
          p.open.add("history");
          renderTree(p);
          if (!found.loaded && !found.loading) void load(p, found);
        }, 250);
      },
    }));
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
      if (following === p) follow(p);
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

  function node(p: ProjectHistory, key: string, label: ReactNode[], children: () => ReactNode[], on?: () => void,
    options: { commit?: boolean; title?: string; current?: boolean } = {}) {
    const open = p.open.has(key);
    return createElement(TreeNode, { key, nodeKey: key, label, open, ...options, onSelect: on, onToggle: (isOpen: boolean) => {
      if (isOpen) p.open.add(key); else p.open.delete(key);
      renderTree(p);
    } }, open ? children() : null);
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

  function changedFiles(p: ProjectHistory, selected: GitSelection): ReactNode[] {
    const result = fileRead(p, selected);
    if (!result.loaded && !result.loading && !result.error) void readFiles(p, selected);
    const rows: ReactNode[] = result.rows.map(f => createElement("button", {
      key: f.path, type: "button", className: "gh-file", "data-path": f.path,
      "data-selection": selectionKey(selected), title: f.old_path ? `${f.old_path} -> ${f.path}` : f.path,
      "aria-current": String(!!p.selected && selectionKey(p.selected) === selectionKey(selected) && p.path === f.path),
      onClick: () => { p.selected = selected; p.path = f.path; p.tab = "changes"; renderTree(p); renderDetail(p); },
    }, createElement("span", { className: "gh-status" }, STATUS_LETTER[f.status]), createElement("span", null, f.path)));
    if (result.loading) rows.push(createElement("p", { key: "loading", className: "gh-empty" }, "Reading changed files…"));
    if (result.error) rows.push(treeError(result.error, () => { result.error = ""; result.loaded = false; void readFiles(p, selected); }));
    if (result.loaded && !rows.length) rows.push(createElement("p", { key: "empty", className: "gh-empty" }, selected.kind === "working" ? "Working tree clean." : "This commit changes no files."));
    return rows;
  }

  function treeError(message: string, retry: () => void) {
    return createElement("div", { key: "error" }, createElement("div", { className: "cc-note" }, createElement("span", { className: "ddot bad" }), createElement("span", { className: "cc-err" }, message)), createElement("button", { type: "button", className: "gh-more", onClick: retry }, "Try again"));
  }

  function commitNode(p: ProjectHistory, c: GitCommit, origin: string, prefix = "") {
    const selected: GitSelection = { kind: "commit", commit: c, origin };
    return node(p, `${prefix}commit:${c.id}`, [createElement("span", { key: "dot", className: `gh-dot${c.parents.length > 1 ? " merge" : ""}` }), createElement("span", { key: "subject", className: "gh-subject" }, c.subject || "Untitled commit"), createElement("code", { key: "id" }, c.id.slice(0, 7))], () => [
      node(p, `${prefix}changes:${c.id}`, [createElement("span", { key: "label" }, "Changes")], () => changedFiles(p, selected), () => select(p, selected)),
    ], () => { select(p, selected); void readFiles(p, selected); }, { commit: true, title: `${c.subject}\n${c.id}`, current: p.selected?.kind === "commit" && p.selected.commit.id === c.id });
  }

  function listStatus(p: ProjectHistory, found: CommitList): ReactNode[] {
    const rows: ReactNode[] = [];
    if (found.loading && !found.loaded) rows.push(createElement("p", { key: "loading", className: "gh-empty" }, "Reading history…"));
    if (found.error) rows.push(treeError(found.error, () => void load(p, found, found.rows.length > 0)));
    if (found.loaded && !found.rows.length) rows.push(createElement("p", { key: "empty", className: "gh-empty" }, found.search ? "No commits match this search." : "No commits yet."));
    if (!found.loading && found.next !== null) rows.push(createElement("button", { key: "more", type: "button", className: "gh-more", onClick: () => void load(p, found, true) }, "Load older commits"));
    return rows;
  }

  function listRows(p: ProjectHistory, found: CommitList, origin: string, prefix = "") {
    return [...found.rows.map(c => commitNode(p, c, origin, prefix)), ...listStatus(p, found)];
  }

  function treeSessions(p: ProjectHistory, w: GitWorktree): ReactNode[] {
    const all = deps.sessions(p.name);
    const related = branchSessions(all || [], p.data?.machine_id, w.path, p.data?.worktrees.map(tree => tree.path));
    return related.length ? related.map(s => createElement("button", {
      key: `${s.machine_id}:${s.agent}:${s.id}`, type: "button", className: "gh-session-link", onClick: () => deps.openSession(s),
    }, createElement("b", null, s.title || "Untitled session"), createElement("span", null, `${agentLabel(s.agent)} · ${s.status} · ${ago(s.last_activity)}`)))
      : [createElement("p", { key: "empty", className: "gh-empty" }, all === undefined ? "Reading sessions…" : "No sessions in this worktree.")];
  }

  function renderTree(p: ProjectHistory) {
    const found = list(p, null, p.query);
    const history = node(p, "history", [createElement("b", { key: "label" }, "All history"), createElement("small", { key: "count" }, found.rows.length ? `${found.rows.length}${found.next !== null ? "+" : ""}` : "")], () => {
      const days = dayGroups(found.rows);
      if (!days.length) return listRows(p, found, "All history");
      if (!p.tree.dataset.started) { p.open.add(`day:${p.query}:${days[0][0]}`); p.tree.dataset.started = "true"; }
      return [...days.map(([day, commits]) => node(p, `day:${p.query}:${day}`, [createElement("span", { key: "day" }, day), createElement("small", { key: "count" }, String(commits.length))], () => commits.map(c => commitNode(p, c, "All history")))), ...listStatus(p, found)];
    });
    const refs = (p.data?.refs || []).filter(r => r.kind !== "tag");
    const branches = node(p, "branches", [createElement("b", { key: "label" }, "Branches"), createElement("small", { key: "count" }, String(refs.length))], () => refs.map(ref => {
      const branchList = list(p, ref.head);
      return node(p, `branch:${ref.kind}:${ref.name}`, [createElement("code", { key: "name" }, ref.name), createElement("small", { key: "kind" }, ref.kind === "remote" ? "remote" : "")], () => {
        if (!branchList.loaded && !branchList.loading && !branchList.error) void load(p, branchList);
        return listRows(p, branchList, `Branches / ${ref.name}`, `${ref.name}:`);
      });
    }));
    const worktrees = node(p, "worktrees", [createElement("b", { key: "label" }, "Worktrees"), createElement("small", { key: "count" }, String(p.data?.worktrees.length ?? 0))], () => (p.data?.worktrees || []).map(w => {
      const selected: GitSelection = { kind: "working", worktree: w };
      return node(p, `worktree:${w.path}`, [createElement("code", { key: "name" }, w.branch || w.head?.slice(0, 7) || "Unborn checkout"), createElement("small", { key: "main" }, w.main ? "main checkout" : "")], () => [
        createElement("div", { key: "location", className: "gh-location" }, `${p.data?.machine_name}\n`, createElement("code", null, w.path)),
        node(p, `working:${w.path}`, [createElement("span", { key: "label" }, "Uncommitted changes")], () => changedFiles(p, selected), () => { select(p, selected); void readFiles(p, selected); }),
        node(p, `sessions:${w.path}`, [createElement("span", { key: "label" }, "Sessions")], () => treeSessions(p, w)),
      ]);
    }));
    drawNavigation(p);
    renderHistory(p, "tree", p.tree, [history, branches, worktrees]);
  }

  function select(p: ProjectHistory, selected: GitSelection) {
    p.selected = selected; p.path = null; p.tab = "changes";
    renderDetail(p);
    renderTree(p);
  }

  async function parent(p: ProjectHistory, id: string) {
    let c = p.commits.get(id);
    if (!c) { const found = list(p, id); await load(p, found); c = p.commits.get(id); }
    if (c) { p.open.add("history"); select(p, { kind: "commit", commit: c, origin: "Parent commit" }); }
  }

  function renderDetail(p: ProjectHistory) {
    if (following === p) follow(p);
    const selected = p.selected;

    if (!selected || !p.data) {
      const found = list(p, null, p.query);
      p.detail.replaceChildren(el("p", "gh-empty", found.loading ? "Reading Git history…" : "Select a commit or worktree to see its changes."));
      return;
    }

    const c = selected.kind === "commit" ? selected.commit : null;
    const w = selected.kind === "working" ? selected.worktree : null;
    const ref = c && p.data.refs.find(r => r.kind === "local" && r.head === c.id);
    renderHistory(p, "header", p.header.element, createElement(HistoryHeader, {
      key: selectionKey(selected), breadcrumb: c ? `${selected.kind === "commit" ? selected.origin : ""} / ${c.id.slice(0, 7)}` : `Worktrees / ${w!.branch || "Detached HEAD"}`,
      title: c ? c.subject || "Untitled commit" : `Uncommitted changes in ${w!.branch || "checkout"}`,
      metadata: c ? `${c.author} · ${new Date(c.committed_at).toLocaleString()}` : `${w!.path} · ${p.data.machine_name}`,
      copyValue: c?.id || w!.path, copyLabel: c ? "Copy SHA" : "Copy path",
      review: ref && deps.canReview(p.name, ref.name) ? () => deps.reviewBranch(p.name, ref.name) : null,
      tab: p.tab, onTab: (tab: ProjectHistory["tab"]) => { p.tab = tab; renderDetail(p); },
    }));

    const drawFacts = () => {
      if (!p.facts) {
        const element = el("div", "gh-facts-root");
        p.facts = { element };
      }
      renderHistory(p, "facts", p.facts.element, createElement(HistoryFacts, {
        id: c?.id || w!.path, author: c?.author ?? null, parents: c?.parents ?? [], onParent: id => void parent(p, id),
      }));
      return p.facts.element;
    };
    const existingBody = p.detail.querySelector<HTMLElement>(".gh-detail-body");
    if (p.tab === "details" && p.facts && existingBody?.contains(p.facts.element)) {
      drawFacts();
      return;
    }
    const currentKey = selected ? `${selectionKey(selected)}\0${p.path || ""}` : "";
    if (p.tab === "changes" && p.view && p.viewKey === currentKey && existingBody?.contains(p.view.el)) {
      return;
    }

    if (p.tab === "sessions") {
      if (!p.sessionsView) {
        const element = el("div", "gh-detail-body");
        p.sessionsView = { element };
      }
      const currentTree = w || (ref && p.data.worktrees.find(tree => tree.branch === ref.name));
      const all = deps.sessions(p.name);
      const sessions = currentTree ? branchSessions(all || [], p.data.machine_id, currentTree.path, p.data.worktrees.map(tree => tree.path)) : [];
      renderHistory(p, "sessions", p.sessionsView.element, createElement(HistorySessions, { linked: !!currentTree, loading: all === undefined, sessions, open: deps.openSession }));
      if (existingBody === p.sessionsView.element) return;
      if (p.header.element.parentElement === p.detail && existingBody) existingBody.replaceWith(p.sessionsView.element);
      else p.detail.replaceChildren(p.header.element, p.sessionsView.element);
      return;
    }

    const body = el("div", "gh-detail-body");

    if (p.tab === "details") {
      body.append(drawFacts());
    } else {
      const path = p.path;
      const key = `${selectionKey(selected)}\0${path || ""}`;

      if (p.viewKey !== key || !p.view) {
        p.view?.dispose();
        p.viewKey = key;
        const location = p.data;
        p.view = diffView({
          owner: windowDiffOwner(),
          readOnly: true,
          load: async (only, lockfiles, _uncommitted, context) => {
            const requested = only.length ? only : path ? [path] : [];
            const paths = diffPaths(fileRead(p, selected).rows, requested);
            const view = c ? await read<DiffView>("project_git_diff", { project: p.name, machineId: location.machine_id, commit: c.id, only: paths, lockfiles: lockfiles || !!path, context })
              : await read<DiffView>("work_diff", { machineId: location.machine_id, root: w!.path, only: paths, lockfiles: lockfiles || !!path, uncommitted: true, context });
            return path ? { ...view, files: view.files.filter(f => f.path === path), more: 0 } : view;
          },
          review: { key: `history:${p.name}:${key}`, heading: () => c?.subject || "Worktree changes", send: async () => "Open a session to send a review." },
        });
      }

      if (path) {
        if (!p.pathView) {
          const element = el("div"); element.style.display = "contents";
          p.pathView = { element };
        }
        renderHistory(p, "path", p.pathView.element, createElement(HistoryPath, { path, showAll: () => { p.path = null; renderDetail(p); } }));
        body.append(p.pathView.element);
      }
      body.append(p.view.el);
    }

    if (p.header.element.parentElement === p.detail && existingBody) existingBody.replaceWith(body);
    else p.detail.replaceChildren(p.header.element, body);
  }

  function panels(name: string) {
    const p = project(name);
    follow(p);
    const found = list(p);
    if (!found.loaded && !found.loading && !found.error) void load(p, found);
    renderTree(p); renderDetail(p);
    return { navigation: p.nav, detail: p.detail };
  }

  function refresh(name: string) {
    const p = project(name);
    for (const key of p.files.keys()) if (key.startsWith("working:")) p.files.delete(key);
    if (p.selected?.kind === "working" && p.viewKey === `${selectionKey(p.selected)}\0${p.path || ""}`) p.view?.reload();
    const current = list(p, null, p.query);
    for (const value of p.lists.values()) { ++value.version; value.loading = false; if (value !== current) value.loaded = false; value.error = ""; }
    return load(p, current);
  }
  return { panels, refresh, stop };
}
