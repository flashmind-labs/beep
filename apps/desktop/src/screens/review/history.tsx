import type { ReactNode } from "react";
import { showWindowParts } from "../../parts/center/window-root.tsx";
import { retainedHost } from "../project/page-view.tsx";
import { HistoryFacts } from "./history-facts.tsx";
import { HistoryDetail } from "./history-detail.tsx";
import { HistoryHeader } from "./history-header.tsx";
import { CheckoutBranch, HistoryNavigation } from "./history-navigation.tsx";
import { HistoryPath, HistorySessions, SessionLink } from "./history-sessions.tsx";
import { TreeNode } from "./history-tree.tsx";
import { subscribeEach, topic } from "../../ui/mesh-events.ts";
import { changedIn, touchesRepo } from "../../stores/agentpane/touched.ts";
// The project's Git navigator and its selected commit or worktree diff.
import { reason } from "../../ui/format.ts";
import type { invoke as Invoke } from "../../ui/tauri.ts";
import type { SessionRow } from "../../stores/project/projects-state.ts";
import type { GitRow } from "../../stores/project/gitsync-state.ts";
import { branchSessions } from "../../stores/review/branchsession-state.ts";
import { windowDiffOwner } from "../../parts/review/diff-owner.ts";
import { diffView, type DiffViewHandle } from "../../parts/review/diff.tsx";
import { type DiffView, STATUS_LETTER } from "../../stores/review/diff-state.ts";
import { appendCommits, dayGroups, diffPaths, selectionKey, type BranchSessionRow, type ChangedFile, type GitCommit, type GitSelection, type GitWorktree, type HistoryPage } from "../../stores/review/history-state.ts";

interface HistoryDeps {
  invoke: typeof Invoke;
  back: () => void;
  sessions: (project: string) => readonly BranchSessionRow[] | undefined;
  openSession: (session: SessionRow) => void;
  changedCheckout: (project: string, row: GitRow) => void;
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
  /// The two panels the project page places; everything in them is React.
  nav: HTMLElement;
  detail: HTMLElement;
  /// Whether the newest day was opened once, so closing it stays closed.
  started: boolean;
  view: DiffViewHandle | null;
  viewKey: string;
  timer?: ReturnType<typeof setTimeout>;
}

const HISTORY_SECTIONS = ["history", "branches", "worktrees"] as const;

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
    // Each push is read once, when it says a file or Git's HEAD, index or refs changed.
    unfollow = subscribeEach(roots.map(root => topic("files", root)), async event => {
      if (event.query != null || !touchesRepo(changedIn(event.data))) return;
      await refresh(p.name);
    }, { machine: p.data.machine_id, when: () => p.nav.isConnected && p.detail.isConnected });
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
    const nav = retainedHost("pj-list gh-nav", { tag: "aside", label: "Git navigation" }), detail = retainedHost("pj-det gh-detail");
    p = { name, lists: new Map(), files: new Map(), commits: new Map(), open: new Set(["history"]), selected: null,
      path: null, tab: "changes", query: "", nav, detail, started: false, view: null, viewKey: "" };
    projects.set(name, p);
    renderTree(p);
    return p;
  }

  function renderHistory(p: ProjectHistory, slot: string, host: HTMLElement, content: ReactNode) {
    showWindowParts([{ part: `git-history:${p.name}:${slot}`, host, content }]);
  }

  function navigation(p: ProjectHistory) {
    const data = p.data;
    const worktree = data?.worktrees.find(tree => tree.path === data.root);
    const checkout = data && worktree ? <CheckoutBranch data={data} worktree={worktree}
      switchTo={branch => switchTo(p, data, worktree, branch)} /> : null;
    const collapsed = !HISTORY_SECTIONS.some(key => p.open.has(key));

    return <HistoryNavigation query={p.query} back={deps.back} collapsed={collapsed} checkout={checkout}
      refresh={() => void refresh(p.name)} toggleSections={() => {
        if (collapsed) {
          for (const key of HISTORY_SECTIONS) p.open.add(key);
        } else {
          p.open.clear();
        }
        renderTree(p);
      }}
      search={(query: string) => {
        p.query = query;
        clearTimeout(p.timer);
        p.timer = setTimeout(() => {
          const found = list(p, null, p.query);
          p.open.add("history");
          renderTree(p);
          if (!found.loaded && !found.loading) void load(p, found);
        }, 250);
      }} />;
  }

  async function switchTo(p: ProjectHistory, data: HistoryPage, worktree: GitWorktree, branch: string) {
    try {
      const row = await deps.invoke<GitRow>("project_git_switch", {
        project: p.name, machineId: data.machine_id, root: data.root, current: worktree.branch, branch,
      });
      if (p.data?.root === data.root && p.data.machine_id === data.machine_id) {
        updatePage(p, { ...p.data, worktrees: p.data.worktrees.map(tree => tree.path === worktree.path ? { ...tree, branch: row.branch } : tree) });
      }
      deps.changedCheckout(p.name, row);
    } finally {
      void refresh(p.name, true);
    }
  }

  function updatePage(p: ProjectHistory, data: HistoryPage) {
    p.data = data;
    const selected = p.selected;
    if (selected?.kind === "working") {
      const worktree = data.worktrees.find(tree => tree.path === selected.worktree.path);
      if (worktree) p.selected = { ...selected, worktree };
    }
  }

  async function load(p: ProjectHistory, target: CommitList, more = false, fresh = false) {
    if (target.loading) return;
    const version = ++target.version;
    target.loading = true;
    target.error = "";
    renderTree(p);

    try {
      const data = await read<HistoryPage>("project_git_history", {
        project: p.name, machineId: p.data?.machine_id ?? null, revision: target.revision,
        offset: more ? target.next ?? 0 : 0, search: target.search, fresh,
      });
      if (version !== target.version) return;
      if (!p.data || (!target.revision && !more)) updatePage(p, data);
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

  function node(p: ProjectHistory, key: string, label: ReactNode, children: () => ReactNode[], on?: () => void,
    options: { commit?: boolean; title?: string; current?: boolean } = {}) {
    const open = p.open.has(key);
    return <TreeNode key={key} nodeKey={key} label={label} open={open} {...options} onSelect={on} onToggle={(isOpen: boolean) => {
      if (isOpen) p.open.add(key); else p.open.delete(key);
      renderTree(p);
    }}>{open ? children() : null}</TreeNode>;
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
    const rows: ReactNode[] = result.rows.map(f => <button key={f.path} type="button" className="gh-file" data-path={f.path}
      data-selection={selectionKey(selected)} title={f.old_path ? `${f.old_path} -> ${f.path}` : f.path}
      aria-current={!!p.selected && selectionKey(p.selected) === selectionKey(selected) && p.path === f.path}
      onClick={() => { p.selected = selected; p.path = f.path; p.tab = "changes"; renderTree(p); renderDetail(p); }}>
      <span className="gh-status">{STATUS_LETTER[f.status]}</span><span>{f.path}</span></button>);
    if (result.loading) rows.push(<p key="loading" className="gh-empty">Reading changed files…</p>);
    if (result.error) rows.push(treeError(result.error, () => { result.error = ""; result.loaded = false; void readFiles(p, selected); }));
    if (result.loaded && !rows.length) rows.push(<p key="empty" className="gh-empty">{selected.kind === "working" ? "Working tree clean." : "This commit changes no files."}</p>);
    return rows;
  }

  function treeError(message: string, retry: () => void) {
    return <div key="error"><div className="cc-note"><span className="ddot bad" /><span className="cc-err">{message}</span></div><button type="button" className="gh-more" onClick={retry}>Try again</button></div>;
  }

  function commitNode(p: ProjectHistory, c: GitCommit, origin: string, prefix = "") {
    const selected: GitSelection = { kind: "commit", commit: c, origin };
    return node(p, `${prefix}commit:${c.id}`, <><span className={`gh-dot${c.parents.length > 1 ? " merge" : ""}`} /><span className="gh-subject">{c.subject || "Untitled commit"}</span><code>{c.id.slice(0, 7)}</code></>, () => [
      node(p, `${prefix}changes:${c.id}`, <span>Changes</span>, () => changedFiles(p, selected), () => select(p, selected)),
    ], () => { select(p, selected); void readFiles(p, selected); }, { commit: true, title: `${c.subject}\n${c.id}`, current: p.selected?.kind === "commit" && p.selected.commit.id === c.id });
  }

  function listStatus(p: ProjectHistory, found: CommitList): ReactNode[] {
    const rows: ReactNode[] = [];
    if (found.loading && !found.loaded) rows.push(<p key="loading" className="gh-empty">Reading history…</p>);
    if (found.error) rows.push(treeError(found.error, () => void load(p, found, found.rows.length > 0)));
    if (found.loaded && !found.rows.length) rows.push(<p key="empty" className="gh-empty">{found.search ? "No commits match this search." : "No commits yet."}</p>);
    if (!found.loading && found.next !== null) rows.push(<button key="more" type="button" className="gh-more" onClick={() => void load(p, found, true)}>Load older commits</button>);
    return rows;
  }

  function listRows(p: ProjectHistory, found: CommitList, origin: string, prefix = "") {
    return [...found.rows.map(c => commitNode(p, c, origin, prefix)), ...listStatus(p, found)];
  }

  function treeSessions(p: ProjectHistory, w: GitWorktree): ReactNode[] {
    const all = deps.sessions(p.name);
    const related = branchSessions(all || [], p.data?.machine_id, w.path, p.data?.worktrees.map(tree => tree.path));
    return related.length ? related.map(s => <SessionLink key={`${s.machine_id}:${s.agent}:${s.id}`} session={s} open={deps.openSession} />)
      : [<p key="empty" className="gh-empty">{all === undefined ? "Reading sessions…" : "No sessions in this worktree."}</p>];
  }

  function renderTree(p: ProjectHistory) {
    const found = list(p, null, p.query);
    const history = node(p, "history", <><b>All history</b><small>{found.rows.length ? `${found.rows.length}${found.next !== null ? "+" : ""}` : ""}</small></>, () => {
      const days = dayGroups(found.rows);
      if (!days.length) return listRows(p, found, "All history");
      if (!p.started) { p.open.add(`day:${p.query}:${days[0][0]}`); p.started = true; }
      return [...days.map(([day, commits]) => node(p, `day:${p.query}:${day}`, <><span>{day}</span><small>{commits.length}</small></>, () => commits.map(c => commitNode(p, c, "All history")))), ...listStatus(p, found)];
    });
    const refs = (p.data?.refs || []).filter(r => r.kind !== "tag");
    const branches = node(p, "branches", <><b>Branches</b><small>{refs.length}</small></>, () => refs.map(ref => {
      const branchList = list(p, ref.head);
      return node(p, `branch:${ref.kind}:${ref.name}`, <><code>{ref.name}</code><small>{ref.kind === "remote" ? "remote" : ""}</small></>, () => {
        if (!branchList.loaded && !branchList.loading && !branchList.error) void load(p, branchList);
        return listRows(p, branchList, `Branches / ${ref.name}`, `${ref.name}:`);
      });
    }));
    const worktrees = node(p, "worktrees", <><b>Worktrees</b><small>{p.data?.worktrees.length ?? 0}</small></>, () => (p.data?.worktrees || []).map(w => {
      const selected: GitSelection = { kind: "working", worktree: w };
      return node(p, `worktree:${w.path}`, <><code>{w.branch || w.head?.slice(0, 7) || "Unborn checkout"}</code><small>{w.main ? "main checkout" : ""}</small></>, () => [
        <div key="location" className="gh-location">{`${p.data?.machine_name}\n`}<code>{w.path}</code></div>,
        node(p, `working:${w.path}`, <span>Uncommitted changes</span>, () => changedFiles(p, selected), () => { select(p, selected); void readFiles(p, selected); }),
        node(p, `sessions:${w.path}`, <span>Sessions</span>, () => treeSessions(p, w)),
      ]);
    }));
    const machine = p.data?.machine_name ?? null;
    renderHistory(p, "navigation", p.nav, <>
      <div style={{ display: "contents" }}>{navigation(p)}</div>
      <div className="gh-tree" data-started={p.started ? "true" : undefined}>{history}{branches}{worktrees}</div>
      <div className="gh-nav-foot">{machine ? `All branches · ${machine}` : ""}</div>
    </>);
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
      renderHistory(p, "detail", p.detail, <p className="gh-empty">{found.loading ? "Reading Git history…" : "Select a commit or worktree to see its changes."}</p>);
      return;
    }

    const c = selected.kind === "commit" ? selected.commit : null;
    const w = selected.kind === "working" ? selected.worktree : null;
    const ref = c && p.data.refs.find(r => r.kind === "local" && r.head === c.id);
    const shown = selected.kind === "commit"
      ? { breadcrumb: `${selected.origin} / ${selected.commit.id.slice(0, 7)}`, title: selected.commit.subject || "Untitled commit",
        metadata: `${selected.commit.author} · ${new Date(selected.commit.committed_at).toLocaleString()}`, id: selected.commit.id, copyLabel: "Copy SHA" }
      : { breadcrumb: `Worktrees / ${selected.worktree.branch || "Detached HEAD"}`, title: `Uncommitted changes in ${selected.worktree.branch || "checkout"}`,
        metadata: `${selected.worktree.path} · ${p.data.machine_name}`, id: selected.worktree.path, copyLabel: "Copy path" };
    const header = <HistoryHeader key={selectionKey(selected)} breadcrumb={shown.breadcrumb} title={shown.title} metadata={shown.metadata}
      copyValue={shown.id} copyLabel={shown.copyLabel}
      tab={p.tab} onTab={(tab: ProjectHistory["tab"]) => { p.tab = tab; renderDetail(p); }} />;
    let body: ReactNode = null, diff: HTMLElement | null = null;
    if (p.tab === "details") {
      body = <div className="gh-facts-root"><HistoryFacts
        id={shown.id} author={c?.author ?? null} parents={c?.parents ?? []} onParent={id => void parent(p, id)} /></div>;
    } else if (p.tab === "sessions") {
      const currentTree = w || (ref && p.data.worktrees.find(tree => tree.branch === ref.name));
      const all = deps.sessions(p.name);
      const sessions = currentTree ? branchSessions(all || [], p.data.machine_id, currentTree.path, p.data.worktrees.map(tree => tree.path)) : [];
      body = <HistorySessions linked={!!currentTree} loading={all === undefined} sessions={sessions} open={deps.openSession} />;
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
            const view = selected.kind === "commit"
              ? await read<DiffView>("project_git_diff", { project: p.name, machineId: location.machine_id, commit: selected.commit.id, only: paths, lockfiles: lockfiles || !!path, context })
              : await read<DiffView>("work_diff", { machineId: location.machine_id, root: selected.worktree.path, only: paths, lockfiles: lockfiles || !!path, uncommitted: true, context });
            return path ? { ...view, files: view.files.filter(f => f.path === path), more: 0 } : view;
          },
          review: { key: `history:${p.name}:${key}`, heading: () => c?.subject || "Worktree changes", send: async () => "Open a session to send a review." },
        });
      }
      if (path) body = <HistoryPath path={path} showAll={() => { p.path = null; renderDetail(p); }} />;
      diff = p.view.el;
    }
    renderHistory(p, "detail", p.detail, <HistoryDetail header={header} body={body} diff={diff} />);
  }

  function panels(name: string) {
    const p = project(name);
    follow(p);
    const found = list(p);
    if (!found.loaded && !found.loading && !found.error) void load(p, found);
    renderTree(p); renderDetail(p);
    return { navigation: p.nav, detail: p.detail };
  }

  function refresh(name: string, fresh = false) {
    const p = project(name);
    for (const key of p.files.keys()) if (key.startsWith("working:")) p.files.delete(key);
    if (p.selected?.kind === "working" && p.viewKey === `${selectionKey(p.selected)}\0${p.path || ""}`) p.view?.reload();
    const current = list(p, null, p.query);
    for (const value of p.lists.values()) { ++value.version; value.loading = false; if (value !== current) value.loaded = false; value.error = ""; }
    return load(p, current, false, fresh);
  }
  return { panels, refresh, stop };
}
