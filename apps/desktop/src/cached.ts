// What the window last drew from the server, kept by the app so a window
// that opens shows Home, the projects and the sidebar at once, then
// updates them in place once the server answers.
import type { HomeView, SyncRow, UpdateRow } from "./home.ts";
import type { OverviewCard, ProjectGroup } from "./projects-state.ts";
import type { ProjectAgent } from "./project.ts";

/// The views as `center_cached` hands them over; one never kept is missing.
export interface CachedViews {
  home?: HomeView;
  top?: string[];
  groups?: ProjectGroup[] | null;
  sync?: SyncRow[];
  agents?: ProjectAgent[] | null;
  overview?: OverviewCard[] | null;
  /// What the project pages last read, as `pagecache.ts` saved it.
  pages?: unknown;
}

/// The parts of the window's state the kept views fill.
export interface Seeded {
  home: HomeView | null;
  top: string[];
  groups: ProjectGroup[] | null | undefined;
  sync: SyncRow[];
  agents: ProjectAgent[] | null | undefined;
  overview: OverviewCard[] | null | undefined;
}

/// Fills in from `cached` what the server has not answered yet, leaving what it
/// has.
export function seed(state: Seeded, cached: CachedViews | null, dismissed: Set<number>): boolean {
  if (!cached) return false;
  let home = false;
  // Home, the pins, groups and sync come in one answer: none of them is in
  // before Home is.
  if (!state.home && cached.home) {
    const kept = cached.home;
    state.home = kept.updates ? { ...kept, updates: kept.updates.filter((u: UpdateRow) => !dismissed.has(u.id)) } : kept;
    if (cached.top) state.top = cached.top;
    if (state.groups === undefined && cached.groups !== undefined) state.groups = cached.groups;
    if (cached.sync) state.sync = cached.sync;
    home = true;
  }
  if (state.agents === undefined && cached.agents !== undefined) state.agents = cached.agents;
  if (state.overview === undefined && cached.overview !== undefined) state.overview = cached.overview;
  return home;
}
