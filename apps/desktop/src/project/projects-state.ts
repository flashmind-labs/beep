import type { AgentQuestion, RunningAgent, RunningRow } from "../center/home-state.ts";
import { plural } from "../ui/format.ts";
// Projects as the window lists them, their groups and the overview's cards,
// as pure functions so they can be tested without Tauri.

/// One imported project on one machine (ProjectRow), as `import_project`
/// gives it and Home lists them.
export interface ProjectRow {
  /// `org/repo`.
  name: string;
  url: string;
  machine_id: string;
  machine_name: string;
  folder: string;
  cloned: boolean;
  /// In milliseconds since the Unix epoch.
  imported_at: number;
}

/// Where a project is on one machine.
export interface ProjectPlace {
  id: string;
  name: string;
  folder: string;
}

/// A project as the window lists it: imported, or a folder agents run in.
export interface Project {
  name: string;
  url: string | null;
  machines: ProjectPlace[];
  /// The coding agents running in it now.
  agents: number;
  /// When one of them last did something, in milliseconds.
  last: number | null;
  imported: boolean;
}

/// A folder on one of the user's machines, as `list_machine_folder` reads it.
export interface MachineFolder {
  path: string;
  /// Null at the root.
  parent: string | null;
  /// False for a folder that is not there yet; an import makes it.
  exists: boolean;
  folders: string[];
  /// Folders left out past the limit.
  more: number;
  empty: boolean;
  /// A git checkout's `origin` remote, empty when it has none; null when it
  /// is no checkout.
  origin: string | null;
}

/// A git checkout the user's coding agents worked in (FoundRow).
export interface FoundRow {
  name: string;
  /// What `import_project` takes as the repository.
  remote: string;
  places: { machine_id: string; machine_name: string; folder: string }[];
  agents: { agent: string; sessions: number }[];
  last_activity: number;
}

/// One of the user's machines as a project's page lists it (ProjectMachineRow).
export interface ProjectMachineRow {
  id: string;
  name: string;
  online: boolean;
  /// The project's folder there; null without a checkout.
  folder: string | null;
  /// The server is looking through it for a checkout now.
  checking: boolean;
  checked_at: number | null;
  /// The Syncthing folder that keeps it in step there, by label; Beep
  /// does not sync such a folder (from `project_syncthing`).
  syncthing?: string | null;
}

/// A machine whose folder of a project Syncthing keeps in step
/// (SyncthingPlace).
export interface SyncthingPlace {
  machine_id: string;
  folder: string;
  label: string;
}

/// `machines` with each folder Syncthing keeps in step marked.
export function withSyncthing(machines: ProjectMachineRow[] | null, places: readonly SyncthingPlace[]): ProjectMachineRow[] | null {
  if (!machines) return machines;
  return machines.map(m => {
    const place = places.find(p => p.machine_id === m.id && p.folder === m.folder);
    return place ? { ...m, syncthing: place.label } : m;
  });
}

/// A coding agent's session, as `project_sessions` lists them (SessionRow).
export interface SessionRow {
  machine_id: string;
  machine_name: string;
  id: string;
  agent: string;
  title: string | null;
  cwd: string;
  status: string;
  doing: string | null;
  started_at: number;
  last_activity: number;
  external: boolean;
  question: AgentQuestion | null;
}

/// Tasks by state (TaskCounts).
export interface TaskCounts {
  planned: number;
  running: number;
  blocked: number;
  review: number;
  done: number;
}

/// The task that changed last in a project (LatestTask).
export interface LatestTask {
  slug: string;
  title: string;
  /// `planned`, `running`, `blocked`, `review` or `done`.
  state: string;
  pr_url: string | null;
  updated_at: number;
}

/// The server's overview of one project with an agent (OverviewCard).
export interface OverviewCard {
  project_id: string;
  name: string;
  tasks: TaskCounts;
  /// Workers that have not ended.
  workers: number;
  latest: LatestTask | null;
}

/// Whether `cwd` is `folder` or inside it.
function within(cwd: string, folder: string) {
  const f = String(folder || "").replace(/[\\/]+$/, "");
  return !!f && (cwd === f || String(cwd || "").startsWith(f + "/"));
}

const isAgent = (r: RunningRow): r is RunningAgent => r.kind === "agent";

/// The imported projects, one row each, with the agents running in their
/// folders counted.
export function mergeProjects(
  imported: readonly Pick<ProjectRow, "name" | "url" | "machine_id" | "machine_name" | "folder">[] | null | undefined,
  running: readonly RunningRow[] | null | undefined,
): Project[] {
  const rows = new Map<string, Project>();
  for (const p of imported || []) {
    const row = rows.get(p.name) || { name: p.name, url: p.url, machines: [], agents: 0, last: null, imported: true };
    row.machines.push({ id: p.machine_id, name: p.machine_name, folder: p.folder });
    rows.set(p.name, row);
  }
  for (const r of (running || []).filter(isAgent)) {
    const row = [...rows.values()].find(p => p.machines.some(m => m.id === r.machine_id && within(r.cwd, m.folder)));
    if (!row) continue;
    row.agents += 1;
    const at = r.last_activity || r.started_at || null;
    if (at && (!row.last || at > row.last)) row.last = at;
  }
  return [...rows.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/// The rows the page shows: all, or only `name` once one is picked.
export function filterProjects<T extends { name: string }>(rows: T[], name: string | null | undefined) {
  return name ? rows.filter(r => r.name === name) : rows;
}

// ---------- Add a project, and one project's view ----------

/// What each coding agent is called on screen.
const AGENT_NAMES: Record<string, string> = { claude: "Claude Code", codex: "Codex", pi: "pi", flsh: "flsh", gemini: "Gemini CLI", opencode: "OpenCode" };

/// "Claude Code" for `claude`; an agent Beep does not know reads as named.
export function agentLabel(agent: string | null | undefined) { return AGENT_NAMES[agent ?? ""] || String(agent || ""); }

/// How many sessions a found row's agents ran, together.
export function foundSessions(row: Pick<FoundRow, "agents"> | null | undefined) {
  return (row?.agents || []).reduce((n, a) => n + (a.sessions || 0), 0);
}

/// What `import_project` takes for one place.
export type ImportArgs = {
  repo: string;
  machineId: string;
  folder: string;
};

/// The imports a found row makes: its folder on each machine, in place.
export function importPlan(row: Pick<FoundRow, "remote" | "places"> | null | undefined): ImportArgs[] {
  if (!row) return [];
  return (row.places || []).map(p => ({ repo: row.remote, machineId: p.machine_id, folder: p.folder }));
}

/// The line while the Add a project sheet waits on the machines.
export function lookingText(machines: readonly { online: boolean }[] | null | undefined) {
  const n = (machines || []).filter(m => m.online).length;
  const on = n === 1 ? "1 machine" : n ? `${n} machines` : "your machines";
  return `Looking through Claude Code, Codex, pi, flsh and OpenCode sessions on ${on}…`;
}

/// A project's past sessions without those running now (they show under
/// Agents), newest first.
export function pastSessions<T extends Pick<SessionRow, "machine_id" | "id" | "last_activity">>(sessions: readonly T[] | null | undefined, running: readonly { machine_id: string; id: string }[] | null | undefined) {
  const live = new Set((running || []).map(r => `${r.machine_id} ${r.id}`));
  return [...(sessions || [])]
    .filter(s => !live.has(`${s.machine_id} ${s.id}`))
    .sort((a, b) => (b.last_activity || 0) - (a.last_activity || 0));
}

/// A project's session list after a read: `got`, else (`got` null, the read
/// failed) the list shown before, else none.
export function nextSessions<T>(before: T[] | undefined, got: T[] | null) {
  const list = got ?? before ?? [];
  return { list, changed: before === undefined || JSON.stringify(before) !== JSON.stringify(list) };
}

/// "Open on GitHub", or "Open on <host>" for another host's web page; null
/// without one.
export function openOnLabel(url: string) {
  let host = "";
  try { host = new URL(url).host; } catch { return null; }
  return host === "github.com" ? "Open on GitHub" : `Open on ${host}`;
}

/// The menu line and button that remove a project from Beep.
export const REMOVE_PROJECT = "Remove project…";

/// What the sheet that asks before removing project `name` says.
export function removeProjectSheet(name: string) {
  return {
    title: `Remove ${name}?`,
    text: "Beep stops syncing it and its workers, and deletes its chat and tasks.",
    note: "Files on your machines stay.",
    action: "Remove project",
  };
}

/// When a project was first imported, in words: "today", "yesterday",
/// "3 days ago", "5 weeks ago". `at` and `now` are in milliseconds.
export function importedWhen(at: number, now: number) {
  const midnight = (ms: number) => new Date(ms).setHours(0, 0, 0, 0);
  const days = Math.round((midnight(now) - midnight(at)) / 86400000);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 14) return `${days} days ago`;
  return `${Math.round(days / 7)} weeks ago`;
}

/// What the machines found of repository `repo`, null when none has it.
export function foundFor(found: readonly FoundRow[] | null | undefined, repo: string) {
  const name = repo.toLowerCase();
  return found?.find(f => f.name.toLowerCase() === name) ?? null;
}

/// Whether a project's page waits for its agent's view: the project has an
/// agent, or `agents` (undefined until first read) cannot say yet.
export function awaitsView(project: { name: string; imported: boolean }, agents: readonly { name: string }[] | null | undefined) {
  return project.imported && (agents === undefined || !!agents?.some(a => a.name === project.name));
}

/// The line under a project's name: its host and when it was imported, or where
/// it came from when it is not imported.
export function projectLine(project: { name: string; imported: boolean; url?: string | null } | null | undefined, imported: readonly Pick<ProjectRow, "name" | "imported_at">[] | null | undefined, now: number) {
  if (!project) return "";
  if (!project.imported) return "From running agents";
  let host = "";
  try { host = new URL(String(project.url)).host; } catch {}
  const times = (imported || []).filter(r => r.name === project.name && r.imported_at).map(r => r.imported_at);
  const when = times.length ? `imported ${importedWhen(Math.min(...times), now)}` : "";
  return [host, when].filter(Boolean).join(" · ");
}

/// A row that keeps `i`, its place in the list it came from, for its colour.
export type Placed<T> = T & { i: number };

/// The projects pinned to the top of the sidebar, in the order they were
/// pinned, and the rest of `rows`.
export function splitOnTop<T extends { name: string }>(rows: readonly T[] | null | undefined, top: readonly string[] | null | undefined) {
  const all: Placed<T>[] = (rows || []).map((p, i) => ({ ...p, i }));
  const byName = new Map(all.map(p => [p.name, p]));
  const names = [...new Set(top || [])];
  return {
    onTop: names.filter(n => byName.has(n)).map(n => byName.get(n)!),
    rest: all.filter(p => !names.includes(p.name)),
  };
}

/// A project group (ProjectGroup): folders the user lists projects under,
/// in place of their org.
export interface ProjectGroup {
  id: string;
  name: string;
  /// As `groupPath` gives them, such as `work`.
  paths: string[];
}

/// What follows the home folder in `path` (`~/`, `/home/<user>/`,
/// `/Users/<user>/`, `C:/Users/<user>/`), or null outside it.
function homeRelative(path: string) {
  if (path === "~") return "";
  if (path.startsWith("~/")) return path.slice(2);
  const unrooted = /^[A-Za-z]:\//.test(path) ? path.slice(2) : path;
  for (const root of ["/home/", "/Users/"]) {
    if (!unrooted.startsWith(root)) continue;
    const rest = unrooted.slice(root.length);
    const cut = rest.indexOf("/");
    if (cut < 0) return rest ? "" : null;
    return cut > 0 ? rest.slice(cut + 1) : null;
  }
  return null;
}

/// The folder a start form shows once `machine` is picked: the project's
/// checkout there, else `cwd` under the home folder written `~/...`,
/// which each machine reads as its own home. Auto is `machine` null.
export function folderOn(cwd: string, machine: string | null, places: readonly { machine: string; cwd: string }[] | null | undefined) {
  const own = machine ? places?.find(p => p.machine === machine)?.cwd : null;
  if (own) return own;
  const p = String(cwd || "").trim();
  const rest = p ? homeRelative(p.replace(/\\/g, "/")) : null;
  if (rest == null) return p;
  return rest ? `~/${rest.replace(/\/+$/, "")}` : "~";
}

/// A folder as groups compare it, as the server's `project_group_path` keeps
/// them: `\` read as `/`, the home folder and then a leading `Projects/` (any
/// case) left out, with no trailing `/`.
export function groupPath(path: string | null | undefined) {
  const p = String(path || "").trim().replace(/\\/g, "/");
  const rest = homeRelative(p);
  if (rest == null) {
    const trimmed = p.replace(/\/+$/, "");
    return !trimmed && p.startsWith("/") ? "/" : trimmed;
  }
  const inner = rest.replace(/^\/+|\/+$/g, "");
  const cut = inner.indexOf("/");
  const first = cut < 0 ? inner : inner.slice(0, cut);
  const after = first.toLowerCase() === "projects" ? (cut < 0 ? "" : inner.slice(cut + 1)) : inner;
  return after.replace(/^\/+|\/+$/g, "");
}

/// Whether `folder`, on any machine, is the group folder `path` (as
/// `groupPath` gives it) or inside it.
export function inGroupPath(folder: string | null | undefined, path: string) {
  if (!path) return false;
  const f = groupPath(folder);
  if (f === path) return true;
  return f.startsWith(path.endsWith("/") ? path : path + "/");
}

/// The group a project is listed under: the one with the deepest folder
/// that holds one of its folders, the older group on a tie; null for none.
export function groupOf(project: { machines?: readonly { folder: string }[] | null }, groups: readonly ProjectGroup[] | null | undefined) {
  let best: ProjectGroup | null = null, depth = -1;
  for (const g of groups || []) {
    for (const path of g.paths) {
      if (path.length > depth && (project.machines || []).some(m => inGroupPath(m.folder, path))) {
        best = g;
        depth = path.length;
      }
    }
  }
  return best;
}

/// The group sheet's folders after adding `text`, or why it is not added.
/// A folder is kept as `groupPath` gives it, once.
export function addGroupFolder(paths: readonly string[], text: string): { paths: string[]; error: string | null } {
  const path = groupPath(text);
  if (!text.trim()) return { paths: [...paths], error: null };
  if (!path || path === "/") return { paths: [...paths], error: `${text.trim()} holds every project; pick a folder inside it.` };
  if (paths.includes(path)) return { paths: [...paths], error: "That folder is in the group already." };
  return { paths: [...paths, path], error: null };
}

/// How many of `rows` a group of `paths` would hold.
export function groupMatches(rows: readonly { machines?: readonly { folder: string }[] | null }[] | null | undefined, paths: readonly string[]) {
  return (rows || []).filter(p => (p.machines || []).some(m => paths.some(path => inGroupPath(m.folder, path)))).length;
}

/// An org's heading among the sidebar's projects.
export interface OrgHeading {
  org: string;
  /// Folded: the projects it hides.
  folded?: string[];
  group?: undefined;
  name?: undefined;
  repo?: undefined;
  i?: undefined;
}

/// A group's heading among the sidebar's projects.
export interface GroupHeading {
  group: ProjectGroup;
  /// Folded: the projects it hides.
  folded?: string[];
  org?: undefined;
  name?: undefined;
  repo?: undefined;
  i?: undefined;
}

/// A project row in the sidebar: `repo` is what shows under its heading,
/// the name after the org's slash, or the whole name under a group's.
export type SideProject<T> = Placed<T> & { repo?: string; org?: undefined; group?: undefined; folded?: undefined };

/// A project row that groups can place: its name and where it is.
type Groupable = { name: string; i?: number; machines?: readonly { folder: string }[] | null };

/// The sidebar's project rows: each group with projects in it, as a `{ group }`
/// heading then its projects by full name, then by GitHub org or user, an `{
/// org }` heading then that org's projects with `repo` set to the name after
/// the slash. A heading whose `foldKey` is in `folded` stands alone, naming
/// the projects it hides.
export function sideProjects<T extends Groupable>(rows: readonly T[] | null | undefined, groups: readonly ProjectGroup[] | null | undefined = [], folded: ReadonlySet<string> = new Set()): (OrgHeading | GroupHeading | SideProject<T>)[] {
  const inGroup = new Map<string, SideProject<T>[]>(), orgs = new Map<string, SideProject<T>[]>(), loose: SideProject<T>[] = [];
  (rows || []).forEach((p, at) => {
    const i = p.i ?? at;
    const group = groupOf(p, groups);
    if (group) {
      if (!inGroup.has(group.id)) inGroup.set(group.id, []);
      return inGroup.get(group.id)!.push({ ...p, i, repo: p.name });
    }
    const cut = p.name.indexOf("/");
    if (cut <= 0) return loose.push({ ...p, i });
    const org = p.name.slice(0, cut);
    if (!orgs.has(org)) orgs.set(org, []);
    orgs.get(org)!.push({ ...p, i, repo: p.name.slice(cut + 1) });
  });
  const under = <H extends OrgHeading | GroupHeading>(head: H, list: SideProject<T>[]): (H | SideProject<T>)[] =>
    folded.has(foldKey(head)) ? [{ ...head, folded: list.map(p => p.name) }] : [head, ...list];
  const grouped = (groups || []).filter(g => inGroup.has(g.id)).flatMap(group => under({ group }, inGroup.get(group.id)!));
  return [...grouped, ...[...orgs].flatMap(([org, list]) => under({ org }, list)), ...loose];
}

/// What remembers a heading as folded; a group's survives a rename.
export function foldKey(head: OrgHeading | GroupHeading) {
  return head.group ? `group:${head.group.id}` : `org:${head.org}`;
}

/// The folded headings as saved; anything unreadable folds nothing.
export function parseFolded(text: string | null): Set<string> {
  try {
    const v: unknown = JSON.parse(text ?? "[]");
    return new Set(Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

/// What keeps project `name` in step, for its card: Beep's sync when a
/// `project_sync` row names it, Syncthing when `where`, the machines its page
/// last read, says so; "" for neither.
export function syncedBy(name: string, rows: readonly { project: string }[], where: readonly { syncthing?: string | null }[] | null | undefined) {
  return [rows.some(r => r.project === name) && "Beep sync", where?.some(m => m.syncthing) && "Syncthing"].filter(Boolean).join(" · ");
}

/// A row of a project's "Where it lives". `kind` is:
/// - "here": a checkout in `folder`; `mac` when it is this Mac
/// - "checking": the server is looking through it now
/// - "none": online without a checkout, so it can be copied or cloned there
/// - "offline": offline without a checkout; it is looked through once it
///   connects
export interface WhereRow {
  kind: "here" | "checking" | "none" | "offline";
  id: string;
  name: string;
  folder: string | null;
  mac: boolean;
  checked_at?: number | null;
  /// What the row says when there is no checkout.
  text?: string;
  /// Another machine online can give it: Beep's sync copies it from there.
  copy?: boolean;
  /// The Syncthing folder's label when Syncthing keeps the folder in step.
  syncthing?: string | null;
}

/// The rows of a project's "Where it lives": every machine, as
/// `project_machines` gives them (null from a server too old to say, when
/// the imported folders are listed).
export function whereRows(project: { machines?: readonly ProjectPlace[] } | null | undefined, machines: readonly ProjectMachineRow[] | null | undefined, macId: string | null | undefined): WhereRow[] {
  const here = (id: string, name: string, folder: string): WhereRow => ({ kind: "here", id, name, folder, mac: !!macId && id === macId });
  if (!machines) return sortWhere((project?.machines || []).map(m => here(m.id, m.name, m.folder)));
  return sortWhere(machines.map((m): WhereRow => {
    if (m.folder) return { ...here(m.id, m.name, m.folder), syncthing: m.syncthing || null };
    const one = { id: m.id, name: m.name, folder: null, mac: false, checked_at: m.checked_at || null };
    if (m.checking) return { ...one, kind: "checking", text: "Checking…" };
    if (m.online) return { ...one, kind: "none", text: `Not on ${m.name}`, copy: machines.some(o => o.folder && o.online && !o.syncthing) };
    return { ...one, kind: "offline", text: `Checks when ${m.name} is online` };
  }));
}

/// The same order every time: this Mac, other checkouts, then the rest, each by name.
function sortWhere(rows: WhereRow[]) {
  const rank = (r: WhereRow) => r.mac ? 0 : r.kind === "here" ? 1 : 2;
  return rows.sort((a, b) => (rank(a) - rank(b)) || a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}

/// "2 of 5 machines" for the section's heading, or "2 machines" while
/// only the imported folders are known.
export function whereCount(rows: readonly Pick<WhereRow, "kind">[], known: boolean) {
  const n = rows.filter(r => r.kind === "here").length;
  return known && rows.length > n ? `${n} of ${plural(rows.length, "machine", "machines")}` : plural(n, "machine", "machines");
}

// ---------- The Projects overview ----------

/// The order a card's bar takes, work in hand first.
export const BAR_STATES = ["running", "review", "blocked", "planned", "done"] as const;

/// A card of the overview: the project row with `i` for its colour, its repo
/// name (the whole name under a group), whether it is pinned, and `sum`, the
/// server's overview of it (null when it has no project agent yet).
export type OverviewCardRow<T, S> = Placed<T> & { repo: string; pinned: boolean; sum: S | null };

/// A section of the overview: a group's, an org's, or (both null) the
/// projects in neither.
export interface OverviewGroup<T, S> {
  group: ProjectGroup | null;
  org: string | null;
  cards: OverviewCardRow<T, S>[];
}

/// The overview's cards grouped as the sidebar groups projects: every group
/// first, in order, even one with no projects yet, then by org, then projects
/// in neither under `org: null`.
export function overviewGroups<T extends Groupable, S extends { name: string } = OverviewCard>(rows: readonly T[] | null | undefined, overview: readonly S[] | null | undefined, top: readonly string[] | null | undefined, groups: readonly ProjectGroup[] | null | undefined = []): OverviewGroup<T, S>[] {
  const sums = new Map((overview || []).map(o => [o.name, o]));
  const pins = new Set(top || []);
  const byGroup = new Map<string, OverviewCardRow<T, S>[]>((groups || []).map(g => [g.id, []]));
  const byOrg = new Map<string | null, OverviewCardRow<T, S>[]>();
  (rows || []).forEach((p, i) => {
    const card = { ...p, i, pinned: pins.has(p.name), sum: sums.get(p.name) || null };
    const group = groupOf(p, groups);
    if (group) return byGroup.get(group.id)!.push({ ...card, repo: p.name });
    const cut = p.name.indexOf("/");
    const org = cut > 0 ? p.name.slice(0, cut) : null;
    if (!byOrg.has(org)) byOrg.set(org, []);
    byOrg.get(org)!.push({ ...card, repo: cut > 0 ? p.name.slice(cut + 1) : p.name });
  });
  const order = (cards: OverviewCardRow<T, S>[]) => cards.sort((a, b) => (Number(b.pinned) - Number(a.pinned)) || (a.i - b.i));
  const inGroups = (groups || []).map(group => ({ group, org: null, cards: order(byGroup.get(group.id)!) }));
  const inOrgs = [...byOrg].map(([org, cards]) => ({ group: null, org, cards: order(cards) }));
  return [...inGroups, ...inOrgs.filter(g => g.org), ...inOrgs.filter(g => !g.org)];
}

/// Tasks not done or dropped.
export function openTasks(counts: Partial<TaskCounts> | null | undefined) {
  const c = counts || {};
  return (c.running || 0) + (c.review || 0) + (c.blocked || 0) + (c.planned || 0);
}

/// The bar's segments, `{ state, n, share }` with share in percent, empty
/// states left out.
export function barSegments(counts: TaskCounts | null | undefined) {
  const total = openTasks(counts) + ((counts && counts.done) || 0);
  if (!total || !counts) return [];
  return BAR_STATES.filter(k => counts[k]).map(k => ({ state: k, n: counts[k], share: counts[k] / total * 100 }));
}

/// "2 open · 1 in review · 1 blocked · 3 done", the parts after "open"
/// only when there are some.
export function countsLine(counts: Partial<TaskCounts> | null | undefined) {
  const c = counts || {};
  const parts = [`${openTasks(c)} open`];
  if (c.review) parts.push(`${c.review} in review`);
  if (c.blocked) parts.push(`${c.blocked} blocked`);
  parts.push(`${c.done || 0} done`);
  return parts;
}

/// What the task that changed last says about the project.
export function latestLine(latest: (Pick<LatestTask, "slug" | "title" | "state"> & { pr_url?: string | null }) | null | undefined) {
  if (!latest) return "No tasks yet";
  const t = latest.title || latest.slug;
  switch (latest.state) {
    case "running": return `Working on ${t}`;
    case "blocked": return `${latest.slug || t} is blocked`;
    case "review": return latest.pr_url ? `${t}: its PR waits for your review` : `${t} waits for your review`;
    case "planned": return `Next: ${t}`;
    case "done": return `Finished ${t}`;
    default: return t;
  }
}

/// "1 worker", "2 workers", or "idle".
export function workersLabel(n: number) {
  return n ? plural(n, "worker", "workers") : "idle";
}

/// The heading's line: "3 projects in 2 orgs, 3 workers running", or
/// "4 projects in 1 group and 2 orgs, ..." with groups.
export function overviewSub(groups: readonly { group?: unknown; org: string | null; cards: readonly { sum: { workers?: number } | null }[] }[]) {
  const cards = groups.flatMap(g => g.cards);
  const orgs = groups.filter(g => g.org).length;
  const named = groups.filter(g => g.group).length;
  const workers = cards.reduce((n, c) => n + (c.sum?.workers || 0), 0);
  const projects = plural(cards.length, "project", "projects");
  const parts = [named && plural(named, "group", "groups"), orgs && plural(orgs, "org", "orgs")].filter(Boolean);
  const where = parts.length ? ` in ${parts.join(" and ")}` : "";
  return `${projects}${where}, ${workers ? `${plural(workers, "worker", "workers")} running` : "no workers running"}`;
}
