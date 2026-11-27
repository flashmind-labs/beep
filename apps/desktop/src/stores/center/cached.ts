// What the window last drew, kept by the app so a window that opens shows
// Home, the projects and the sidebar at once, then updates them in place
// once the mesh answers.
import type { HomeView } from "./home-state.ts";
import type { ProjectGroup } from "../project/projects-state.ts";
import type { SyncRow } from "../../parts/project/filesync.ts";

/// The views as `center_cached` hands them over; one never kept is missing.
export interface CachedViews {
  home?: HomeView;
  top?: string[];
  groups?: ProjectGroup[] | null;
  sync?: SyncRow[];
  /// What the project pages last read, as `pagecache.ts` saved it.
  pages?: unknown;
}

/// The parts of the window's state the kept views fill.
export interface Seeded {
  home: HomeView | null;
  top: string[];
  groups: ProjectGroup[] | null | undefined;
  sync: SyncRow[];
}

/// Fills in from `cached` what has not been read yet, leaving what has.
/// Home, the pins, groups and sync come in one answer: none of them is in
/// before Home is.
export function seed(state: Seeded, cached: CachedViews | null): boolean {
  if (!cached?.home || state.home) return false;
  // Nothing kept is live yet: each machine connects until Home is read.
  state.home = { ...cached.home, machines: cached.home.machines?.map(m => ({ ...m, online: false, connecting: true })) ?? null };
  if (cached.top) state.top = cached.top;
  if (state.groups === undefined && cached.groups !== undefined) state.groups = cached.groups;
  if (cached.sync) state.sync = cached.sync;
  return true;
}
