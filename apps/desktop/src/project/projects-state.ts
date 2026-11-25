import { dedupe, type MoveRecord } from "../agents/move-state.ts";
import { plural } from "../ui/format.ts";
import { syncedBySyncthing } from "./filesync.ts";
import type { AgentQuestion, RunningAgent } from "../agents/running-state.ts";
import type { HostRow } from "./runs.tsx";
import type { Persona } from "../agents/agents-state.ts";
// Projects as the window lists them, their sessions, machines and groups,
// as pure functions so they can be tested without Tauri.

/// One imported project on one machine (ProjectRow), as `import_project`
/// gives it and Home lists them.
export interface ProjectRow {
  /// `org/repo`.
  name: string;
  display_name: string | null;
  url: string;
  machine_id: string;
  machine_name: string;
  folder: string;
  cloned: boolean;
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
  display_name: string | null;
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
  /// Those of `folders` that are git checkouts.
  repos: string[];
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
  /// The machine is looking through it for a checkout now.
  checking: boolean;
  checked_at: number | null;
  /// The mesh holds the project, so Add reaches the machine.
  mesh?: boolean;
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
export function withSyncthing(machines: ProjectMachineRow[], places: readonly SyncthingPlace[]): ProjectMachineRow[] {
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
  /// The Home assistant.
  assistant: boolean;
  /// The custom agent it runs as.
  persona: Persona | null;
}

/// A session list as the desktop sends it when it changes: a project's, or Home's of every
/// project.
export interface SessionList {
  list: { project: string } | "home";
  sessions: SessionRow[];
}

/// Whether `cwd` is `folder` or inside it.
function within(cwd: string, folder: string) {
  const f = String(folder || "").replace(/[\\/]+$/, "");
  return !!f && (cwd === f || String(cwd || "").startsWith(f + "/"));
}

/// The imported projects, one row each, with the agents running in their
/// folders counted.
export function mergeProjects(
  imported: readonly (Pick<ProjectRow, "name" | "url" | "machine_id" | "machine_name" | "folder"> & Partial<Pick<ProjectRow, "display_name">>)[] | null | undefined,
  running: readonly RunningAgent[] | null | undefined,
): Project[] {
  const rows = new Map<string, Project>();

  for (const p of imported || []) {
    const row = rows.get(p.name) || { name: p.name, display_name: p.display_name ?? null, url: p.url, machines: [], agents: 0, last: null, imported: true };
    row.machines.push({ id: p.machine_id, name: p.machine_name, folder: p.folder });
    rows.set(p.name, row);
  }

  for (const r of (running || [])) {
    let row: Project | undefined;
    let deepest = -1;

    // The deepest folder holds it, as running-state's projectOf says.
    for (const p of rows.values()) {
      for (const m of p.machines) {
        if (m.id === r.machine_id && within(r.cwd, m.folder) && m.folder.length > deepest) { row = p; deepest = m.folder.length; }
      }
    }

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
const AGENT_NAMES: Record<string, string> = { claude: "Claude Code", codex: "Codex", pi: "pi", flsh: "flsh", agy: "Antigravity", opencode: "OpenCode" };

/// "Claude Code" for `claude`; an agent Beep does not know reads as named.
export function agentLabel(agent: string | null | undefined) { return AGENT_NAMES[agent ?? ""] || String(agent || ""); }

/// How many sessions a found row's agents ran, together.
export function foundSessions(row: Pick<FoundRow, "agents"> | null | undefined) {
  return (row?.agents || []).reduce((n, a) => n + (a.sessions || 0), 0);
}

/// What `import_project` takes for one place.
type ImportArgs = {
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
  return `Looking through Claude Code, Codex, pi, flsh, OpenCode and Antigravity sessions on ${on}…`;
}

/// The project's running sessions, including live rows read from its history
/// before Home has refreshed. Home supplies the full state when both have a row.
export function projectRunning(project: Pick<Project, "machines">, running: readonly RunningAgent[] | null | undefined, sessions: readonly SessionRow[] | null | undefined, moves: readonly MoveRecord[] = []): RunningAgent[] {
  const inside = (r: RunningAgent) => project.machines.some(m => r.machine_id === m.id && (r.cwd === m.folder || r.cwd.startsWith(m.folder + "/")));
  const live = (running || []).filter(inside);
  const known = new Set(live.map(r => `${r.machine_id} ${r.id}`));

  for (const s of sessions || []) {
    const key = `${s.machine_id} ${s.id}`;
    if (s.status === "ended" || known.has(key)) continue;
    known.add(key);
    live.push({ ...s, kind: "agent" });
  }
  return dedupe(live, moves);
}

/// Ended sessions, excluding any still listed as live, newest first.
export function pastSessions<T extends Pick<SessionRow, "machine_id" | "id" | "status" | "last_activity">>(sessions: readonly T[] | null | undefined, running: readonly { machine_id: string; id: string }[] | null | undefined) {
  const live = new Set((running || []).map(r => `${r.machine_id} ${r.id}`));
  return [...(sessions || [])]
    .filter(s => s.status === "ended" && !live.has(`${s.machine_id} ${s.id}`))
    .sort((a, b) => (b.last_activity || 0) - (a.last_activity || 0));
}

/// History excludes the source copy once a moved session lists on its target.
export function projectPastSessions(sessions: readonly SessionRow[] | null | undefined, running: readonly RunningAgent[], moves: readonly MoveRecord[]): SessionRow[] {
  const kept = new Set(dedupe<SessionRow | RunningAgent>([...(sessions ?? []), ...running], moves));
  return pastSessions((sessions ?? []).filter(s => kept.has(s)), running);
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

/// The header's buttons, one per forge the repository is on: each run host
/// with a page (the project's own first), and the project's page when no
/// host names its server. Hosts on one server count once. "GitHub" or
/// "Forgejo", or the server's host when two share a label.
export function forgeLinks(url: string | null | undefined, hosts: readonly Pick<HostRow, "host" | "kind" | "repo" | "primary" | "url">[] | null | undefined) {
  const server = (page: string) => { try { return new URL(page).hostname.toLowerCase(); } catch { return null; } };
  const found: { kind: HostRow["kind"]; url: string; server: string }[] = [];
  const add = (kind: HostRow["kind"], page: string | null | undefined, first = false) => {
    const at = page ? server(page) : null;
    if (!page || !at || found.some(f => f.server === at)) return;
    found[first ? "unshift" : "push"]({ kind, url: page, server: at });
  };

  for (const h of [...(hosts ?? [])].sort((a, b) => Number(b.primary) - Number(a.primary))) {
    add(h.kind, h.url ?? (h.kind === "github" ? `https://github.com/${h.repo}` : null));
  }

  if (url) add(server(url) === "github.com" ? "github" : "forgejo", url, true);
  const label = (kind: HostRow["kind"]) => (kind === "github" ? "GitHub" : "Forgejo");
  return found.map(f => {
    const shared = found.filter(o => label(o.kind) === label(f.kind)).length > 1;
    return { label: shared ? new URL(f.url).host : label(f.kind), url: f.url };
  });
}

/// The menu line and button that remove a project from Beep.
export const REMOVE_PROJECT = "Remove project";

/// What the sheet that asks before removing project `name` says.
export function removeProjectSheet(name: string) {
  return {
    title: `Remove ${name}?`,
    text: "Beep forgets it and stops syncing it.",
    note: "Files on your machines stay.",
    action: "Remove project",
  };
}

/// What the machines found of repository `repo`, null when none has it.
export function foundFor(found: readonly FoundRow[] | null | undefined, repo: string) {
  const name = repo.toLowerCase();
  return found?.find(f => f.name.toLowerCase() === name) ?? null;
}

/// The line under a project's name: its host, or where it came from.
export function projectLine(project: { imported: boolean; url?: string | null } | null | undefined) {
  if (!project) return "";
  if (!project.imported) return "From running agents";
  try { return new URL(String(project.url)).host; } catch { return ""; }
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
/// which each machine reads as its own home; `machine` null before one is
/// picked.
export function folderOn(cwd: string, machine: string | null, places: readonly { machine: string; cwd: string }[] | null | undefined) {
  const own = machine ? places?.find(p => p.machine === machine)?.cwd : null;
  return own || homeShort(cwd);
}

/// `path` written `~/...` when it is under a home folder, else as it is.
export function homeShort(path: string) {
  const p = String(path || "").trim();
  const rest = p ? homeRelative(p.replace(/\\/g, "/")) : null;
  if (rest == null) return p;
  return rest ? `~/${rest.replace(/\/+$/, "")}` : "~";
}

/// A folder as groups compare it, as `project_group_path` keeps
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
type SideProject<T> = Placed<T> & { repo?: string; org?: undefined; group?: undefined; folded?: undefined };

/// A project row that groups can place: its name and where it is.
export type Groupable = { name: string; i?: number; machines?: readonly { folder: string }[] | null };

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
      return inGroup.get(group.id)!.push({ ...p, i, repo: projectName(p) });
    }

    const label = projectName(p);
    const cut = label.indexOf("/");
    if (cut <= 0) return loose.push({ ...p, i });
    const org = label.slice(0, cut);
    if (!orgs.has(org)) orgs.set(org, []);
    orgs.get(org)!.push({ ...p, i, repo: label.slice(cut + 1) });
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
/// `project_sync` row names it and Syncthing does not have that folder,
/// Syncthing when `where`, the machines its page last read, says so; "" for
/// neither.
export function syncedBy(name: string, rows: readonly { project: string; error?: string | null }[], where: readonly { syncthing?: string | null }[] | null | undefined) {
  return [rows.some(r => r.project === name && !syncedBySyncthing(r)) && "Beep sync", where?.some(m => m.syncthing) && "Syncthing"].filter(Boolean).join(" · ");
}

/// A row of a project's "Where it lives". `kind` is:
/// - "here": a checkout in `folder`; `mac` when it is this Mac
/// - "checking": Beep is looking through it now
/// - "none": online without a checkout, so it can be added or cloned there
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
  /// Another machine online can give it, for a project the mesh holds: the
  /// Add sheet makes the folder.
  add?: boolean;
  /// The Syncthing folder's label when Syncthing keeps the folder in step; on
  /// a machine without it, when that holds every copy online to give it.
  syncthing?: string | null;
}

/// The rows of a project's "Where it lives": every machine, as
/// `project_machines` gives them; until they are read, the imported folders.
export function whereRows(project: { machines?: readonly ProjectPlace[] } | null | undefined, machines: readonly ProjectMachineRow[] | null | undefined, macId: string | null | undefined): WhereRow[] {
  const here = (id: string, name: string, folder: string): WhereRow => ({ kind: "here", id, name, folder, mac: !!macId && id === macId });
  if (!machines) return sortWhere((project?.machines || []).map(m => here(m.id, m.name, m.folder)));
  return sortWhere(machines.map((m): WhereRow => {
    if (m.folder) return { ...here(m.id, m.name, m.folder), syncthing: m.syncthing || null };
    const one = { id: m.id, name: m.name, folder: null, mac: false, checked_at: m.checked_at || null };
    if (m.checking) return { ...one, kind: "checking", text: "Checking…" };

    if (m.online) {
      const copies = machines.filter(o => o.folder && o.online);
      const free = copies.some(o => !o.syncthing);
      return { ...one, kind: "none", text: `Not on ${m.name}`, add: free && !!m.mesh, syncthing: free ? null : copies[0]?.syncthing || null };
    }

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

/// A project's visible name, including rows from older cached responses.
export function projectName(project: { name: string; display_name?: string | null }): string {
  return project.display_name ?? project.name;
}
