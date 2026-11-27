import { projectName } from "./projects-state.ts";
// The Projects overview: every project as a card, grouped as the sidebar
// groups them, as pure functions so they can be tested without Tauri.
import { plural } from "../../ui/format.ts";
import { groupOf, type Groupable, type Placed, type Project, type ProjectGroup, type SessionRow } from "./projects-state.ts";

export const PROJECT_SORTS = [
  { id: "default", label: "Default" },
  { id: "activity", label: "Recent activity" },
  { id: "opened", label: "Last opened" },
] as const;
export type ProjectSort = typeof PROJECT_SORTS[number]["id"];

export function parseProjectSort(value: string | null): ProjectSort {
  return PROJECT_SORTS.find(sort => sort.id === value)?.id ?? "default";
}

export function projectSortTime(sort: ProjectSort, project: Pick<Project, "last">, sessions: readonly Pick<SessionRow, "last_activity">[] | null, visited: number | null): number {
  switch (sort) {
    case "default": return projectActivity(project, sessions, visited);
    case "activity": return projectActivity(project, sessions, null);
    case "opened": return visited ?? 0;
  }
}

/// A card of the overview: the project row with `i` for its colour, its repo
/// name (the whole name under a group), and whether it is pinned.
export type OverviewCardRow<T> = Placed<T> & { repo: string; pinned: boolean };

/// A section of the overview: a group's, an org's, or (both null) the
/// projects in neither.
export interface OverviewGroup<T> {
  group: ProjectGroup | null;
  org: string | null;
  cards: OverviewCardRow<T>[];
}

/// The overview's cards grouped as the sidebar groups projects: every group
/// first, in order, even one with no projects yet, then by org, then projects
/// in neither under `org: null`. In each, pinned cards come first, then the
/// most recent by `activity` (milliseconds by name), then by name.
export function overviewGroups<T extends Groupable>(rows: readonly T[] | null | undefined, top: readonly string[] | null | undefined, groups: readonly ProjectGroup[] | null | undefined = [], activity: ReadonlyMap<string, number> = new Map()): OverviewGroup<T>[] {
  const pins = new Set(top || []);
  const byGroup = new Map<string, OverviewCardRow<T>[]>((groups || []).map(g => [g.id, []]));
  const byOrg = new Map<string | null, OverviewCardRow<T>[]>();
  (rows || []).forEach((p, i) => {
    const card = { ...p, i, pinned: pins.has(p.name) };
    const group = groupOf(p, groups);
    if (group) return byGroup.get(group.id)!.push({ ...card, repo: projectName(p) });
    const label = projectName(p);
    const cut = label.indexOf("/");
    const org = cut > 0 ? label.slice(0, cut) : null;
    if (!byOrg.has(org)) byOrg.set(org, []);
    byOrg.get(org)!.push({ ...card, repo: cut > 0 ? label.slice(cut + 1) : label });
  });
  const at = (card: OverviewCardRow<T>) => activity.get(card.name) ?? 0;
  const order = (cards: OverviewCardRow<T>[]) => cards.sort((a, b) => (Number(b.pinned) - Number(a.pinned)) || (at(b) - at(a)) || (a.i - b.i));
  const inGroups = (groups || []).map(group => ({ group, org: null, cards: order(byGroup.get(group.id)!) }));
  const inOrgs = [...byOrg].map(([org, cards]) => ({ group: null, org, cards: order(cards) }));
  return [...inGroups, ...inOrgs.filter(g => g.org), ...inOrgs.filter(g => !g.org)];
}

/// When something last happened in a project, in milliseconds, 0 for never:
/// the latest of its agents' activity (running or in its session list) and
/// the user's last visit.
export function projectActivity(project: Pick<Project, "last">, sessions: readonly Pick<SessionRow, "last_activity">[] | null | undefined, visited: number | null | undefined) {
  let at = Math.max(project.last ?? 0, visited ?? 0);
  for (const s of sessions ?? []) if (s.last_activity > at) at = s.last_activity;
  return at;
}

/// `groups` with only the cards whose name, folder on any machine or remote
/// holds `query`, ignoring case; groups left without a card go. An empty
/// query keeps everything.
export function searchOverview<T extends { name: string; display_name?: string | null; url?: string | null; machines?: readonly { folder: string }[] | null }>(groups: readonly OverviewGroup<T>[], query: string): OverviewGroup<T>[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...groups];

  const hit = (card: T) => (projectName(card).toLowerCase().includes(q) || card.name.toLowerCase().includes(q)) || !!card.url?.toLowerCase().includes(q) || !!card.machines?.some(m => m.folder.toLowerCase().includes(q));
  return groups.map(g => ({ ...g, cards: g.cards.filter(hit) })).filter(g => g.cards.length);
}

/// The heading's line: "3 projects in 2 orgs, 3 agents running", or
/// "4 projects in 1 group and 2 orgs, ..." with groups.
export function overviewSub(groups: readonly { group?: unknown; org: string | null; cards: readonly { agents?: number }[] }[]) {
  const cards = groups.flatMap(g => g.cards);
  const orgs = groups.filter(g => g.org).length;
  const named = groups.filter(g => g.group).length;
  const agents = cards.reduce((n, c) => n + (c.agents ?? 0), 0);
  const projects = plural(cards.length, "project", "projects");
  const parts = [named && plural(named, "group", "groups"), orgs && plural(orgs, "org", "orgs")].filter(Boolean);
  const where = parts.length ? ` in ${parts.join(" and ")}` : "";
  return `${projects}${where}, ${agents ? `${plural(agents, "agent", "agents")} running` : "no agents running"}`;
}
