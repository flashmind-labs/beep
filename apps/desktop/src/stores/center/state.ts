import type { ReactNode } from "react";
// The command center window's shared state: what Home and the projects
// last read, and the page cache kept between launches.
import { PageCache, freshness, type PageSection, type SectionStore } from "../../ui/pagecache.ts";
import { invoke } from "../../ui/tauri.ts";
import { signal } from "../../ui/signal.ts";
import type { PaneTarget } from "../../shell/bar/pane.ts";
import { withListedMachines, type HomeView } from "./home-state.ts";
import { MACHINES, REMOVED } from "../machines/machine-store.ts";
import type { FoundRow, ProjectGroup, ProjectMachineRow, SessionRow } from "../project/projects-state.ts";
import type { GitItemsView } from "./openitems-state.ts";
import type { MacControl } from "../../parts/machines/machine-state.ts";
import type { SyncRow } from "../../parts/project/filesync.ts";
import type { GitRow } from "../project/gitsync-state.ts";

/// What `agent_resume` gives.
export interface Started {
  session: SessionRow;
  why: string;
}

// ---------- Window state ----------

/// What a view's mount gets: a pane target, or the Projects page's pick.
interface ViewArgs {
  home: null;
  pane: PaneTarget;
  projects: ProjectsArg | null;
  providers: ProvidersArg | null;
  instructions: InstructionsArg | null;
  terminals: null;
}

interface ProvidersArg { agent: string | null; settings: boolean }
export type InstructionsTab = "editor" | "history";
interface InstructionsArg { tab: InstructionsTab | null }
/// The names of the views the window can show.
export type ViewName = keyof ViewArgs;
export type ViewArg = ViewArgs[ViewName];
export type Location = { [K in ViewName]: { view: K; arg: ViewArgs[K] } }[ViewName];
export type NavigationArgs = { [K in ViewName]: null extends ViewArgs[K]
  ? [view: K, arg?: ViewArgs[K]]
  : [view: K, arg: ViewArgs[K]] }[ViewName];
/// `run` opens that run on the Actions tile.
export interface ProjectsArg { project: string | null; tile?: string | null; run?: RunAt | null }
interface RunAt { id: number; host: string }

/// A view's header: its title and line, buttons, and an element beside the title.
export interface ViewHead { title: string; sub?: string; reactTools?: ReactNode; reactScope?: ReactNode }

export interface CenterView<K extends ViewName = ViewName> {
  title?: string | (() => ViewHead);
  mount(el: HTMLElement, arg: ViewArgs[K]): void | Promise<void>;
  unmount?(): void;
}

export const OFFLINE_NOTE_MS = 60 * 60 * 1000;
/// Before Home first loads: how long "Connecting" shows before "Still
/// connecting".
export const CONNECT_STUCK_MS = 15000;

/// The Home the window holds, as it was read. `state.home` derives what Home shows from it each
/// time it is read: the machine list's names, and no machine this window removed.
let homeRaw: HomeView | null = null;
let derived: { raw: HomeView; listed: unknown; removed: string; home: HomeView } | null = null;

/// The Home as it was read or edited, before the machine list is applied.
export const heldHome = () => homeRaw;

/// `since` is when the window started waiting for its first Home.
export const state = {
  get home(): HomeView | null {
    if (!homeRaw) return null;
    const removed = [...REMOVED].join("\n");
    if (derived?.raw === homeRaw && derived.listed === MACHINES.list && derived.removed === removed) return derived.home;

    derived = { raw: homeRaw, listed: MACHINES.list, removed, home: withListedMachines(homeRaw, MACHINES.list, REMOVED) };
    return derived.home;
  },
  set home(home: HomeView | null) { homeRaw = home; },
  error: null as string | null,
  since: Date.now(),
  picked: null as string | null,
  found: null as FoundRow[] | null,
  importing: null as string | null,
  foundError: null as { name: string; message: string } | null,
  sync: [] as SyncRow[],
  git: null as GitItemsView | null,
  /// Why `git_items` threw, when it did.
  gitError: null as string | null,
  top: [] as string[],
  groups: undefined as ProjectGroup[] | null | undefined,
  renaming: null as { id: string; draft: string } | null,
};
/// Home's data changed: the read, the machines and their names, the sessions and the open items.
export const homeDataChanged = signal();
/// The projects, pins, groups, conflicts or machines the sidebar lists changed.
export const listsChanged = signal();
/// A project or the list of them was read again, for the page that shows it.
export const projectLoaded = signal();
/// The sessions kept for a project changed, or for every project when `null`.
export const sessionsChanged = signal<string | null>();
/// Conflict counts changed, in the sidebar and in Home's conflict cards.
export const syncChanged = signal();
/// What each project's page last read, section by section: drawn at once when a
/// project opens, then read again quietly.
export const pages = new PageCache(null);
/// Projects whose sessions are read again when opened, though their kept list shows meanwhile.
export const sessionsDue = new Set<string>();
/// The saved cache from `center_cached`, until this Mac's account is known.
let pagesFile: unknown;
export function keepPagesFile(file: unknown) { pagesFile = file; }
const PAGES_SAVE_MS = 5000;
let pagesTimer: ReturnType<typeof setTimeout> | undefined;

function keepPages() {
  clearTimeout(pagesTimer);
  pagesTimer = setTimeout(() => {
    if (!pages.takeChanged()) return;
    const file = pages.toFile();
    if (file) invoke("center_keep_pages", { pages: file }).catch(e => console.warn("keeping the project pages:", e));
  }, PAGES_SAVE_MS);
}

/// Keeps `data` as what `section` of `id` read now.
export function keep(section: PageSection, id: string, data: unknown) {
  pages.put(section, id, data);
  keepPages();
}

/// `section` of the cache for threads.ts and review.ts, saved on change.
export function keptSection<T>(section: PageSection): SectionStore<T> {
  const store = pages.section<T>(section);
  return { ...store, put: (id, data) => { store.put(id, data); keepPages(); } };
}

/// Takes this Mac's account once known: the saved cache comes in for it,
/// and another account's goes.
export function pagesAccount(mac: MacControl | null) {
  if (!mac?.id) return;

  if (pagesFile !== undefined) {
    pages.restore(pagesFile, mac.id);
    pagesFile = undefined;
  } else {
    pages.setAccount(mac.id);
  }
}

/// Project `name`'s sessions, where it lives and its checkouts' branches as last read or
/// pushed: undefined before any, null when the first read of where it lives failed. Held in
/// `pages` only, which also drops the projects used least.
export const sessionsOf = (name: string) => pages.get<SessionRow[]>("sessions", name);
/// The same without counting the project as used, for lists that read every project.
export const peekSessions = (name: string) => pages.peek<SessionRow[]>("sessions", name);
export const whereOf = (name: string) => pages.get<ProjectMachineRow[] | null>("where", name);
export const gitOf = (name: string) => pages.get<GitRow[]>("gitsync", name);

/// A section header's note for `section` of `id`, `loading` while it reads.
export function pageFresh(section: PageSection, id: string, loading: boolean) {
  return freshness(pages.at(section, id), loading, Date.now());
}

/// A sidebar group.
export interface SidebarGroup<T> {
  order: number;
  title?: string;
  /// The group's rows from what the window holds, read at each draw of the sidebar; null hides the group.
  load(): T[] | null;
  key(item: T): string;
  render(item: T): ReactNode;
  onTitle?(): void;
  action?: { label: string; run(): void; title?: boolean };
  empty?: string | (() => string);
}

/// A group as drawn: `items` is what `load` gave at this draw.
export interface GroupSlot<T> extends SidebarGroup<T> {
  title: string;
  items: T[] | null;
}

export const groups = new Map<string, SidebarGroup<unknown>>();

/// True once the first Home has been waited for too long.
export function stuck() { return !state.home && Date.now() - state.since >= CONNECT_STUCK_MS; }
