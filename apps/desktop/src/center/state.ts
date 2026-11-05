// The command center window's shared state: what Home and the projects
// last read, and the page cache kept between launches.
import { PageCache, freshness, type PageSection, type SectionStore } from "../ui/pagecache.ts";
import { invoke } from "../ui/tauri.ts";
import type { pane, PaneTarget } from "../bar/pane.ts";
import type { HomeView } from "./home-state.ts";
import type { MachineTraffic } from "../machines/traffic-state.ts";
import type { FoundRow, OverviewCard, Placed, Project, ProjectGroup, ProjectMachineRow, SessionRow } from "../project/projects-state.ts";
import type { GitItemsView } from "./openitems-state.ts";
import type { ProjectAgent, ProjectView } from "../project/agent.ts";
import type { MacControl } from "../machines/machine-state.ts";
import type { SyncRow } from "../project/filesync.ts";
import type { GitRow } from "../project/gitsync-state.ts";

/// What `agent_resume` gives.
export interface Started {
  session: SessionRow;
  why: string;
}

// ---------- Window state ----------

/// What a view's mount gets: a pane target, the Projects page's pick, or a research.
export type ViewArg = PaneTarget | ProjectsArg | ResearchArg | undefined;
export interface ProjectsArg { project: string | null }
/// The research an opened research page shows.
export interface ResearchArg { session: string }

/// A view's header: its title and line, buttons, and an element beside the title.
export interface ViewHead { title: string; sub?: string; tools?: Node[]; scope?: Node }

export interface CenterView {
  el?: HTMLElement;
  title?: string | (() => ViewHead);
  mount(el: HTMLElement, arg: ViewArg): void;
  unmount?(): void;
}

/// How a Needs you card or a project card was answered.
export interface Outcome { said?: string; error?: string; final?: boolean }

/// A chat or group in the sidebar, or a project pinned among them.
export type ChatItem = Awaited<ReturnType<typeof pane.chatRows>>[number];
export interface ProjectPin { kind: "project"; id: string; title: string; pinned: true; project: Placed<Project> }
export type SideChat = ChatItem | ProjectPin;

export const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;

export const OFFLINE_NOTE_MS = 60 * 60 * 1000;
/// Before Home first loads: how long "Connecting" shows before "Still
/// connecting", and how soon a failed try is made again.
export const CONNECT_STUCK_MS = 15000;
export const CONNECT_RETRY_MS = 3000;

/// `since` is when the window started waiting for its first Home.
export const state = {
  /// Signed in to a subscription: chats, memories and the rest of the assistant. Without one the
  /// window runs on the mesh alone.
  subscribed: false,
  home: null as HomeView | null,
  mac: null as MacControl | null,
  error: null as string | null,
  outcomes: new Map<string, Outcome>(),
  since: Date.now(),
  picked: null as string | null,
  found: null as FoundRow[] | null,
  importing: null as string | null,
  foundError: null as { name: string; message: string } | null,
  sessions: new Map<string, SessionRow[]>(),
  where: new Map<string, ProjectMachineRow[] | null>(),
  /// Each project's checkouts' branches, as `project_git_state` read them.
  gitsync: new Map<string, GitRow[]>(),
  agents: undefined as ProjectAgent[] | null | undefined,
  pview: new Map<string, { view: ProjectView | null; error: string | null }>(),
  sync: [] as SyncRow[],
  traffic: [] as MachineTraffic[],
  git: null as GitItemsView | null,
  /// Why `git_items` threw, when it did.
  gitError: null as string | null,
  top: [] as string[],
  overview: undefined as OverviewCard[] | null | undefined,
  groups: undefined as ProjectGroup[] | null | undefined,
  renaming: null as { id: string; draft: string } | null,
};
/// What each project's page last read, section by section: drawn at once when a
/// project opens, then read again quietly.
export const pages = new PageCache(null);
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

/// Fills in project `name`'s sections from the cache where the window has
/// not read them yet.
export function warm(name: string) {
  if (!state.pview.get(name)?.view) {
    const view = pages.get<ProjectView>("view", name);
    if (view) state.pview.set(name, { view, error: state.pview.get(name)?.error ?? null });
  }
  if (!state.where.get(name)) {
    const where = pages.get<ProjectMachineRow[]>("where", name);
    if (where) state.where.set(name, where);
  }
  if (!state.gitsync.has(name)) {
    const git = pages.get<GitRow[]>("gitsync", name);
    if (git) state.gitsync.set(name, git);
  }
  if (!state.sessions.has(name)) {
    const sessions = pages.get<SessionRow[]>("sessions", name);
    if (sessions) state.sessions.set(name, sessions);
  }
}

/// A section header's note for `section` of `id`, `loading` while it reads.
export function pageFresh(section: PageSection, id: string, loading: boolean) {
  return freshness(pages.at(section, id), loading, Date.now());
}

/// The names of the views the window can show, one per `registerView`.
export type ViewName = "home" | "pane" | "projects" | "providers" | "research" | "researchopen" | "sync" | "instructions" | "terminals";

/// The views the window can show, by name.
export const views = new Map<ViewName, CenterView & { el: HTMLElement }>();

/// A sidebar group.
export interface SidebarGroup<T> {
  order: number;
  title?: string;
  load(): Promise<T[] | null>;
  row(item: T): HTMLElement;
  onPick?(item: T): void;
  onTitle?(): void;
  action?(): HTMLElement;
  empty?: string | (() => string | HTMLElement);
  foot?(): HTMLElement[];
}

export interface GroupSlot<T> extends SidebarGroup<T> {
  title: string;
  items: T[] | null;
  loading: boolean;
  again: boolean;
}

export const groups = new Map<string, GroupSlot<unknown>>();
