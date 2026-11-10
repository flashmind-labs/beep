// The Projects overview: every project as a card, grouped as the sidebar
// groups them, with its task bar and lines, as pure functions so they can be
// tested without Tauri.
import { plural } from "../ui/format.ts";
import { groupOf, type Groupable, type LatestTask, type OverviewCard, type Placed, type Project, type ProjectGroup, type SessionRow, type TaskCounts } from "./projects-state.ts";

/// The order a card's bar takes, work in hand first.
const BAR_STATES = ["running", "review", "blocked", "planned", "done"] as const;

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
/// in neither under `org: null`. In each, pinned cards come first, then the
/// most recent by `activity` (milliseconds by name), then by name.
export function overviewGroups<T extends Groupable, S extends { name: string } = OverviewCard>(rows: readonly T[] | null | undefined, overview: readonly S[] | null | undefined, top: readonly string[] | null | undefined, groups: readonly ProjectGroup[] | null | undefined = [], activity: ReadonlyMap<string, number> = new Map()): OverviewGroup<T, S>[] {
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
  const at = (card: OverviewCardRow<T, S>) => activity.get(card.name) ?? 0;
  const order = (cards: OverviewCardRow<T, S>[]) => cards.sort((a, b) => (Number(b.pinned) - Number(a.pinned)) || (at(b) - at(a)) || (a.i - b.i));
  const inGroups = (groups || []).map(group => ({ group, org: null, cards: order(byGroup.get(group.id)!) }));
  const inOrgs = [...byOrg].map(([org, cards]) => ({ group: null, org, cards: order(cards) }));
  return [...inGroups, ...inOrgs.filter(g => g.org), ...inOrgs.filter(g => !g.org)];
}

/// When something last happened in a project, in milliseconds, 0 for never:
/// the latest of its agents' activity (running or in its session list), the
/// user's last visit and its task that changed last.
export function projectActivity(project: Pick<Project, "last">, sessions: readonly Pick<SessionRow, "last_activity">[] | null | undefined, visited: number | null | undefined, sum: { latest: Pick<LatestTask, "updated_at"> | null } | null | undefined) {
  let at = Math.max(project.last ?? 0, visited ?? 0, sum?.latest?.updated_at ?? 0);
  for (const s of sessions ?? []) if (s.last_activity > at) at = s.last_activity;
  return at;
}

/// `groups` with only the cards whose name, folder on any machine or remote
/// holds `query`, ignoring case; groups left without a card go. An empty
/// query keeps everything.
export function searchOverview<T extends { name: string; url?: string | null; machines?: readonly { folder: string }[] | null }, S>(groups: readonly OverviewGroup<T, S>[], query: string): OverviewGroup<T, S>[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...groups];

  const hit = (card: T) => card.name.toLowerCase().includes(q) || !!card.url?.toLowerCase().includes(q) || !!card.machines?.some(m => m.folder.toLowerCase().includes(q));
  return groups.map(g => ({ ...g, cards: g.cards.filter(hit) })).filter(g => g.cards.length);
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
export function overviewSub(groups: readonly { group?: unknown; org: string | null; cards: readonly { agents?: number; sum: { workers?: number } | null }[] }[]) {
  const cards = groups.flatMap(g => g.cards);
  const orgs = groups.filter(g => g.org).length;
  const named = groups.filter(g => g.group).length;
  const workers = cards.reduce((n, c) => n + (c.sum?.workers ?? c.agents ?? 0), 0);
  const unit = cards.some(c => c.sum) ? "worker" : "agent";
  const projects = plural(cards.length, "project", "projects");
  const parts = [named && plural(named, "group", "groups"), orgs && plural(orgs, "org", "orgs")].filter(Boolean);
  const where = parts.length ? ` in ${parts.join(" and ")}` : "";
  return `${projects}${where}, ${workers ? `${plural(workers, unit, `${unit}s`)} running` : `no ${unit}s running`}`;
}
